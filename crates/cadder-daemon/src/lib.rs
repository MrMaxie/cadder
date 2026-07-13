mod autostart;
mod caddy;
mod caddy_image;
mod caddy_path_trust;
mod config;
mod iis;
mod ipc;
mod ipc_client_error;
mod ipc_codec;
mod ipc_discovery;
mod ipc_security;
#[cfg(unix)]
mod ipc_unix_security;
#[cfg(windows)]
mod ipc_windows_security;
mod logs;
mod operation_fence;
mod operation_registry;
mod paths;
mod privilege;
mod process_tree;
mod runtime;
mod runtime_file;
mod runtime_lock;
mod state;
mod storage;

pub use caddy::{
  CaddyBackendMode, CaddyConfigAdapter, CaddyConfigCoordinator, CaddyRegistrationAdapter,
  RealCaddyResolver,
};
pub use config::{CONFIG_FILE_NAME, CadderConfig, RuntimeConfig};
pub use iis::{IisBindingRecord, IisMetadataStore, IisProvider};
pub use ipc::{
  CadderClient, CadderSession, DaemonLaunchMode, DaemonLaunchOptions, DaemonServer,
  StateSubscription, ensure_daemon_running, ensure_daemon_running_with_options,
};
pub use ipc_client_error::{
  IpcClientError, IpcClientPhase, IpcClientResult, LocalIpcError, LocalIpcErrorCode,
  LocalIpcErrorKind,
};
pub use ipc_discovery::{
  IpcEndpoint, IpcEndpointMetadata, IpcEndpointPublication, discover_ipc_endpoint,
};
pub use ipc_security::{
  IpcAccessDecision, IpcOperation, IpcOperationKind, IpcPrincipal, IpcSecurityPolicy,
};
pub use logs::{CaddyLogStore, Redactor};
pub use paths::{RuntimePaths, RuntimeProfile, StoragePaths};
pub use privilege::{
  PrivilegeDiagnostic, PrivilegeStatus, current_privilege_status,
  elevated_management_surface_diagnostic, management_surface_privilege_diagnostic,
  shim_privilege_diagnostic,
};
pub use runtime::{CaddyRuntime, MockCaddyRuntime, ProcessRuntime, RuntimeTimeouts};
pub use runtime_lock::DaemonLock;
pub use state::DaemonState;
pub use storage::RuntimeStore;

use anyhow::{Result, bail};
use cadder_protocol::{LogAttributionKind, LogSeverity, LogStreamIdentity};
use std::path::PathBuf;
use tokio::sync::watch;
use tokio::time::{Duration, sleep};

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone)]
pub struct DaemonOptions {
  pub runtime_dir: Option<PathBuf>,
  pub runtime_profile: Option<RuntimeProfile>,
  pub real_caddy_override: Option<PathBuf>,
  pub caddy_backend: Option<CaddyBackendMode>,
}

const DAEMON_RUNTIME_RELEASE_ATTEMPTS: usize = 50;
const DAEMON_RUNTIME_RELEASE_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub async fn run_daemon(options: DaemonOptions, shutdown: watch::Receiver<bool>) -> Result<()> {
  let paths = RuntimePaths::resolve_with_profile(options.runtime_dir, options.runtime_profile)?;
  paths.ensure_dirs()?;
  let Some(lock) = acquire_daemon_lock_or_wait_for_ready(&paths).await? else {
    return Ok(());
  };
  let lock_recovery = lock.recovery().cloned();

  let caddy_backend = options
    .caddy_backend
    .map_or_else(CaddyBackendMode::from_env, Ok)?;
  if caddy_backend == CaddyBackendMode::Mock && options.real_caddy_override.is_some() {
    bail!("--real-caddy cannot be combined with --caddy-backend mock");
  }
  let coordinator = match caddy_backend {
    CaddyBackendMode::Real => {
      let real_caddy =
        RealCaddyResolver::for_daemon(options.real_caddy_override, paths.runtime_profile());
      real_caddy.pin().await?;
      let adapter = CaddyConfigAdapter::new(real_caddy.clone());
      let runtime = ProcessRuntime::new(real_caddy, paths.clone());
      CaddyConfigCoordinator::new(adapter, runtime)
    }
    CaddyBackendMode::Mock => CaddyConfigCoordinator::new_mock(paths.clone()),
  };
  let state = DaemonState::with_runtime_paths(coordinator, paths.clone()).await?;
  if let Some(recovery) = lock_recovery {
    state.logs().append(
      LogStreamIdentity::runtime_control(),
      LogSeverity::Warn,
      recovery.log_message(),
      LogAttributionKind::RuntimeControl,
      Some("runtime-lock-recovery".to_string()),
    );
  }

  let server = DaemonServer::new(paths, state);
  let result = server.run_until(shutdown).await;
  drop(lock);
  result
}

