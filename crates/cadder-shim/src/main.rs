use anyhow::{Context, Result, anyhow};
use cadder_daemon::{
  CadderSession, CaddyBackendMode, DaemonLaunchOptions, RealCaddyResolver, RuntimePaths,
  RuntimeProfile, ensure_daemon_running_with_options, shim_privilege_diagnostic,
};
use cadder_protocol::{
  ActivationState, BasicResponse, EntrypointInstanceIdentity, EntrypointRegistration,
  HeartbeatEntrypointRequest, LogStreamIdentity, OwnerProcessIdentity, RegisterEntrypointRequest,
  RegisterEntrypointResponse, ShimRunMetadata, SourcePath, UnregisterEntrypointRequest,
  message_types, new_request_id,
};
use chrono::Utc;
use clap::Parser;
use std::{
  env,
  future::Future,
  path::PathBuf,
  process::{ExitCode, Stdio},
  sync::Arc,
  time::Duration,
};
use tokio::{process::Command, sync::Mutex, time::interval};

#[derive(Debug, Clone, Parser)]
#[command(
  name = "caddy",
  version,
  about = "Cadder PATH-facing Caddy shim",
  long_about = "Acts as the Cadder-managed caddy command. `caddy run` requires a running cadderd backend and registers the current project; other commands are classified by the shim policy table before they are delegated to the safely resolved real Caddy binary or rejected."
)]
struct ShimArgs {
  #[arg(long = "cadder-runtime-dir", hide = true)]
  runtime_dir: Option<PathBuf>,

  #[arg(
    long = "cadder-runtime-profile",
    hide = true,
    value_parser = RuntimeProfile::parse_cli
  )]
  runtime_profile: Option<RuntimeProfile>,

  #[arg(long = "cadder-daemon-path", hide = true)]
  daemon_path: Option<PathBuf>,

  #[arg(long = "cadder-real-caddy-command", hide = true)]
  real_caddy_command: Option<String>,

  #[arg(
    long = "cadder-caddy-backend",
    hide = true,
    value_parser = CaddyBackendMode::parse_cli
  )]
  caddy_backend: Option<CaddyBackendMode>,

  #[arg(
    value_name = "CADDY_ARGS",
    trailing_var_arg = true,
    allow_hyphen_values = true,
    help = "Arguments for the caddy command; `run` is managed by Cadder and other commands must have an explicit shim policy"
  )]
  caddy_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShimCommandPolicyKind {
  Managed,
  ReadOnlyInspection,
  ExplicitPassthrough,
  Unsupported,
}

#[derive(Debug, Clone, Copy)]
struct ShimCommandPolicyEntry {
  command: &'static str,
  kind: ShimCommandPolicyKind,
  rationale: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClassifiedShimCommand<'a> {
  command: &'a str,
  kind: ShimCommandPolicyKind,
  rationale: &'static str,
}

const fn policy_entry(
  command: &'static str,
  kind: ShimCommandPolicyKind,
  rationale: &'static str,
) -> ShimCommandPolicyEntry {
  ShimCommandPolicyEntry {
    command,
    kind,
    rationale,
  }
}

