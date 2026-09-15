use anyhow::{Context, Result};
use cadder_daemon::{
  CaddyBackendMode, DaemonLaunchMode, DaemonLaunchOptions, DaemonOptions, RuntimePaths,
  ensure_daemon_running_with_options, run_daemon,
};
use clap::Parser;
use std::{env, io, path::PathBuf};
use tokio::sync::watch;

#[derive(Debug, Parser)]
#[command(
  name = "cadderd",
  version,
  about = env!("CARGO_PKG_DESCRIPTION"),
  long_about = "Runs the portable Cadder daemon that owns local IPC, project registrations, generated Caddy config, the Cadder-owned real Caddy process, diagnostics, and bounded logs."
)]
struct Args {
  #[arg(
    long = "real-caddy",
    help = "Absolute path used when Cadder starts the real Caddy executable"
  )]
  real_caddy_override: Option<PathBuf>,

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
    request_shutdown_after_ctrl_c(tokio::signal::ctrl_c().await, &shutdown_tx);
  });

  run_daemon(
    DaemonOptions {
      runtime_dir: None,
      real_caddy_override: args.real_caddy_override,
      caddy_backend: args.caddy_backend,
    },
    shutdown_rx,
  )
  .await
}

fn request_shutdown_after_ctrl_c(ctrl_c_result: io::Result<()>, shutdown_tx: &watch::Sender<bool>) {
  if ctrl_c_result.is_ok() {
    let _ = shutdown_tx.send(true);
  }
}

async fn launch_background_daemon(args: Args) -> Result<()> {
  let paths = RuntimePaths::resolve(None)?;
  let current_daemon =
    env::current_exe().context("resolve the cadderd executable for the background daemon")?;

  ensure_daemon_running_with_options(
    &paths,
    DaemonLaunchOptions {
      explicit_daemon: Some(current_daemon),
      real_caddy_override: args.real_caddy_override,
      caddy_backend: args.caddy_backend,
      launch_mode: DaemonLaunchMode::Background,
    },
  )
  .await?;
  Ok(())
}

#[cfg(test)]
#[path = "cadderd/tests.rs"]
mod tests;