pub async fn wait_for_daemon_runtime_released(paths: &RuntimePaths) -> Result<()> {
  wait_for_daemon_runtime_released_with_limits(
    paths,
    DAEMON_RUNTIME_RELEASE_ATTEMPTS,
    DAEMON_RUNTIME_RELEASE_POLL_INTERVAL,
  )
  .await
}

async fn wait_for_daemon_runtime_released_with_limits(
  paths: &RuntimePaths,
  attempts: usize,
  poll_interval: Duration,
) -> Result<()> {
  for _ in 0..attempts {
    if !ipc::is_daemon_ready(paths).await?
      && let Some(lock) = DaemonLock::try_acquire(paths.lock_path())?
    {
      drop(lock);
      return Ok(());
    }
    sleep(poll_interval).await;
  }

  bail!(
    "previous cadderd owner did not release socket and daemon lock before timeout for runtime {}; retry restart after it exits, or inspect `cadder daemon status --runtime-dir \"{}\"` before removing stale runtime files",
    paths.runtime_dir().display(),
    paths.runtime_dir().display()
  )
}

async fn acquire_daemon_lock_or_wait_for_ready(paths: &RuntimePaths) -> Result<Option<DaemonLock>> {
  acquire_daemon_lock_or_wait_for_ready_with_limits(
    paths,
    DAEMON_RUNTIME_RELEASE_ATTEMPTS,
    DAEMON_RUNTIME_RELEASE_POLL_INTERVAL,
  )
  .await
}