const SHIM_COMMAND_POLICY_TABLE: &[ShimCommandPolicyEntry] = &[
  policy_entry(
    "run",
    ShimCommandPolicyKind::Managed,
    "Registers the project definition with cadderd and keeps Cadder as runtime owner.",
  ),
  policy_entry(
    "adapt",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Reads a Caddy config and prints adapted JSON without mutating runtime state.",
  ),
  policy_entry(
    "build-info",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Reports real Caddy build metadata.",
  ),
  policy_entry(
    "environ",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Reports real Caddy environment information.",
  ),
  policy_entry(
    "help",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Displays command help.",
  ),
  policy_entry(
    "list-modules",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Reports installed real Caddy modules.",
  ),
  policy_entry(
    "validate",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Validates config input without applying it to Cadder-managed runtime state.",
  ),
  policy_entry(
    "version",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Reports real Caddy version metadata.",
  ),
  policy_entry(
    "completion",
    ShimCommandPolicyKind::ExplicitPassthrough,
    "Generates shell completion output without touching Cadder-managed state.",
  ),
  policy_entry(
    "file-server",
    ShimCommandPolicyKind::ExplicitPassthrough,
    "Starts an unmanaged one-shot real Caddy file server by explicit command.",
  ),
  policy_entry(
    "fmt",
    ShimCommandPolicyKind::ExplicitPassthrough,
    "Formats user-provided config files without touching Cadder-managed runtime state.",
  ),
  policy_entry(
    "manpage",
    ShimCommandPolicyKind::ExplicitPassthrough,
    "Generates manual page output without touching Cadder-managed state.",
  ),
  policy_entry(
    "reverse-proxy",
    ShimCommandPolicyKind::ExplicitPassthrough,
    "Starts an unmanaged one-shot real Caddy reverse proxy by explicit command.",
  ),
  policy_entry(
    "add-package",
    ShimCommandPolicyKind::Unsupported,
    "Mutates the real Caddy binary/module set outside Cadder release ownership.",
  ),
  policy_entry(
    "reload",
    ShimCommandPolicyKind::Unsupported,
    "Mutates real Caddy runtime state outside Cadder's generated config model.",
  ),
  policy_entry(
    "remove-package",
    ShimCommandPolicyKind::Unsupported,
    "Mutates the real Caddy binary/module set outside Cadder release ownership.",
  ),
  policy_entry(
    "start",
    ShimCommandPolicyKind::Unsupported,
    "Starts an unmanaged real Caddy runtime that can drift from cadderd ownership.",
  ),
  policy_entry(
    "stop",
    ShimCommandPolicyKind::Unsupported,
    "Stops real Caddy outside Cadder's runtime ownership boundary.",
  ),
  policy_entry(
    "trust",
    ShimCommandPolicyKind::Unsupported,
    "Mutates local trust stores outside the current Cadder shim contract.",
  ),
  policy_entry(
    "untrust",
    ShimCommandPolicyKind::Unsupported,
    "Mutates local trust stores outside the current Cadder shim contract.",
  ),
  policy_entry(
    "upgrade",
    ShimCommandPolicyKind::Unsupported,
    "Mutates the real Caddy binary outside Cadder release ownership.",
  ),
];

#[tokio::main]
async fn main() -> Result<ExitCode> {
  let args = ShimArgs::parse();
  if args
    .caddy_args
    .iter()
    .any(|arg| arg == "--cadder-shim-info")
  {
    println!(
      "{}",
      serde_json::json!({
          "role": "caddy-shim",
          "version": env!("CARGO_PKG_VERSION"),
          "executable": env::current_exe().ok().map(|path| path.display().to_string())
      })
    );
    return Ok(ExitCode::SUCCESS);
  }

  let caddy_backend = args
    .caddy_backend
    .map_or_else(CaddyBackendMode::from_env, Ok)?;
  let command_policy = classify_caddy_command(&args.caddy_args);

  if command_policy.kind == ShimCommandPolicyKind::Managed {
    run_managed(args).await
  } else if command_policy.kind == ShimCommandPolicyKind::Unsupported {
    Ok(reject_unsupported_caddy_command(command_policy))
  } else if caddy_backend == CaddyBackendMode::Mock {
    run_mock_caddy_command(&args.caddy_args).await
  } else {
    delegate_to_real_caddy(args.real_caddy_command, &args.caddy_args).await
  }
}

fn classify_caddy_command(args: &[String]) -> ClassifiedShimCommand<'_> {
  let command = normalized_caddy_command(args);
  if let Some(entry) = SHIM_COMMAND_POLICY_TABLE
    .iter()
    .find(|entry| entry.command == command)
  {
    return ClassifiedShimCommand {
      command,
      kind: entry.kind,
      rationale: entry.rationale,
    };
  }

  ClassifiedShimCommand {
    command,
    kind: ShimCommandPolicyKind::Unsupported,
    rationale: "No explicit Cadder shim policy entry exists for this Caddy command.",
  }
}

fn normalized_caddy_command(args: &[String]) -> &str {
  match args.first().map(String::as_str) {
    None | Some("--help" | "-h" | "help") => "help",
    Some("--version" | "-v" | "version") => "version",
    Some(command) => command,
  }
}

fn reject_unsupported_caddy_command(command: ClassifiedShimCommand<'_>) -> ExitCode {
  eprintln!(
    "Cadder shim does not support `caddy {}`. {}",
    command.command, command.rationale
  );
  ExitCode::FAILURE
}

async fn run_managed(args: ShimArgs) -> Result<ExitCode> {
  write_shim_privilege_warning();
  run_managed_until(args, tokio::signal::ctrl_c()).await
}

fn write_shim_privilege_warning() {
  if let Some(message) = shim_privilege_warning_text() {
    eprintln!("warning: {message}");
  }
}

fn shim_privilege_warning_text() -> Option<String> {
  shim_privilege_diagnostic("caddy shim")
    .map(|diagnostic| format!("{} {}", diagnostic.message, diagnostic.guidance))
}

