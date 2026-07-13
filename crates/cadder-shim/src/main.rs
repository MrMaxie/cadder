use anyhow::{Context, Result, anyhow};
use cadder_daemon::{
  CadderSession, CaddyBackendMode, DaemonLaunchOptions, IpcClientError, IpcClientResult,
  RealCaddyResolver, RuntimePaths, RuntimeProfile, ensure_daemon_running_with_options,
  shim_privilege_diagnostic,
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
use tokio::{
  process::Command,
  sync::{Mutex, oneshot},
  time::interval,
};

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
  rejected_real_caddy_selector: Option<String>,

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

  if args.rejected_real_caddy_selector.is_some() {
    eprintln!(
      "Cadder did not run real Caddy.\nThe shim cannot select the executable, and no daemon or Caddy state changed.\nNext: Configure an absolute path in the per-user Cadder configuration, or start the daemon with --real-caddy."
    );
    return Ok(ExitCode::FAILURE);
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
    if command_policy.kind == ShimCommandPolicyKind::ReadOnlyInspection {
      write_read_only_real_caddy_inspection_notice(&args, command_policy).await;
    }
    delegate_to_real_caddy(&args).await
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

async fn write_read_only_real_caddy_inspection_notice(
  args: &ShimArgs,
  command: ClassifiedShimCommand<'_>,
) {
  let Ok(paths) =
    RuntimePaths::resolve_with_profile(args.runtime_dir.clone(), args.runtime_profile)
  else {
    return;
  };

  match CadderSession::connect(&paths).await {
    Ok(_) => {}
    Err(error) => eprintln!(
      "{}",
      read_only_real_caddy_inspection_message(&paths, command, &error)
    ),
  }
}

fn read_only_real_caddy_inspection_message(
  paths: &RuntimePaths,
  command: ClassifiedShimCommand<'_>,
  error: &IpcClientError,
) -> String {
  let runtime_dir = paths.runtime_dir().display();
  let unavailable = daemon_error_indicates_not_running(error);
  let outcome = if unavailable {
    format!("Cadder backend `cadderd` is not running for runtime `{runtime_dir}`.")
  } else {
    format!(
      "Cadder runtime inspection is unavailable for runtime `{runtime_dir}`: {}",
      terminal_sentence(error.message())
    )
  };
  let recovery = if unavailable {
    format!(
      "Run `cadder daemon start --runtime-dir \"{}\"`, then retry Cadder runtime inspection.",
      paths.runtime_dir().display()
    )
  } else {
    error.guidance().map(ToOwned::to_owned).unwrap_or_else(|| {
      "Inspect the Cadder daemon diagnostics, correct the reported error, then retry runtime inspection."
        .to_string()
    })
  };
  let details = format_ipc_diagnostic_details(error);

  format!(
    "{outcome}\n\
     Running read-only `caddy {}` against the safely resolved real Caddy binary.\n\
     Output is real-Caddy inspection, not Cadder runtime state.\n\
     Next: {recovery}{details}",
    command.command,
  )
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
  let (heartbeat_stop_tx, heartbeat_stop_rx) = oneshot::channel();
  let heartbeat = tokio::spawn(run_heartbeat_loop(heartbeat_stop_rx, move || {
    let heartbeat_session = heartbeat_session.clone();
    let heartbeat_registration = heartbeat_registration.clone();
    let heartbeat_nonce = heartbeat_nonce.clone();
    async move {
      let _response: IpcClientResult<BasicResponse> = heartbeat_session
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
  }));

  let shutdown_result = shutdown
    .await
    .context("wait for shutdown signal while registered with Cadder");
  stop_heartbeat(heartbeat_stop_tx, heartbeat).await?;
  shutdown_result?;

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

async fn run_heartbeat_loop<F, Fut>(mut stop: oneshot::Receiver<()>, mut send_heartbeat: F)
where
  F: FnMut() -> Fut,
  Fut: Future<Output = ()>,
{
  let mut heartbeat_interval = interval(Duration::from_secs(5));
  loop {
    tokio::select! {
      biased;
      _ = &mut stop => break,
      _ = heartbeat_interval.tick() => send_heartbeat().await,
    }
  }
}

async fn stop_heartbeat(
  stop: oneshot::Sender<()>,
  heartbeat: tokio::task::JoinHandle<()>,
) -> Result<()> {
  let _ = stop.send(());
  heartbeat
    .await
    .context("wait for the heartbeat loop to stop before unregistering")
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
  Fut: Future<Output = IpcClientResult<()>>,
{
  match CadderSession::connect(paths).await {
    Ok(session) => Ok(ManagedRunTarget::Cadder(Arc::new(Mutex::new(session)))),
    Err(error) if !daemon_error_indicates_not_running(&error) => {
      eprintln!("{}", managed_backend_unavailable_message(paths, &error));
      Ok(ManagedRunTarget::Exit(ExitCode::FAILURE))
    }
    Err(error) => match start_daemon(args.clone(), paths.clone()).await {
      Ok(()) => match CadderSession::connect(paths).await {
        Ok(session) => Ok(ManagedRunTarget::Cadder(Arc::new(Mutex::new(session)))),
        Err(recovery_error) => {
          eprintln!(
            "{}",
            managed_recovery_failed_message(
              paths,
              &error,
              ManagedRecoveryStage::PostStartAttach,
              &recovery_error,
            )
          );
          Ok(ManagedRunTarget::Exit(ExitCode::FAILURE))
        }
      },
      Err(recovery_error) => {
        eprintln!(
          "{}",
          managed_recovery_failed_message(
            paths,
            &error,
            ManagedRecoveryStage::DaemonStart,
            &recovery_error,
          )
        );
        Ok(ManagedRunTarget::Exit(ExitCode::FAILURE))
      }
    },
  }
}

async fn start_missing_daemon(args: &ShimArgs, paths: &RuntimePaths) -> IpcClientResult<()> {
  ensure_daemon_running_with_options(
    paths,
    DaemonLaunchOptions {
      explicit_daemon: args.daemon_path.clone(),
      runtime_profile: args.runtime_profile,
      real_caddy_override: None,
      caddy_backend: args.caddy_backend,
      ..DaemonLaunchOptions::default()
    },
  )
  .await
}

async fn start_missing_daemon_owned(args: ShimArgs, paths: RuntimePaths) -> IpcClientResult<()> {
  start_missing_daemon(&args, &paths).await
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedRecoveryStage {
  DaemonStart,
  PostStartAttach,
}

impl ManagedRecoveryStage {
  fn label(self) -> &'static str {
    match self {
      Self::DaemonStart => "Daemon start failed",
      Self::PostStartAttach => "Post-start attach failed",
    }
  }
}

fn managed_recovery_failed_message(
  paths: &RuntimePaths,
  attach_error: &IpcClientError,
  recovery_stage: ManagedRecoveryStage,
  recovery_error: &IpcClientError,
) -> String {
  let recovery = if recovery_stage == ManagedRecoveryStage::PostStartAttach
    && recovery_error.is_daemon_unavailable()
  {
    format!(
      "Run `cadderd --runtime-dir \"{}\"` in foreground diagnostic mode, correct the startup error, then retry.",
      paths.runtime_dir().display()
    )
  } else {
    recovery_error
      .guidance()
      .map(ToOwned::to_owned)
      .unwrap_or_else(|| {
        "Inspect the Cadder daemon diagnostics, correct the reported error, then retry.".to_string()
      })
  };
  format!(
    "Cadder could not recover `caddy run` for backend runtime `{}`.\n\
     Managed `caddy run` was not delegated to real Caddy because Cadder must update runtime state through `cadderd`.\n\
     Next: {recovery}\n\
     Details:\n\
     - Initial attach failed: {}\n\
     - {}: {}",
    paths.runtime_dir().display(),
    format_ipc_error_chain(attach_error),
    recovery_stage.label(),
    format_ipc_error_chain(recovery_error),
  )
}

fn managed_backend_unavailable_message(paths: &RuntimePaths, error: &IpcClientError) -> String {
  let runtime_dir = paths.runtime_dir().display();
  let details = format_ipc_diagnostic_details(error);

  if daemon_error_indicates_not_running(error) {
    format!(
      "Cadder backend `cadderd` is not running for runtime `{runtime_dir}`.\n\
       Managed `caddy run` was not delegated to real Caddy.\n\
       Next: Run `cadder daemon start --runtime-dir \"{runtime_dir}\"`, then retry `caddy run`.{details}"
    )
  } else {
    let guidance = error.guidance().unwrap_or(
      "Inspect the Cadder daemon diagnostics, correct the reported error, then retry `caddy run`.",
    );
    format!(
      "Cadder could not attach `caddy run` to backend runtime `{runtime_dir}`: {}\n\
       Managed `caddy run` was not delegated to real Caddy.\n\
       Next: {guidance}{details}",
      terminal_sentence(error.message()),
    )
  }
}

fn daemon_error_indicates_not_running(error: &IpcClientError) -> bool {
  error.is_daemon_unavailable()
}

fn format_ipc_error_chain(error: &IpcClientError) -> String {
  let mut messages = vec![error.to_string()];
  let mut source = std::error::Error::source(error);
  while let Some(error) = source {
    let message = error.to_string();
    if messages.last() != Some(&message) {
      messages.push(message);
    }
    source = error.source();
  }
  messages.join(": ")
}

fn format_ipc_diagnostic_details(error: &IpcClientError) -> String {
  let details = format_ipc_error_chain(error);
  if details == error.message() {
    String::new()
  } else {
    format!("\nDetails: {details}")
  }
}

fn terminal_sentence(message: &str) -> String {
  let message = message.trim();
  if message.ends_with(['.', '!', '?']) {
    message.to_string()
  } else {
    format!("{message}.")
  }
}

#[cfg(test)]
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

async fn delegate_to_real_caddy(args: &ShimArgs) -> Result<ExitCode> {
  match run_real_caddy_fallback(args).await {
    Ok(code) => Ok(code),
    Err(error) => {
      eprintln!("{}", RealCaddyResolver::resolution_help(&error));
      Ok(ExitCode::FAILURE)
    }
  }
}

async fn run_real_caddy_fallback(args: &ShimArgs) -> Result<ExitCode> {
  let profile = real_caddy_profile(args)?;
  let resolver = RealCaddyResolver::from_trusted_sources(profile);
  let binary = resolver.resolve()?;
  let status = Command::new(binary)
    .args(&args.caddy_args)
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit())
    .status()
    .await
    .context("delegate command to real Caddy")?;
  Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
}

fn real_caddy_profile(args: &ShimArgs) -> Result<RuntimeProfile> {
  Ok(
    RuntimePaths::resolve_with_profile(args.runtime_dir.clone(), args.runtime_profile)?
      .runtime_profile(),
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_daemon::{
    CaddyConfigAdapter, CaddyConfigCoordinator, DaemonServer, DaemonState, ProcessRuntime,
  };
  use cadder_protocol::{ProtocolError, ProtocolErrorCode, ProtocolErrorKind};
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
  async fn typed_error_managed_backend_unavailable_message_explains_manual_start() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let error = CadderSession::connect(&paths).await.unwrap_err();

    let message = managed_backend_unavailable_message(&paths, &error);

    assert!(message.contains("Cadder backend `cadderd` is not running"));
    assert!(message.contains("Next: Run `cadder daemon start --runtime-dir"));
    assert!(message.contains("retry `caddy run`"));
    assert!(
      message
        .find("Next:")
        .expect("message should include recovery")
        < message
          .find("Details:")
          .expect("message should place diagnostics after recovery")
    );
  }

  #[tokio::test]
  async fn typed_error_read_only_inspection_uses_matching_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let args = ["version".to_string()];
    let command = classify_caddy_command(&args);
    let unavailable = CadderSession::connect(&paths).await.unwrap_err();

    let message = read_only_real_caddy_inspection_message(&paths, command, &unavailable);

    assert!(message.contains("cadderd` is not running"));
    assert!(message.contains("read-only `caddy version`"));
    assert!(message.contains("real-Caddy inspection, not Cadder runtime state"));
    assert!(message.contains("cadder daemon start"));
    assert!(!message.lines().next().unwrap().contains("os error"));
    assert!(
      message
        .find("Next:")
        .expect("message should include recovery")
        < message
          .find("Details:")
          .expect("message should place diagnostics after recovery")
    );

    let permission = IpcClientError::Daemon(ProtocolError::access_denied(
      message_types::QUERY_STATE_REQUEST,
      "Access is denied.",
      Some("Use the account that owns this runtime.".to_string()),
    ));
    let permission_message = read_only_real_caddy_inspection_message(&paths, command, &permission);
    assert!(permission_message.contains("Next: Use the account that owns this runtime."));
    assert!(!permission_message.contains("cadder daemon start"));
    assert!(
      !permission_message
        .lines()
        .next()
        .unwrap()
        .contains("os error")
    );
  }

  #[test]
  fn typed_error_backend_helpers_classify_transport_and_protocol_failures() {
    let paths =
      RuntimePaths::resolve(Some(std::env::temp_dir().join("cadder-shim-protocol-test"))).unwrap();
    let protocol_error = ipc_error(
      ProtocolErrorKind::IncompatibleProtocolVersion,
      "incompatible_protocol",
      "protocol mismatch",
    );
    let message = managed_backend_unavailable_message(&paths, &protocol_error);

    assert!(message.contains("could not attach `caddy run`"));
    assert!(message.contains("protocol mismatch"));
    assert!(message.contains("\nNext: Inspect the Cadder daemon diagnostics"));
    assert!(!message.contains("cadder daemon start"));

    let permission = IpcClientError::Daemon(ProtocolError::access_denied(
      message_types::QUERY_STATE_REQUEST,
      "Access is denied.",
      Some("Use the account that owns this runtime.".to_string()),
    ));
    let permission_message = managed_backend_unavailable_message(&paths, &permission);
    assert!(permission_message.contains("Next: Use the account that owns this runtime."));
    assert!(!permission_message.contains("cadder daemon start"));
    let unavailable = ipc_error(
      ProtocolErrorKind::Internal,
      "daemon_unavailable",
      "backend unavailable",
    );
    assert!(!daemon_error_indicates_not_running(&unavailable));

    let duplicated = Err::<(), _>(anyhow!("same")).context("same").unwrap_err();
    assert_eq!(format_error_chain(&duplicated), "same");
    let nested = Err::<(), _>(anyhow!("inner")).context("outer").unwrap_err();
    assert_eq!(format_error_chain(&nested), "outer: inner");
  }

  #[tokio::test]
  async fn typed_error_recovery_message_uses_the_recovery_failure_guidance() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let attach_error = CadderSession::connect(&paths).await.unwrap_err();
    let start_error = IpcClientError::Daemon(ProtocolError::new(
      ProtocolErrorKind::InvalidInput,
      ProtocolErrorCode::parse("invalid_input").unwrap(),
      "The daemon launch configuration is invalid.",
      Some("Correct the daemon launch configuration, then retry.".into()),
      false,
    ));

    let message = managed_recovery_failed_message(
      &paths,
      &attach_error,
      ManagedRecoveryStage::DaemonStart,
      &start_error,
    );

    assert!(message.contains("Next: Correct the daemon launch configuration, then retry."));
    assert!(!message.contains("cadder daemon start"));

    let post_start_message = managed_recovery_failed_message(
      &paths,
      &attach_error,
      ManagedRecoveryStage::PostStartAttach,
      &attach_error,
    );
    assert!(post_start_message.contains("foreground diagnostic mode"));
  }

  fn ipc_error(kind: ProtocolErrorKind, code: &str, message: &str) -> IpcClientError {
    IpcClientError::Daemon(ProtocolError::new(
      kind,
      ProtocolErrorCode::parse(code).unwrap(),
      message,
      None,
      false,
    ))
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
      rejected_real_caddy_selector: None,
      caddy_backend: Some(CaddyBackendMode::Mock),
      caddy_args: vec!["run".to_string()],
    })
    .await
    .unwrap();

    assert_eq!(code, ExitCode::FAILURE);
  }

  #[test]
  fn real_caddy_delegation_uses_the_requested_runtime_profile() {
    let temp = tempfile::tempdir().unwrap();
    let args = ShimArgs {
      runtime_dir: Some(temp.path().join("runtime")),
      runtime_profile: Some(RuntimeProfile::Dev),
      daemon_path: None,
      rejected_real_caddy_selector: None,
      caddy_backend: None,
      caddy_args: vec!["version".to_string()],
    };

    assert_eq!(real_caddy_profile(&args).unwrap(), RuntimeProfile::Dev);
  }

  #[tokio::test]
  async fn run_managed_does_not_delegate_to_real_caddy_when_backend_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let fake_caddy = temp.path().join(fake_caddy_name_for_test());
    write_fake_caddy(&fake_caddy);

    let code = run_managed(ShimArgs {
      runtime_dir: Some(paths.runtime_dir().to_path_buf()),
      runtime_profile: None,
      daemon_path: Some(temp.path().join(fake_daemon_name_for_test())),
      rejected_real_caddy_selector: Some(fake_caddy.display().to_string()),
      caddy_backend: None,
      caddy_args: vec!["run".to_string()],
    })
    .await
    .unwrap();

    assert_eq!(code, ExitCode::FAILURE);
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
      rejected_real_caddy_selector: None,
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
  async fn typed_error_managed_run_reports_complete_recovery_failure() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let args = ShimArgs {
      runtime_dir: Some(paths.runtime_dir().to_path_buf()),
      runtime_profile: None,
      daemon_path: Some(temp.path().join(fake_daemon_name_for_test())),
      rejected_real_caddy_selector: Some("definitely-missing-caddy-binary".to_string()),
      caddy_backend: None,
      caddy_args: vec!["run".to_string()],
    };
    let starter = |_args: ShimArgs, _paths: RuntimePaths| async {
      Err(ipc_error(
        ProtocolErrorKind::Internal,
        "daemon_start_failed",
        "Test daemon start failed.",
      ))
    };

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
  async fn heartbeat_stop_waits_for_in_flight_request_before_unregister() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (heartbeat_started_tx, heartbeat_started_rx) = oneshot::channel();
    let (heartbeat_release_tx, heartbeat_release_rx) = oneshot::channel();
    let mut heartbeat_started_tx = Some(heartbeat_started_tx);
    let mut heartbeat_release_rx = Some(heartbeat_release_rx);
    let heartbeat_events = events.clone();
    let (heartbeat_stop_tx, heartbeat_stop_rx) = oneshot::channel();
    let heartbeat = tokio::spawn(run_heartbeat_loop(heartbeat_stop_rx, move || {
      let started = heartbeat_started_tx
        .take()
        .expect("the test heartbeat should start once");
      let release = heartbeat_release_rx
        .take()
        .expect("the test heartbeat should finish once");
      let events = heartbeat_events.clone();
      async move {
        events.lock().await.push("heartbeat-started");
        started.send(()).unwrap();
        release.await.unwrap();
        events.lock().await.push("heartbeat-finished");
      }
    }));

    heartbeat_started_rx.await.unwrap();
    let unregister_events = events.clone();
    let shutdown = tokio::spawn(async move {
      stop_heartbeat(heartbeat_stop_tx, heartbeat).await.unwrap();
      unregister_events.lock().await.push("unregister");
    });
    tokio::task::yield_now().await;

    assert!(!shutdown.is_finished());
    assert_eq!(*events.lock().await, ["heartbeat-started"]);

    heartbeat_release_tx.send(()).unwrap();
    shutdown.await.unwrap();

    assert_eq!(
      *events.lock().await,
      ["heartbeat-started", "heartbeat-finished", "unregister"]
    );
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
    let resolver = RealCaddyResolver::for_test_fixture(fake_caddy);
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
        rejected_real_caddy_selector: None,
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
