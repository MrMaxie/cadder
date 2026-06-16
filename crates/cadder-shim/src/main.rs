use anyhow::{Context, Result, anyhow};
use cadder_daemon::{CadderSession, RealCaddyResolver, RuntimePaths};
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
  path::PathBuf,
  process::{ExitCode, Stdio},
  time::Duration,
};
use tokio::{process::Command, sync::Mutex, time::interval};

#[derive(Debug, Parser)]
#[command(
  name = "caddy",
  version,
  about = "Cadder PATH-facing Caddy shim",
  long_about = "Acts as the Cadder-managed caddy command. `caddy run` requires a running cadderd backend and registers the current project; other commands are delegated to the safely resolved real Caddy binary."
)]
struct ShimArgs {
  #[arg(long = "cadder-runtime-dir", hide = true)]
  runtime_dir: Option<PathBuf>,

  #[arg(long = "cadder-daemon-path", hide = true)]
  daemon_path: Option<PathBuf>,

  #[arg(long = "cadder-real-caddy-command", hide = true)]
  real_caddy_command: Option<String>,

  #[arg(
    value_name = "CADDY_ARGS",
    trailing_var_arg = true,
    allow_hyphen_values = true,
    help = "Arguments for the caddy command; `run` is managed by Cadder and other commands are delegated to real Caddy"
  )]
  caddy_args: Vec<String>,
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

  if args.caddy_args.first().is_some_and(|arg| arg == "run") {
    run_managed(args).await
  } else {
    delegate_to_real_caddy(args.real_caddy_command, &args.caddy_args).await
  }
}

async fn run_managed(args: ShimArgs) -> Result<ExitCode> {
  let paths = RuntimePaths::resolve(args.runtime_dir)?;
  let session = match CadderSession::connect(&paths).await {
    Ok(session) => std::sync::Arc::new(Mutex::new(session)),
    Err(error) => {
      eprintln!("{}", managed_backend_unavailable_message(&paths, &error));
      return Ok(ExitCode::FAILURE);
    }
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

  tokio::signal::ctrl_c()
    .await
    .context("wait for Ctrl+C while registered with Cadder")?;
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

fn managed_backend_unavailable_message(paths: &RuntimePaths, error: &anyhow::Error) -> String {
  let runtime_dir = paths.runtime_dir().display();
  let retry = format!(
    "Start `cadderd --runtime-dir \"{}\"` explicitly, or open `cadder-tui` and press s, then retry `caddy run`.",
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
  let resolver = RealCaddyResolver::new(real_caddy_command);
  let binary = match resolver.resolve() {
    Ok(binary) => binary,
    Err(error) => {
      eprintln!("{}", RealCaddyResolver::resolution_help(&error));
      return Ok(ExitCode::FAILURE);
    }
  };
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
  use clap::CommandFactory;

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
      help.contains("delegated to real Caddy"),
      "long help output should describe delegation behavior: {help}"
    );
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
    assert!(message.contains("Start `cadderd --runtime-dir"));
    assert!(message.contains("retry `caddy run`"));
  }

  #[tokio::test]
  async fn run_managed_returns_failure_when_backend_is_unavailable() {
    let runtime_dir = std::env::temp_dir().join(format!(
      "cadder-shim-missing-backend-{}",
      std::process::id()
    ));
    let code = run_managed(ShimArgs {
      runtime_dir: Some(runtime_dir),
      daemon_path: None,
      real_caddy_command: None,
      caddy_args: vec!["run".to_string()],
    })
    .await
    .unwrap();

    assert_eq!(code, ExitCode::FAILURE);
  }
}