async fn run_managed_until<F>(args: ShimArgs, shutdown: F) -> Result<ExitCode>
where
  F: Future<Output = std::io::Result<()>>,
{
  let paths = RuntimePaths::resolve_with_profile(args.runtime_dir.clone(), args.runtime_profile)?;
  let session = match open_managed_run_target(&args, &paths).await? {
    ManagedRunTarget::Cadder(session) => session,
    ManagedRunTarget::Exit(code) => return Ok(code),
  };
  let registration = build_registration(&args.caddy_args)?;
  let registration_id = registration.registration_id.clone();
  let shim_session_nonce = registration.entrypoint_instance.shim_session_nonce.clone();

  let response: RegisterEntrypointResponse = session
    .lock()
    .await
    .request(
      message_types::REGISTER_ENTRYPOINT_REQUEST,
      message_types::REGISTER_ENTRYPOINT_RESPONSE,
      &RegisterEntrypointRequest {
        request_id: new_request_id("shim-register"),
        registration,
      },
    )
    .await?;
  if !response.accepted {
    return Err(anyhow!(response.message));
  }

  let heartbeat_session = session.clone();
  let heartbeat_registration = registration_id.clone();
  let heartbeat_nonce = shim_session_nonce.clone();
  let heartbeat = tokio::spawn(async move {
    let mut interval = interval(Duration::from_secs(5));
    loop {
      interval.tick().await;
      let _response: Result<BasicResponse> = heartbeat_session
        .lock()
        .await
        .request(
          message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
          message_types::HEARTBEAT_ENTRYPOINT_RESPONSE,
          &HeartbeatEntrypointRequest {
            request_id: new_request_id("shim-heartbeat"),
            registration_id: heartbeat_registration.clone(),
            shim_session_nonce: heartbeat_nonce.clone(),
          },
        )
        .await;
    }
  });

  shutdown
    .await
    .context("wait for shutdown signal while registered with Cadder")?;
  heartbeat.abort();

  let _response: BasicResponse = session
    .lock()
    .await
    .request(
      message_types::UNREGISTER_ENTRYPOINT_REQUEST,
      message_types::UNREGISTER_ENTRYPOINT_RESPONSE,
      &UnregisterEntrypointRequest {
        request_id: new_request_id("shim-unregister"),
        registration_id,
        shim_session_nonce,
      },
    )
    .await?;

  Ok(ExitCode::SUCCESS)
}

enum ManagedRunTarget {
  Cadder(Arc<Mutex<CadderSession>>),
  Exit(ExitCode),
}

async fn open_managed_run_target(
  args: &ShimArgs,
  paths: &RuntimePaths,
) -> Result<ManagedRunTarget> {
  open_managed_run_target_with_starter(args, paths, start_missing_daemon_owned).await
}

async fn open_managed_run_target_with_starter<F, Fut>(
  args: &ShimArgs,
  paths: &RuntimePaths,
  start_daemon: F,
) -> Result<ManagedRunTarget>
where
  F: FnOnce(ShimArgs, RuntimePaths) -> Fut,
  Fut: Future<Output = Result<()>>,
{
  match CadderSession::connect(paths).await {
    Ok(session) => Ok(ManagedRunTarget::Cadder(Arc::new(Mutex::new(session)))),
    Err(error) if !daemon_error_indicates_not_running(&error) => {
      eprintln!("{}", managed_backend_unavailable_message(paths, &error));
      Ok(ManagedRunTarget::Exit(ExitCode::FAILURE))
    }
    Err(error) => {
      let attach_error = format_error_chain(&error);
      let caddy_backend = args
        .caddy_backend
        .map_or_else(CaddyBackendMode::from_env, Ok)?;
      let real_caddy_result = if caddy_backend == CaddyBackendMode::Real {
        match run_real_caddy_fallback(args.real_caddy_command.clone(), &args.caddy_args).await {
          Ok(code) => return Ok(ManagedRunTarget::Exit(code)),
          Err(error) => RecoveryStepResult::Failed(format_error_chain(&error)),
        }
      } else {
        RecoveryStepResult::Skipped(format!(
          "Caddy backend mode is `{}`",
          caddy_backend.as_str()
        ))
      };

      match start_daemon(args.clone(), paths.clone()).await {
        Ok(()) => match CadderSession::connect(paths).await {
          Ok(session) => Ok(ManagedRunTarget::Cadder(Arc::new(Mutex::new(session)))),
          Err(error) => {
            eprintln!(
              "{}",
              managed_recovery_failed_message(
                paths,
                &attach_error,
                &real_caddy_result,
                &RecoveryStepResult::Failed(format!(
                  "started cadderd, but attach failed: {}",
                  format_error_chain(&error)
                )),
              )
            );
            Ok(ManagedRunTarget::Exit(ExitCode::FAILURE))
          }
        },
        Err(error) => {
          eprintln!(
            "{}",
            managed_recovery_failed_message(
              paths,
              &attach_error,
              &real_caddy_result,
              &RecoveryStepResult::Failed(format_error_chain(&error)),
            )
          );
          Ok(ManagedRunTarget::Exit(ExitCode::FAILURE))
        }
      }
    }
  }
}

