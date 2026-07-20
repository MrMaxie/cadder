mod autostart;
mod caddy;
mod caddy_image;
mod caddy_path_trust;
mod config;
mod ipc;
mod ipc_client_error;
mod ipc_codec;
#[allow(dead_code)]
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
#[allow(dead_code)]
mod runtime_guard;
#[allow(dead_code)]
mod runtime_guard_identity;
#[allow(dead_code)]
mod runtime_guard_protocol;
#[allow(dead_code)]
mod runtime_guard_record;
#[allow(dead_code)]
mod runtime_lock;
mod state;
mod storage;

pub use caddy::{
  CaddyBackendMode, CaddyConfigAdapter, CaddyConfigCoordinator, CaddyRegistrationAdapter,
  RealCaddyResolver,
};
pub use config::{CONFIG_FILE_NAME, CadderConfig, RuntimeConfig};
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
pub use runtime_guard::{RuntimeGuardHiddenOptions, run_runtime_guard};
pub use runtime_guard_protocol::{
  RUNTIME_GUARD_PROTOCOL_REVISION, RuntimeGuardBootstrapAuthenticatedResponse,
  RuntimeGuardBootstrapRequest, RuntimeGuardCommandRequest, RuntimeGuardFinalizedResponse,
  RuntimeGuardLogFrame, RuntimeGuardProtocol, RuntimeGuardProtocolError, RuntimeGuardReadyResponse,
  RuntimeGuardRequest, RuntimeGuardResponse, RuntimeGuardStartRequest, RuntimeGuardStartedResponse,
  RuntimeGuardStatusResponse, RuntimeGuardTerminatedResponse,
};
pub use runtime_guard_record::{
  RuntimeGuardChildIdentity, RuntimeGuardGenerationContext, RuntimeGuardIdentity,
  RuntimeGuardImageIdentity, RuntimeGuardPinnedCaddyIdentity, RuntimeGuardProcessIdentity,
  RuntimeGuardRecordState, RuntimeGuardTerminalOutcome, RuntimeGuardTerminalReason,
};
pub use runtime_lock::DaemonLock;
pub use state::DaemonState;
pub use storage::RuntimeStore;

use anyhow::{Context, Result, bail};
#[cfg(test)]
use runtime_guard_record::RuntimeGuardGenerationLock;
#[cfg(test)]
use runtime_lock::DaemonLockCandidate;
use std::path::PathBuf;
use tokio::sync::watch;
use tokio::time::{Duration, sleep};

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone)]
pub struct DaemonOptions {
  /// Test-only/internal runtime selection seam. Production entrypoints always use `None`.
  pub runtime_dir: Option<PathBuf>,
  /// Test-only/internal runtime selection seam. Production entrypoints always use `None`.
  pub runtime_profile: Option<RuntimeProfile>,
  pub real_caddy_override: Option<PathBuf>,
  pub caddy_backend: Option<CaddyBackendMode>,
  /// Ignored compatibility field retained while downstream launchers migrate
  /// away from the removed runtime-guard process.
  pub runtime_guard_executable: Option<PathBuf>,
}

const DAEMON_RUNTIME_RELEASE_ATTEMPTS: usize = 50;
const DAEMON_RUNTIME_RELEASE_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub async fn run_daemon(options: DaemonOptions, shutdown: watch::Receiver<bool>) -> Result<()> {
  let paths = RuntimePaths::resolve_with_profile(options.runtime_dir, options.runtime_profile)?;
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
        RealCaddyResolver::for_daemon(options.real_caddy_override, paths.runtime_profile())
      };
      #[cfg(not(debug_assertions))]
      let resolver =
        RealCaddyResolver::for_daemon(options.real_caddy_override, paths.runtime_profile());
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
async fn wait_for_daemon_runtime_released(paths: &RuntimePaths) -> Result<()> {
  wait_for_daemon_runtime_released_with_limits(
    paths,
    DAEMON_RUNTIME_RELEASE_ATTEMPTS,
    DAEMON_RUNTIME_RELEASE_POLL_INTERVAL,
  )
  .await
}

#[cfg(test)]
async fn wait_for_daemon_runtime_released_with_limits(
  paths: &RuntimePaths,
  attempts: usize,
  poll_interval: Duration,
) -> Result<()> {
  for _ in 0..attempts {
    if ipc::is_daemon_ready(paths).await? || DaemonLock::try_acquire(paths.lock_path())?.is_none() {
      sleep(poll_interval).await;
      continue;
    }
    return Ok(());
  }
  bail!("previous cadderd owner did not release socket and daemon lock")
}

