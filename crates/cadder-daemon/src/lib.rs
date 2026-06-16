mod caddy;
mod config;
mod iis;
mod ipc;
mod logs;
mod paths;
mod runtime;
mod state;

pub use caddy::{CaddyConfigAdapter, CaddyConfigCoordinator, RealCaddyResolver};
pub use config::{CONFIG_FILE_NAME, CadderConfig, CaddyRuntimeConfig};
pub use iis::{IisBindingRecord, IisMetadataStore, IisProvider};
pub use ipc::{
  CadderClient, CadderSession, DaemonLaunchOptions, DaemonServer, StateSubscription,
  ensure_daemon_running, ensure_daemon_running_with_options,
};
pub use logs::{CaddyLogStore, Redactor};
pub use paths::{DaemonLock, RuntimePaths};
pub use runtime::ProcessRuntime;
pub use state::DaemonState;

use anyhow::Result;
use std::path::PathBuf;
use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct DaemonOptions {
  pub runtime_dir: Option<PathBuf>,
  pub real_caddy_command: Option<String>,
}

pub async fn run_daemon(options: DaemonOptions, shutdown: watch::Receiver<bool>) -> Result<()> {
  let paths = RuntimePaths::resolve(options.runtime_dir)?;
  paths.ensure_dirs()?;
  let _lock = DaemonLock::acquire(paths.lock_path())?;

  let real_caddy = RealCaddyResolver::new(options.real_caddy_command);
  let adapter = CaddyConfigAdapter::new(real_caddy.clone());
  let runtime = ProcessRuntime::new(real_caddy, paths.clone());
  let coordinator = CaddyConfigCoordinator::new(adapter, runtime);
  let state = DaemonState::with_runtime_paths(coordinator, paths.clone()).await?;

  let server = DaemonServer::new(paths, state);
  server.run_until(shutdown).await
}
