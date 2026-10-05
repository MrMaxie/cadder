mod command_policy;
mod registration;

use anyhow::{Context, Result, anyhow};
use cadder_api::{
  CadderSession, CaddyBackendMode, DaemonLaunchOptions, IpcClientError, IpcClientResult,
  RealCaddyResolver, RuntimePaths, ensure_daemon_running_with_options, shim_privilege_diagnostic,
};
use cadder_ipc::{
  BasicResponse, HeartbeatEntrypointPayload, RegisterEntrypointPayload, RegisterEntrypointResponse,
  UnregisterEntrypointPayload, new_request_id,
};
use clap::Parser;
use command_policy::{ClassifiedShimCommand, ShimCommandPolicyKind, classify_caddy_command};
use registration::build_registration;
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

  #[cfg(test)]
  #[arg(skip)]
  test_runtime_dir: Option<PathBuf>,
}

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
  let Ok(paths) = runtime_paths_for_args(args) else {
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
    "Start `cadderd`, then retry Cadder runtime inspection.".to_string()
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
  let paths = runtime_paths_for_args(&args)?;
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
      new_request_id("shim-register"),
      &RegisterEntrypointPayload { registration },
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
          new_request_id("shim-heartbeat"),
          &HeartbeatEntrypointPayload {
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
      new_request_id("shim-unregister"),
      &UnregisterEntrypointPayload {
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
    "Run `cadderd` in foreground diagnostic mode, correct the startup error, then retry."
      .to_string()
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
       Next: Start `cadderd`, then retry `caddy run`.{details}"
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
  let resolver = RealCaddyResolver::from_trusted_sources();
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

fn runtime_paths_for_args(args: &ShimArgs) -> Result<RuntimePaths> {
  #[cfg(test)]
  if let Some(runtime_dir) = &args.test_runtime_dir {
    return RuntimePaths::resolve(Some(runtime_dir.clone()));
  }

  let _ = args;
  RuntimePaths::resolve(None)
}

#[cfg(test)]
mod tests;
