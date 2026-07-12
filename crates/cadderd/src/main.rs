use anyhow::{Context, Result};
use cadder_daemon::{
  CaddyBackendMode, DaemonLaunchMode, DaemonLaunchOptions, DaemonOptions, RuntimePaths,
  RuntimeProfile, ensure_daemon_running_with_options, run_daemon,
};
use clap::Parser;
use std::{env, path::PathBuf};
use tokio::sync::watch;

#[derive(Debug, Parser)]
#[command(
  name = "cadderd",
  version,
  about = "Cadder per-user Caddy coordinator daemon",
  long_about = "Runs the per-user Cadder daemon that owns local IPC, project registrations, generated Caddy config, the Cadder-owned real Caddy process, diagnostics, and bounded logs."
)]
struct Args {
  #[arg(
    long,
    help = "Override the Cadder runtime directory for IPC, lock, config, metadata, and logs"
  )]
  runtime_dir: Option<PathBuf>,

  #[arg(
    long,
    value_parser = RuntimeProfile::parse_cli,
    help = "Runtime profile used when --runtime-dir and CADDER_RUNTIME_DIR are not set"
  )]
  runtime_profile: Option<RuntimeProfile>,

  #[arg(
    long,
    help = "Command or path used when Cadder starts the real Caddy binary"
  )]
  real_caddy_command: Option<String>,

  #[arg(
    long,
    value_parser = CaddyBackendMode::parse_cli,
    help = "Caddy backend mode used by the daemon: real or mock"
  )]
  caddy_backend: Option<CaddyBackendMode>,

  #[arg(
    long,
    help = "Start cadderd detached in the background and exit after the daemon socket is ready"
  )]
  background: bool,

  #[arg(long, hide = true)]
  detach_ready: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
  tracing_subscriber::fmt::init();
  let args = Args::parse();
  if args.background {
    return launch_background_daemon(args).await;
  }

  let (shutdown_tx, shutdown_rx) = watch::channel(false);

  tokio::spawn(async move {
    let _ = tokio::signal::ctrl_c().await;
    let _ = shutdown_tx.send(true);
  });

  run_daemon(
    DaemonOptions {
      runtime_dir: args.runtime_dir,
      runtime_profile: args.runtime_profile,
      real_caddy_command: args.real_caddy_command,
      caddy_backend: args.caddy_backend,
    },
    shutdown_rx,
  )
  .await
}

async fn launch_background_daemon(args: Args) -> Result<()> {
  let paths = RuntimePaths::resolve_with_profile(args.runtime_dir.clone(), args.runtime_profile)?;
  let current_exe = env::current_exe().context("resolve current cadderd executable")?;

  ensure_daemon_running_with_options(
    &paths,
    DaemonLaunchOptions {
      explicit_daemon: Some(current_exe),
      runtime_profile: args.runtime_profile,
      real_caddy_command: args.real_caddy_command,
      caddy_backend: args.caddy_backend,
      shim_path: None,
      launch_mode: DaemonLaunchMode::Background,
    },
  )
  .await?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use clap::CommandFactory;

  #[test]
  fn command_metadata_matches_release_identity() {
    let command = Args::command();

    assert_eq!(command.get_name(), "cadderd");
    assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(
      command.get_about().map(ToString::to_string),
      Some(env!("CARGO_PKG_DESCRIPTION").to_string())
    );
  }

  #[test]
  fn short_help_uses_package_description() {
    let help = Args::command().render_help().to_string();

    assert!(
      help.contains(env!("CARGO_PKG_DESCRIPTION")),
      "short help output should include the package description: {help}"
    );
  }

  #[test]
  fn long_help_describes_daemon_options() {
    let help = Args::command().render_long_help().to_string();

    assert!(
      help.contains("Override the Cadder runtime directory"),
      "long help output should describe --runtime-dir: {help}"
    );
    assert!(
      help.contains("Command or path used when Cadder starts the real Caddy binary"),
      "long help output should describe --real-caddy-command: {help}"
    );
    assert!(
      help.contains("Runtime profile used when --runtime-dir"),
      "long help output should describe --runtime-profile: {help}"
    );
    assert!(
      help.contains("Caddy backend mode used by the daemon"),
      "long help output should describe --caddy-backend: {help}"
    );
    assert!(
      help.contains("Start cadderd detached in the background"),
      "long help output should describe --background: {help}"
    );
  }

  #[test]
  fn background_flag_is_explicit_detached_launcher() {
    let args = Args::parse_from([
      "cadderd",
      "--background",
      "--runtime-dir",
      "C:/temp/cadder-runtime",
      "--caddy-backend",
      "mock",
    ]);

    assert!(args.background);
    assert!(!args.detach_ready);
    assert_eq!(args.caddy_backend, Some(CaddyBackendMode::Mock));
  }
}
