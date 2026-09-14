mod caddy;
mod caddy_image;
mod caddy_path_trust;
mod config;
mod database;
mod ipc;
mod ipc_client_error;
mod ipc_codec;
mod ipc_security;
#[cfg(unix)]
mod ipc_unix_security;
#[cfg(windows)]
mod ipc_windows_security;
mod logs;
mod operation_fence;
mod paths;
mod privilege;
mod process_tree;
mod runtime;
mod runtime_file;
mod state;

pub use caddy::{
  CaddyBackendMode, CaddyConfigAdapter, CaddyConfigCoordinator, CaddyRegistrationAdapter,
  RealCaddyResolver,
};
pub use config::{CONFIG_FILE_NAME, CadderConfig};
pub use ipc::{
  CadderClient, CadderSession, DaemonLaunchMode, DaemonLaunchOptions, DaemonServer,
  ensure_daemon_running, ensure_daemon_running_with_options,
};
pub use ipc_client_error::{
  IpcClientError, IpcClientPhase, IpcClientResult, LocalIpcError, LocalIpcErrorCode,
  LocalIpcErrorKind,
};
pub use ipc_security::{
  IpcAccessDecision, IpcOperation, IpcOperationKind, IpcPrincipal, IpcSecurityPolicy,
};
pub use logs::{CaddyLogStore, Redactor};
pub use paths::{RuntimePaths, StoragePaths};
pub use privilege::{
  PrivilegeDiagnostic, PrivilegeStatus, current_privilege_status,
  elevated_management_surface_diagnostic, management_surface_privilege_diagnostic,
  shim_privilege_diagnostic,
};
pub use runtime::{CaddyRuntime, MockCaddyRuntime, ProcessRuntime, RuntimeTimeouts};
pub use state::DaemonState;

use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use tokio::sync::watch;
use tokio::time::{Duration, sleep};

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone)]
pub struct DaemonOptions {
  /// Test-only/internal runtime selection seam. Production entrypoints always use `None`.
  pub runtime_dir: Option<PathBuf>,
  pub real_caddy_override: Option<PathBuf>,
  pub caddy_backend: Option<CaddyBackendMode>,
}

const DAEMON_RUNTIME_RELEASE_ATTEMPTS: usize = 50;
const DAEMON_RUNTIME_RELEASE_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub async fn run_daemon(options: DaemonOptions, shutdown: watch::Receiver<bool>) -> Result<()> {
  let paths = RuntimePaths::resolve(options.runtime_dir)?;
  let Some(lease) = ipc::RuntimeEndpointLease::claim(&paths).await? else {
    wait_for_daemon_runtime_ready(&paths).await?;
    return Ok(());
  };
  cleanup_legacy_runtime_artifacts(&paths)?;
  let caddy_backend = options
    .caddy_backend
    .map_or_else(CaddyBackendMode::from_env, Ok)?;
  if caddy_backend == CaddyBackendMode::Mock && options.real_caddy_override.is_some() {
    bail!("--real-caddy cannot be combined with --caddy-backend mock");
  }
  let real_caddy = match caddy_backend {
    CaddyBackendMode::Real => {
      #[cfg(debug_assertions)]
      let resolver = if std::env::var_os("CADDER_TEST_ALLOW_UNTRUSTED_CADDY").as_deref()
        == Some(std::ffi::OsStr::new("1"))
      {
        RealCaddyResolver::for_test_fixture(
          options
            .real_caddy_override
            .context("test Caddy trust bypass requires --real-caddy")?,
        )
      } else {
        RealCaddyResolver::for_daemon(options.real_caddy_override)
      };
      #[cfg(not(debug_assertions))]
      let resolver = RealCaddyResolver::for_daemon(options.real_caddy_override);
      resolver.pin().await?;
      Some(resolver)
    }
    CaddyBackendMode::Mock => None,
  };
  let coordinator = match caddy_backend {
    CaddyBackendMode::Real => {
      let real_caddy = real_caddy.expect("real backend pins its Caddy resolver");
      let adapter = CaddyConfigAdapter::new(real_caddy.clone());
      let runtime = ProcessRuntime::new(real_caddy, paths.clone());
      CaddyConfigCoordinator::new(adapter, runtime)
    }
    CaddyBackendMode::Mock => CaddyConfigCoordinator::new_mock(paths.clone()),
  };
  let state = DaemonState::with_runtime_paths(coordinator, paths.clone()).await?;
  DaemonServer::new(paths, state)
    .with_lease(lease)
    .run_until(shutdown)
    .await
}

async fn wait_for_daemon_runtime_ready(paths: &RuntimePaths) -> Result<()> {
  for _ in 0..DAEMON_RUNTIME_RELEASE_ATTEMPTS {
    if ipc::is_daemon_ready(paths).await? {
      return Ok(());
    }
    sleep(DAEMON_RUNTIME_RELEASE_POLL_INTERVAL).await;
  }
  bail!(
    "a Cadder process owns the local endpoint but did not become ready for runtime {}",
    paths.runtime_dir().display(),
  )
}

fn cleanup_legacy_runtime_artifacts(paths: &RuntimePaths) -> Result<()> {
  for name in [
    "cadder.lock",
    "cadder.lock.json",
    "cadder-containment.lock",
    "cadder-containment.json",
    "cadder-launch.lock",
    "cadder-ipc.lock",
    "cadder-ipc.json",
  ] {
    let path = paths.runtime_dir().join(name);
    match std::fs::remove_file(&path) {
      Ok(()) => {}
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
      Err(error) => {
        return Err(error)
          .with_context(|| format!("remove legacy runtime artifact {}", path.display()));
      }
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests;