async fn acquire_daemon_lock_or_wait_for_ready_with_limits(
  paths: &RuntimePaths,
  attempts: usize,
  poll_interval: Duration,
) -> Result<Option<DaemonLock>> {
  for _ in 0..attempts {
    if ipc::is_daemon_ready(paths).await? {
      return Ok(None);
    }
    if let Some(lock) = DaemonLock::try_acquire_for_runtime(paths)? {
      return Ok(Some(lock));
    }
    sleep(poll_interval).await;
  }

  if ipc::is_daemon_ready(paths).await? {
    return Ok(None);
  }

  bail!(
    "previous cadderd owner did not release socket and daemon lock before timeout for runtime {}; {}",
    paths.runtime_dir().display(),
    DaemonLock::active_owner_diagnostic(paths)
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_protocol::{
    LogSeverity, LogStreamIdentity, QueryLogsRequest, QueryLogsResponse, QueryStateRequest,
    QueryStateResponse, message_types, new_request_id,
  };
  use std::fs;
  use tokio::time::{Duration, sleep, timeout};

  #[tokio::test]
  async fn run_daemon_starts_ipc_and_stops_on_shutdown_signal() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
    let client = CadderClient::new(paths);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon = tokio::spawn(run_daemon(
      DaemonOptions {
        runtime_dir: Some(runtime_dir),
        real_caddy_override: None,
        runtime_profile: None,
        caddy_backend: Some(CaddyBackendMode::Mock),
      },
      shutdown_rx,
    ));

    let response = wait_for_query_state(&client).await;
    shutdown_tx.send(true).unwrap();
    let daemon_result = timeout(Duration::from_secs(2), daemon)
      .await
      .unwrap()
      .unwrap();

    assert!(response.accepted);
    assert!(response.snapshot.is_some());
    daemon_result.unwrap();
  }

  #[tokio::test]
  async fn run_daemon_shutdown_storage_flushes_before_runtime_release() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
    let client = CadderClient::new(paths.clone());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon = tokio::spawn(run_daemon(
      DaemonOptions {
        runtime_dir: Some(runtime_dir),
        real_caddy_override: None,
        runtime_profile: None,
        caddy_backend: Some(CaddyBackendMode::Mock),
      },
      shutdown_rx,
    ));

    wait_for_query_state(&client).await;
    shutdown_tx.send(true).unwrap();
    timeout(Duration::from_secs(2), daemon)
      .await
      .unwrap()
      .unwrap()
      .unwrap();

    assert!(!paths.ipc_endpoint_path().exists());
    let store = RuntimeStore::open(paths.storage_paths());
    assert_eq!(store.state().backend, "files");
    let history = store
      .query_history(Some(cadder_protocol::HistoryKind::Runtime), 10)
      .await;
    assert!(
      history
        .iter()
        .any(|record| record.summary == "Daemon shutdown requested.")
    );
    store.contain_shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn run_daemon_shutdown_storage_retains_discovery_and_lock_until_flush_finishes() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let client = CadderClient::new(paths.clone());
    let (store, release_storage) = RuntimeStore::memory_stalled_for_test(1);
    let mut state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths.clone()));
    state.set_runtime_store_for_test(store);
    let server = DaemonServer::new(paths.clone(), state);
    let lock = DaemonLock::try_acquire_for_runtime(&paths)
      .unwrap()
      .unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon = tokio::spawn(async move {
      let _lock = lock;
      server.run_until(shutdown_rx).await
    });

    wait_for_query_state(&client).await;
    shutdown_tx.send(true).unwrap();
    sleep(Duration::from_millis(75)).await;

    assert!(paths.ipc_endpoint_path().exists());
    assert!(
      DaemonLock::try_acquire(paths.lock_path())
        .unwrap()
        .is_none()
    );

    release_storage.send(()).unwrap();
    timeout(Duration::from_secs(2), daemon)
      .await
      .unwrap()
      .unwrap()
      .unwrap();

    assert!(!paths.ipc_endpoint_path().exists());
    assert!(
      DaemonLock::try_acquire(paths.lock_path())
        .unwrap()
        .is_some()
    );
  }

  #[tokio::test]
  async fn run_daemon_returns_ok_when_runtime_already_has_healthy_socket() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
    let client = CadderClient::new(paths);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon = tokio::spawn(run_daemon(
      DaemonOptions {
        runtime_dir: Some(runtime_dir.clone()),
        real_caddy_override: None,
        runtime_profile: None,
        caddy_backend: Some(CaddyBackendMode::Mock),
      },
      shutdown_rx,
    ));
    let response = wait_for_query_state(&client).await;

    let (_second_shutdown_tx, second_shutdown_rx) = watch::channel(false);
    let second_start = timeout(
      Duration::from_secs(1),
      run_daemon(
        DaemonOptions {
          runtime_dir: Some(runtime_dir),
          real_caddy_override: None,
          runtime_profile: None,
          caddy_backend: Some(CaddyBackendMode::Mock),
        },
        second_shutdown_rx,
      ),
    )
    .await
    .unwrap();

    assert!(response.accepted);
    second_start.unwrap();
    shutdown_tx.send(true).unwrap();
    timeout(Duration::from_secs(2), daemon)
      .await
      .unwrap()
      .unwrap()
      .unwrap();
  }

  #[tokio::test]
  async fn run_daemon_logs_stale_lock_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
    paths.ensure_dirs().unwrap();
    fs::write(
      paths.lock_metadata_path(),
      r#"{
        "metadataVersion": 2,
        "cadderVersion": "0.7.0",
        "protocolVersion": 2,
        "minimumCompatibleProtocolVersion": 1,
        "capabilities": {
          "protocolVersion": 2,
          "minimumCompatibleProtocolVersion": 1,
          "supportedCapabilities": [],
          "supportedCapabilityVersions": []
        },
        "runtimeProfile": "default",
        "runtimeDir": "stale-runtime",
        "instanceKey": "stale-instance",
        "socketName": "stale.sock",
        "processId": 1,
        "ownerGeneration": "00112233445566778899aabbccddeeff",
        "acquiredAtUtc": "2026-01-01T00:00:00Z",
        "executablePath": "old-cadderd"
      }"#,
    )
    .unwrap();
    let client = CadderClient::new(paths);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon = tokio::spawn(run_daemon(
      DaemonOptions {
        runtime_dir: Some(runtime_dir),
        real_caddy_override: None,
        runtime_profile: None,
        caddy_backend: Some(CaddyBackendMode::Mock),
      },
      shutdown_rx,
    ));

    let response = wait_for_recovery_log(&client).await;
    shutdown_tx.send(true).unwrap();
    timeout(Duration::from_secs(2), daemon)
      .await
      .unwrap()
      .unwrap()
      .unwrap();

    assert!(response.entries.iter().any(|entry| {
      entry.operation.as_deref() == Some("runtime-lock-recovery")
        && entry.severity == LogSeverity::Warn
        && entry
          .raw_message
          .contains("Recovered stale daemon lock metadata")
    }));
  }

  #[tokio::test]
  async fn daemon_runtime_release_waits_for_previous_lock_owner() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir)).unwrap();
    paths.ensure_dirs().unwrap();
    let lock = DaemonLock::acquire(paths.lock_path()).unwrap();
    let release_path = paths.lock_path();
    let release_lock = tokio::spawn(async move {
      sleep(Duration::from_millis(40)).await;
      drop(lock);
      assert!(DaemonLock::try_acquire(release_path).unwrap().is_some());
    });

    wait_for_daemon_runtime_released(&paths).await.unwrap();
    release_lock.await.unwrap();
  }

  #[tokio::test]
  async fn daemon_runtime_release_reports_precise_timeout_for_locked_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir)).unwrap();
    paths.ensure_dirs().unwrap();
    let _lock = DaemonLock::acquire(paths.lock_path()).unwrap();

    let error = wait_for_daemon_runtime_released_with_limits(&paths, 2, Duration::from_millis(1))
      .await
      .unwrap_err();

    assert!(
      error
        .to_string()
        .contains("previous cadderd owner did not release socket and daemon lock"),
      "{error:?}"
    );
  }

  #[tokio::test]
  async fn daemon_lock_wait_can_take_over_after_failed_previous_start() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir)).unwrap();
    paths.ensure_dirs().unwrap();
    let lock = DaemonLock::acquire(paths.lock_path()).unwrap();
    let release_lock = tokio::spawn(async move {
      sleep(Duration::from_millis(40)).await;
      drop(lock);
    });

    let acquired =
      acquire_daemon_lock_or_wait_for_ready_with_limits(&paths, 20, Duration::from_millis(10))
        .await
        .unwrap();

    assert!(acquired.is_some());
    release_lock.await.unwrap();
  }

  #[tokio::test]
  async fn daemon_lock_wait_reports_locked_runtime_without_ready_socket() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir)).unwrap();
    paths.ensure_dirs().unwrap();
    let _lock = DaemonLock::acquire(paths.lock_path()).unwrap();
    let error =
      acquire_daemon_lock_or_wait_for_ready_with_limits(&paths, 2, Duration::from_millis(1))
        .await
        .unwrap_err();

    assert!(
      error
        .to_string()
        .contains("previous cadderd owner did not release socket and daemon lock"),
      "{error:?}"
    );
  }

  async fn wait_for_query_state(client: &CadderClient) -> QueryStateResponse {
    for _ in 0..50 {
      let result = client
        .request::<_>(
          message_types::QUERY_STATE_REQUEST,
          message_types::QUERY_STATE_RESPONSE,
          &QueryStateRequest {
            request_id: new_request_id("wait"),
          },
        )
        .await;
      if let Ok(response) = result {
        return response;
      }
      sleep(Duration::from_millis(20)).await;
    }

    panic!("daemon server did not become ready");
  }

  async fn wait_for_recovery_log(client: &CadderClient) -> QueryLogsResponse {
    for _ in 0..50 {
      let result = client
        .request::<_>(
          message_types::QUERY_LOGS_REQUEST,
          message_types::QUERY_LOGS_RESPONSE,
          &QueryLogsRequest {
            request_id: new_request_id("wait-lock-recovery"),
            stream: LogStreamIdentity::runtime_control(),
            limit: Some(20),
            cursor: None,
            minimum_severity: None,
          },
        )
        .await;
      if let Ok(response) = result
        && response
          .entries
          .iter()
          .any(|entry| entry.operation.as_deref() == Some("runtime-lock-recovery"))
      {
        return response;
      }
      sleep(Duration::from_millis(20)).await;
    }

    panic!("daemon recovery log did not become available");
  }
}