async fn start_missing_daemon(args: &ShimArgs, paths: &RuntimePaths) -> Result<()> {
  ensure_daemon_running_with_options(
    paths,
    DaemonLaunchOptions {
      explicit_daemon: args.daemon_path.clone(),
      runtime_profile: args.runtime_profile,
      real_caddy_command: args.real_caddy_command.clone(),
      caddy_backend: args.caddy_backend,
      shim_path: env::current_exe().ok(),
      ..DaemonLaunchOptions::default()
    },
  )
  .await
}

async fn start_missing_daemon_owned(args: ShimArgs, paths: RuntimePaths) -> Result<()> {
  start_missing_daemon(&args, &paths).await
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecoveryStepResult {
  Failed(String),
  Skipped(String),
}

async fn run_mock_caddy_command(args: &[String]) -> Result<ExitCode> {
  match args.first().map(String::as_str) {
    None | Some("--version" | "version") => {
      println!("mock-caddy dev backend");
      Ok(ExitCode::SUCCESS)
    }
    Some("adapt") => {
      println!(
        "{}",
        serde_json::json!({ "apps": { "http": { "servers": {} } } })
      );
      Ok(ExitCode::SUCCESS)
    }
    Some(command) => {
      eprintln!("Cadder mock Caddy backend does not execute external caddy command `{command}`.");
      Ok(ExitCode::FAILURE)
    }
  }
}

fn managed_recovery_failed_message(
  paths: &RuntimePaths,
  attach_error: &str,
  real_caddy_result: &RecoveryStepResult,
  daemon_result: &RecoveryStepResult,
) -> String {
  format!(
    "Cadder could not recover `caddy run` for backend runtime `{}`.\n\
     Initial attach failed: {attach_error}.\n\
     Real Caddy fallback: {}.\n\
     Daemon startup: {}.\n\
     Next: configure a safe real Caddy command, start `cadderd --background --runtime-dir \"{}\"`, or run `cadder daemon start` for the same runtime and retry.",
    paths.runtime_dir().display(),
    recovery_step_summary(real_caddy_result),
    recovery_step_summary(daemon_result),
    paths.runtime_dir().display(),
  )
}

fn recovery_step_summary(result: &RecoveryStepResult) -> String {
  match result {
    RecoveryStepResult::Failed(message) => format!("failed: {message}"),
    RecoveryStepResult::Skipped(message) => format!("skipped: {message}"),
  }
}

fn managed_backend_unavailable_message(paths: &RuntimePaths, error: &anyhow::Error) -> String {
  let runtime_dir = paths.runtime_dir().display();
  let retry = format!(
    "Start `cadderd --background --runtime-dir \"{}\"` explicitly, or run `cadder daemon start`, then retry `caddy run`.",
    runtime_dir
  );

  if daemon_error_indicates_not_running(error) {
    format!("Cadder backend `cadderd` is not running for runtime `{runtime_dir}`. {retry}")
  } else {
    format!(
      "Cadder could not attach `caddy run` to backend runtime `{runtime_dir}`: {}. {retry}",
      format_error_chain(error)
    )
  }
}

fn daemon_error_indicates_not_running(error: &anyhow::Error) -> bool {
  error.chain().any(|cause| {
    cause.downcast_ref::<std::io::Error>().is_some_and(|error| {
      matches!(
        error.kind(),
        std::io::ErrorKind::NotFound
          | std::io::ErrorKind::ConnectionRefused
          | std::io::ErrorKind::ConnectionAborted
          | std::io::ErrorKind::ConnectionReset
          | std::io::ErrorKind::UnexpectedEof
      )
    })
  })
}

fn format_error_chain(error: &anyhow::Error) -> String {
  let mut messages = error.chain().map(ToString::to_string).collect::<Vec<_>>();
  messages.dedup();
  messages.join(": ")
}

fn build_registration(args: &[String]) -> Result<EntrypointRegistration> {
  let now = Utc::now();
  let cwd = env::current_dir().context("resolve current directory")?;
  let (config_path, adapter) = parse_run_args(args, &cwd);
  let canonical_cwd = cwd.canonicalize().ok();
  let canonical_config = config_path.canonicalize().ok();
  let identity = EntrypointInstanceIdentity::new(now);
  let executable_path = env::current_exe().ok();

  Ok(EntrypointRegistration {
    registration_id: identity.instance_id.clone(),
    source_working_directory: SourcePath::new(
      cwd.display().to_string(),
      canonical_cwd.map(|path| path.display().to_string()),
    ),
    source_config_path: SourcePath::new(
      config_path.display().to_string(),
      canonical_config.map(|path| path.display().to_string()),
    ),
    registered_domains: Vec::new(),
    activation_state: ActivationState::Active,
    owner_process: OwnerProcessIdentity {
      process_id: std::process::id(),
      process_start_time_utc: now,
      shim_session_nonce: identity.shim_session_nonce.clone(),
      executable_path: executable_path.map(|path| path.display().to_string()),
    },
    log_stream: LogStreamIdentity::entrypoint(&identity.instance_id),
    shim_run: Some(ShimRunMetadata {
      adapter,
      raw_arguments: args.to_vec(),
      command_line: args.join(" "),
    }),
    created_at_utc: now,
    last_heartbeat_utc: now,
    entrypoint_instance: identity,
  })
}

fn parse_run_args(args: &[String], cwd: &std::path::Path) -> (PathBuf, Option<String>) {
  let mut config = None;
  let mut adapter = None;
  let mut iter = args.iter().skip(1);
  while let Some(arg) = iter.next() {
    match arg.as_str() {
      "--config" | "-c" => config = iter.next().map(PathBuf::from),
      "--adapter" | "-a" => adapter = iter.next().cloned(),
      _ => {}
    }
  }
  (config.unwrap_or_else(|| cwd.join("Caddyfile")), adapter)
}

async fn delegate_to_real_caddy(
  real_caddy_command: Option<String>,
  args: &[String],
) -> Result<ExitCode> {
  match run_real_caddy_fallback(real_caddy_command, args).await {
    Ok(code) => Ok(code),
    Err(error) => {
      eprintln!("{}", RealCaddyResolver::resolution_help(&error));
      Ok(ExitCode::FAILURE)
    }
  }
}

async fn run_real_caddy_fallback(
  real_caddy_command: Option<String>,
  args: &[String],
) -> Result<ExitCode> {
  let resolver = RealCaddyResolver::new(real_caddy_command);
  let binary = resolver.resolve()?;
  let status = Command::new(binary)
    .args(args)
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit())
    .status()
    .await
    .context("delegate command to real Caddy")?;
  Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_daemon::{
    CaddyConfigAdapter, CaddyConfigCoordinator, DaemonServer, DaemonState, ProcessRuntime,
  };
  use clap::CommandFactory;
  use std::{fs, path::Path, sync::Mutex as StdMutex};
  use tokio::{sync::watch, time::sleep};

  static TEST_ENV_LOCK: StdMutex<()> = StdMutex::new(());

  #[test]
  fn command_metadata_matches_release_identity() {
    let command = ShimArgs::command();

    assert_eq!(command.get_name(), "caddy");
    assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(
      command.get_about().map(ToString::to_string),
      Some(env!("CARGO_PKG_DESCRIPTION").to_string())
    );
  }

  #[test]
  fn short_help_uses_package_description() {
    let help = ShimArgs::command().render_help().to_string();

    assert!(
      help.contains(env!("CARGO_PKG_DESCRIPTION")),
      "short help output should include the package description: {help}"
    );
  }

  #[test]
  fn long_help_describes_managed_and_delegated_commands() {
    let help = ShimArgs::command().render_long_help().to_string();

    assert!(
      help.contains("`run` is managed by Cadder"),
      "long help output should describe managed caddy run behavior: {help}"
    );
    assert!(
      help.contains("delegated to the safely resolved real Caddy binary or rejected"),
      "long help output should describe command policy behavior: {help}"
    );
  }

  #[test]
  fn command_policy_table_classifies_core_caddy_command_paths() {
    let run = vec!["run".to_string()];
    let adapt = vec!["adapt".to_string()];
    let fmt = vec!["fmt".to_string()];
    let start = vec!["start".to_string()];
    let version = vec!["--version".to_string()];
    let unknown = vec!["frobnicate".to_string()];

    assert_eq!(
      classify_caddy_command(&run).kind,
      ShimCommandPolicyKind::Managed
    );
    assert_eq!(
      classify_caddy_command(&adapt).kind,
      ShimCommandPolicyKind::ReadOnlyInspection
    );
    assert_eq!(
      classify_caddy_command(&fmt).kind,
      ShimCommandPolicyKind::ExplicitPassthrough
    );
    assert_eq!(
      classify_caddy_command(&start).kind,
      ShimCommandPolicyKind::Unsupported
    );
    assert_eq!(classify_caddy_command(&version).command, "version");
    assert_eq!(
      classify_caddy_command(&unknown),
      ClassifiedShimCommand {
        command: "frobnicate",
        kind: ShimCommandPolicyKind::Unsupported,
        rationale: "No explicit Cadder shim policy entry exists for this Caddy command.",
      }
    );
  }

  #[test]
  fn shim_privilege_warning_text_is_available_for_elevated_managed_runs() {
    let _guard = TEST_ENV_LOCK.lock().unwrap();
    unsafe { env::set_var("CADDER_TEST_ELEVATED_CONTEXT", "elevated") };

    let message = shim_privilege_warning_text().unwrap();

    unsafe { env::remove_var("CADDER_TEST_ELEVATED_CONTEXT") };

    assert!(message.contains("caddy shim is running with elevated privileges"));
    assert!(message.contains("user that owns the Cadder runtime"));
    assert!(message.contains("IIS handoff"));
  }

  #[test]
  fn parses_config_and_adapter_flags() {
    let cwd = PathBuf::from("/project");
    let args = vec![
      "run".to_string(),
      "--config".to_string(),
      "Proxy.Caddyfile".to_string(),
      "--adapter".to_string(),
      "caddyfile".to_string(),
    ];

    let (config, adapter) = parse_run_args(&args, &cwd);

    assert_eq!(config, PathBuf::from("Proxy.Caddyfile"));
    assert_eq!(adapter.as_deref(), Some("caddyfile"));
  }

  #[test]
  fn parses_short_config_and_adapter_flags() {
    let cwd = PathBuf::from("/project");
    let args = vec![
      "run".to_string(),
      "-c".to_string(),
      "Caddy.alt".to_string(),
      "-a".to_string(),
      "json".to_string(),
    ];

    let (config, adapter) = parse_run_args(&args, &cwd);

    assert_eq!(config, PathBuf::from("Caddy.alt"));
    assert_eq!(adapter.as_deref(), Some("json"));
  }

  #[test]
  fn parse_run_args_defaults_to_caddyfile_in_working_directory() {
    let cwd = PathBuf::from("/project/site");
    let args = vec!["run".to_string()];

    let (config, adapter) = parse_run_args(&args, &cwd);

    assert_eq!(config, cwd.join("Caddyfile"));
    assert_eq!(adapter, None);
  }

  #[test]
  fn build_registration_captures_owner_and_run_metadata() {
    let args = vec![
      "run".to_string(),
      "--config".to_string(),
      "Proxy.Caddyfile".to_string(),
      "--adapter".to_string(),
      "caddyfile".to_string(),
    ];

    let registration = build_registration(&args).unwrap();

    assert_eq!(registration.activation_state, ActivationState::Active);
    assert_eq!(
      registration.registration_id,
      registration.entrypoint_instance.instance_id
    );
    assert_eq!(
      registration.owner_process.shim_session_nonce,
      registration.entrypoint_instance.shim_session_nonce
    );
    assert_eq!(
      registration.source_config_path.raw,
      PathBuf::from("Proxy.Caddyfile").display().to_string()
    );
    let run = registration.shim_run.unwrap();
    assert_eq!(run.adapter.as_deref(), Some("caddyfile"));
    assert_eq!(run.raw_arguments, args);
    assert_eq!(
      run.command_line,
      "run --config Proxy.Caddyfile --adapter caddyfile"
    );
  }

  #[tokio::test]
  async fn delegate_to_real_caddy_returns_failure_when_resolution_fails() {
    let code = delegate_to_real_caddy(Some("definitely-missing-caddy-binary".to_string()), &[])
      .await
      .unwrap();

    assert_eq!(code, ExitCode::FAILURE);
  }

  #[test]
  fn managed_backend_unavailable_message_explains_manual_backend_start() {
    let paths = RuntimePaths::resolve(Some(std::env::temp_dir().join("cadder-shim-test"))).unwrap();
    let error = anyhow::Error::from(std::io::Error::new(
      std::io::ErrorKind::ConnectionRefused,
      "connection refused",
    ));

    let message = managed_backend_unavailable_message(&paths, &error);

    assert!(message.contains("Cadder backend `cadderd` is not running"));
    assert!(message.contains("Start `cadderd --background --runtime-dir"));
    assert!(message.contains("retry `caddy run`"));
  }

  #[test]
  fn backend_error_helpers_classify_transport_and_protocol_failures() {
    let paths =
      RuntimePaths::resolve(Some(std::env::temp_dir().join("cadder-shim-protocol-test"))).unwrap();
    let protocol_error = anyhow!("protocol mismatch");
    let message = managed_backend_unavailable_message(&paths, &protocol_error);

    assert!(message.contains("could not attach `caddy run`"));
    assert!(message.contains("protocol mismatch"));
    for kind in [
      std::io::ErrorKind::NotFound,
      std::io::ErrorKind::ConnectionAborted,
      std::io::ErrorKind::ConnectionReset,
      std::io::ErrorKind::UnexpectedEof,
    ] {
      let error = anyhow::Error::from(std::io::Error::new(kind, "backend unavailable"));
      assert!(daemon_error_indicates_not_running(&error));
    }

    let duplicated = Err::<(), _>(anyhow!("same")).context("same").unwrap_err();
    assert_eq!(format_error_chain(&duplicated), "same");
    let nested = Err::<(), _>(anyhow!("inner")).context("outer").unwrap_err();
    assert_eq!(format_error_chain(&nested), "outer: inner");
  }

  #[tokio::test]
  async fn run_managed_returns_failure_when_backend_is_unavailable() {
    let runtime_dir = std::env::temp_dir().join(format!(
      "cadder-shim-missing-backend-{}",
      std::process::id()
    ));
    let missing_daemon = runtime_dir.join(fake_daemon_name_for_test());
    let code = run_managed(ShimArgs {
      runtime_dir: Some(runtime_dir),
      runtime_profile: None,
      daemon_path: Some(missing_daemon),
      real_caddy_command: None,
      caddy_backend: Some(CaddyBackendMode::Mock),
      caddy_args: vec!["run".to_string()],
    })
    .await
    .unwrap();

    assert_eq!(code, ExitCode::FAILURE);
  }

  #[tokio::test]
  async fn run_managed_uses_real_caddy_fallback_when_backend_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let fake_caddy = temp.path().join(fake_caddy_name_for_test());
    write_fake_caddy(&fake_caddy);

    let code = run_managed(ShimArgs {
      runtime_dir: Some(paths.runtime_dir().to_path_buf()),
      runtime_profile: None,
      daemon_path: Some(temp.path().join(fake_daemon_name_for_test())),
      real_caddy_command: Some(fake_caddy.display().to_string()),
      caddy_backend: None,
      caddy_args: vec!["run".to_string()],
    })
    .await
    .unwrap();

    assert_eq!(code, ExitCode::SUCCESS);
  }

  #[tokio::test]
  async fn open_managed_run_target_starts_missing_daemon_when_fallback_is_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths.clone()));
    let server = DaemonServer::new(paths.clone(), state);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let args = ShimArgs {
      runtime_dir: Some(paths.runtime_dir().to_path_buf()),
      runtime_profile: None,
      daemon_path: Some(temp.path().join(fake_daemon_name_for_test())),
      real_caddy_command: None,
      caddy_backend: Some(CaddyBackendMode::Mock),
      caddy_args: vec!["run".to_string()],
    };
    let starter = move |_args: ShimArgs, paths: RuntimePaths| async move {
      tokio::spawn(async move {
        let _ = server.run_until(shutdown_rx).await;
      });
      wait_for_backend(&paths).await;
      Ok(())
    };

    let target = open_managed_run_target_with_starter(&args, &paths, starter)
      .await
      .unwrap();

    match target {
      ManagedRunTarget::Cadder(session) => {
        let response: cadder_protocol::QueryStateResponse = session
          .lock()
          .await
          .request(
            message_types::QUERY_STATE_REQUEST,
            message_types::QUERY_STATE_RESPONSE,
            &cadder_protocol::QueryStateRequest {
              request_id: new_request_id("test-query"),
            },
          )
          .await
          .unwrap();
        assert!(response.accepted);
      }
      ManagedRunTarget::Exit(code) => panic!("expected Cadder session, got exit code {code:?}"),
    }
    let _ = shutdown_tx.send(true);
  }

  #[tokio::test]
  async fn open_managed_run_target_reports_complete_recovery_failure() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let args = ShimArgs {
      runtime_dir: Some(paths.runtime_dir().to_path_buf()),
      runtime_profile: None,
      daemon_path: Some(temp.path().join(fake_daemon_name_for_test())),
      real_caddy_command: Some("definitely-missing-caddy-binary".to_string()),
      caddy_backend: None,
      caddy_args: vec!["run".to_string()],
    };
    let starter =
      |_args: ShimArgs, _paths: RuntimePaths| async { Err(anyhow!("test daemon start failed")) };

    let target = open_managed_run_target_with_starter(&args, &paths, starter)
      .await
      .unwrap();

    match target {
      ManagedRunTarget::Exit(code) => assert_eq!(code, ExitCode::FAILURE),
      ManagedRunTarget::Cadder(_) => panic!("expected recovery failure"),
    }
  }

  #[tokio::test]
  async fn mock_caddy_command_handles_adapt_without_delegating_to_real_caddy() {
    let code = run_mock_caddy_command(&["adapt".to_string()])
      .await
      .unwrap();

    assert_eq!(code, ExitCode::SUCCESS);
  }

  #[tokio::test]
  async fn run_managed_registers_heartbeats_and_unregisters_on_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("run"))).unwrap();
    paths.ensure_dirs().unwrap();
    let fake_caddy = temp.path().join(fake_caddy_name_for_test());
    write_fake_caddy(&fake_caddy);
    fs::write(
      temp.path().join("Caddyfile"),
      "app.localhost { respond ok }",
    )
    .unwrap();
    let resolver = RealCaddyResolver::new(Some(fake_caddy.display().to_string()));
    let state = DaemonState::new(CaddyConfigCoordinator::new(
      CaddyConfigAdapter::new(resolver.clone()),
      ProcessRuntime::new(resolver, paths.clone()),
    ));
    let server = DaemonServer::new(paths.clone(), state.clone());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
      let _ = server.run_until(shutdown_rx).await;
    });
    wait_for_backend(&paths).await;
    let config_path = temp.path().join("Caddyfile");

    let code = run_managed_until(
      ShimArgs {
        runtime_dir: Some(paths.runtime_dir().to_path_buf()),
        runtime_profile: None,
        daemon_path: None,
        real_caddy_command: None,
        caddy_backend: None,
        caddy_args: vec![
          "run".to_string(),
          "--config".to_string(),
          config_path.display().to_string(),
          "--adapter".to_string(),
          "caddyfile".to_string(),
        ],
      },
      async { Ok(()) },
    )
    .await
    .unwrap();
    let snapshot = state.snapshot().await;

    assert_eq!(code, ExitCode::SUCCESS);
    assert!(snapshot.registrations.is_empty());
    assert_eq!(
      snapshot.config.status,
      cadder_protocol::ConfigApplyStatus::Idle
    );
    let _ = shutdown_tx.send(true);
  }

  async fn wait_for_backend(paths: &RuntimePaths) {
    for _ in 0..50 {
      if CadderSession::connect(paths).await.is_ok() {
        return;
      }
      sleep(Duration::from_millis(20)).await;
    }
    panic!("backend did not become ready");
  }

  #[cfg(windows)]
  fn fake_caddy_name_for_test() -> &'static str {
    "fake-caddy.cmd"
  }

  #[cfg(not(windows))]
  fn fake_caddy_name_for_test() -> &'static str {
    "fake-caddy"
  }

  #[cfg(windows)]
  fn fake_daemon_name_for_test() -> &'static str {
    "missing-cadderd.exe"
  }

  #[cfg(not(windows))]
  fn fake_daemon_name_for_test() -> &'static str {
    "missing-cadderd"
  }

  fn write_fake_caddy(path: &Path) {
    #[cfg(windows)]
    fs::write(
      path,
      r#"@echo off
if "%1"=="adapt" (
  echo {"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["app.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}
  exit /b 0
)
if "%1"=="run" (
  ping -n 2 127.0.0.1 >nul
  exit /b 0
)
if "%1"=="stop" (
  exit /b 0
)
if "%1"=="reload" (
  exit /b 0
)
exit /b 0
"#,
    )
    .unwrap();

    #[cfg(not(windows))]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::write(
        path,
        r#"#!/usr/bin/env sh
if [ "$1" = "adapt" ]; then
  printf '%s\n' '{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["app.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}'
  exit 0
fi
if [ "$1" = "run" ]; then
  sleep 1
  exit 0
fi
exit 0
"#,
      )
      .unwrap();
      let mut permissions = fs::metadata(path).unwrap().permissions();
      permissions.set_mode(0o755);
      fs::set_permissions(path, permissions).unwrap();
    }
  }
}