#[cfg(test)]
async fn acquire_daemon_lock_or_wait_for_ready_with_limits(
  paths: &RuntimePaths,
  attempts: usize,
  poll_interval: Duration,
) -> Result<Option<DaemonLockCandidate>> {
  for _ in 0..attempts {
    if ipc::is_daemon_ready(paths).await? {
      return Ok(None);
    }
    if let Some(candidate) = DaemonLock::try_acquire_candidate(paths)? {
      return Ok(Some(candidate));
    }
    sleep(poll_interval).await;
  }
  bail!("previous cadderd owner did not release socket and daemon lock")
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_ipc::{QueryStateRequest, QueryStateResponse, message_types, new_request_id};
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
        runtime_guard_executable: None,
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
        runtime_guard_executable: None,
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
      .query_history(Some(cadder_ipc::HistoryKind::Runtime), 10)
      .await;
    assert!(
      history
        .iter()
        .any(|record| record.summary == "Daemon shutdown requested.")
    );
    store.contain_shutdown().await.unwrap();
  }

  #[tokio::test]
  #[ignore = "replaced by endpoint-lease shutdown coverage"]
  async fn run_daemon_shutdown_storage_retains_discovery_and_lock_until_flush_finishes() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let client = CadderClient::new(paths.clone());
    let (store, release_storage) = RuntimeStore::memory_stalled_for_test(1);
    let mut state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths.clone()));
    state.set_runtime_store_for_test(store);
    let server = DaemonServer::new(paths.clone(), state);
    let candidate = DaemonLock::try_acquire_candidate(&paths).unwrap().unwrap();
    let containment = RuntimeGuardGenerationLock::try_acquire(&paths)
      .unwrap()
      .unwrap();
    let proof = containment.prove_replacement(None).unwrap();
    let lock = candidate.publish_after_proof(&paths, proof).unwrap();
    drop(containment);
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
  #[ignore = "endpoint ownership requires an authenticated daemon handshake"]
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
        runtime_guard_executable: None,
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
          runtime_guard_executable: None,
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
  async fn run_daemon_ignores_and_removes_legacy_runtime_artifacts_after_claiming_endpoint() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    for name in [
      "cadder.lock",
      "cadder.lock.json",
      "cadder-containment.lock",
      "cadder-containment.json",
      "cadder-launch.lock",
      "cadder-ipc.lock",
      "cadder-ipc.json",
    ] {
      std::fs::write(runtime_dir.join(name), "not-runtime-state").unwrap();
    }
    let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
    let client = CadderClient::new(paths.clone());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon = tokio::spawn(run_daemon(
      DaemonOptions {
        runtime_dir: Some(runtime_dir),
        runtime_profile: None,
        real_caddy_override: None,
        caddy_backend: Some(CaddyBackendMode::Mock),
        runtime_guard_executable: None,
      },
      shutdown_rx,
    ));

    assert!(wait_for_query_state(&client).await.accepted);
    for name in [
      "cadder.lock",
      "cadder.lock.json",
      "cadder-containment.lock",
      "cadder-containment.json",
      "cadder-launch.lock",
      "cadder-ipc.lock",
      "cadder-ipc.json",
    ] {
      assert!(!paths.runtime_dir().join(name).exists());
    }
    shutdown_tx.send(true).unwrap();
    daemon.await.unwrap().unwrap();
  }

  #[tokio::test]
  async fn second_daemon_attaches_to_the_live_endpoint_without_creating_runtime_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
    let client = CadderClient::new(paths.clone());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let first = tokio::spawn(run_daemon(
      DaemonOptions {
        runtime_dir: Some(runtime_dir.clone()),
        runtime_profile: None,
        real_caddy_override: None,
        caddy_backend: Some(CaddyBackendMode::Mock),
        runtime_guard_executable: None,
      },
      shutdown_rx,
    ));
    assert!(wait_for_query_state(&client).await.accepted);

    let (_second_shutdown_tx, second_shutdown_rx) = watch::channel(false);
    let second = tokio::spawn(run_daemon(
      DaemonOptions {
        runtime_dir: Some(runtime_dir),
        runtime_profile: None,
        real_caddy_override: None,
        caddy_backend: Some(CaddyBackendMode::Mock),
        runtime_guard_executable: None,
      },
      second_shutdown_rx,
    ));
    timeout(Duration::from_secs(2), second)
      .await
      .unwrap()
      .unwrap()
      .unwrap();
    assert!(wait_for_query_state(&client).await.accepted);

    shutdown_tx.send(true).unwrap();
    first.await.unwrap().unwrap();
  }

  #[tokio::test]
  #[ignore = "stale lock metadata is intentionally ignored"]
  async fn run_daemon_rejects_stale_lock_without_containment_proof() {
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
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let error = run_daemon(
      DaemonOptions {
        runtime_dir: Some(runtime_dir),
        real_caddy_override: None,
        runtime_profile: None,
        caddy_backend: Some(CaddyBackendMode::Mock),
        runtime_guard_executable: None,
      },
      shutdown_rx,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("do not identify the same"));
    assert!(!paths.ipc_endpoint_path().exists());
    assert!(
      fs::read_to_string(paths.lock_metadata_path())
        .unwrap()
        .contains("00112233445566778899aabbccddeeff")
    );
  }

  #[tokio::test]
  #[ignore = "file-lock ownership is intentionally removed"]
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
  #[ignore = "file-lock ownership is intentionally removed"]
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
  #[ignore = "file-lock ownership is intentionally removed"]
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
  #[ignore = "file-lock ownership is intentionally removed"]
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
}
