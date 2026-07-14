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
mod runtime_guard;
mod runtime_guard_identity;
mod runtime_guard_protocol;
mod runtime_guard_record;
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
use cadder_protocol::{LogAttributionKind, LogSeverity, LogStreamIdentity};
use runtime_guard_record::{
  RuntimeGuardGenerationBinding, RuntimeGuardGenerationLock, RuntimeGuardReplacementBinding,
};
use runtime_lock::{DaemonLockCandidate, RuntimeContainmentMetadata};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, watch};
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
  pub runtime_guard_executable: Option<PathBuf>,
}

const DAEMON_RUNTIME_RELEASE_ATTEMPTS: usize = 50;
const DAEMON_RUNTIME_RELEASE_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub async fn run_daemon(options: DaemonOptions, shutdown: watch::Receiver<bool>) -> Result<()> {
  let paths = RuntimePaths::resolve_with_profile(options.runtime_dir, options.runtime_profile)?;
  paths.ensure_dirs()?;
  let Some(candidate) = acquire_daemon_lock_or_wait_for_ready(&paths).await? else {
    return Ok(());
  };
  let containment_lock = acquire_containment_lock(&paths).await?;
  let replacement_proof = containment_lock.prove_replacement(candidate.expected_containment())?;
  let mut lock = Some(candidate.publish_after_proof(&paths, replacement_proof)?);
  let lock_recovery = lock.as_ref().and_then(|lock| lock.recovery()).cloned();
  let caddy_backend = options
    .caddy_backend
    .map_or_else(CaddyBackendMode::from_env, Ok)?;
  if caddy_backend == CaddyBackendMode::Mock && options.real_caddy_override.is_some() {
    bail!("--real-caddy cannot be combined with --caddy-backend mock");
  }
  let (real_caddy, pinned_caddy) = match caddy_backend {
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
      let claim = resolver.pin().await?.runtime_guard_identity();
      (Some(resolver), Some(claim))
    }
    CaddyBackendMode::Mock => (None, None),
  };
  let endpoint =
    IpcEndpointMetadata::new(&paths).context("create the daemon generation identity")?;
  let containment_metadata;
  let guard = if let Some(executable) = options.runtime_guard_executable.as_deref() {
    let owner_generation = lock
      .as_ref()
      .expect("daemon lock is available before guard handoff")
      .owner_generation()
      .context("daemon lock did not retain its owner generation")?;
    let generation = runtime_guard_record::RuntimeGuardGeneration::random()?;
    let context = runtime_guard_record::RuntimeGuardGenerationContext {
      profile: paths.runtime_profile().to_string(),
      runtime_id: paths.instance_key().to_string(),
      daemon_instance_id: endpoint.daemon_instance_id.clone(),
      owner_generation: owner_generation.to_string(),
      nonce_commitment: generation.commitment().to_string(),
    };
    let mut guard = runtime_guard::RuntimeGuardClient::spawn_authenticated(
      executable,
      &paths,
      context.clone(),
      &generation,
      pinned_caddy,
    )
    .await?;
    containment_lock.publish(&runtime_guard_record::RuntimeGuardRecord::preparing(
      context.clone(),
    ))?;
    drop(containment_lock);
    let guard_identity = guard.wait_until_ready().await?;
    containment_metadata = Some(RuntimeContainmentMetadata::new(
      lock
        .take()
        .expect("daemon lock is available before guard handoff"),
      RuntimeGuardReplacementBinding {
        generation: RuntimeGuardGenerationBinding {
          context,
          guard: guard_identity,
        },
        last_child: None,
      },
    )?);
    Some(Arc::new(Mutex::new(guard)))
  } else {
    drop(containment_lock);
    containment_metadata = None;
    None
  };

  let coordinator = match caddy_backend {
    CaddyBackendMode::Real => {
      let real_caddy = real_caddy.expect("real backend pins its Caddy resolver");
      let adapter = CaddyConfigAdapter::new(real_caddy.clone());
      let runtime = match &guard {
        Some(guard) => ProcessRuntime::guarded(
          real_caddy,
          paths.clone(),
          Arc::clone(guard),
          containment_metadata
            .clone()
            .expect("runtime guard has containment metadata"),
        ),
        None => ProcessRuntime::new(real_caddy, paths.clone()),
      };
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

  let server = DaemonServer::new(paths, state).with_endpoint(endpoint);
  let (result, guard_failure) = match &guard {
    Some(guard) => run_server_with_guard_supervision(server, shutdown, Arc::clone(guard)).await,
    None => (server.run_until(shutdown).await, Ok(())),
  };
  let guard_result = match guard {
    Some(guard) if guard_failure.is_ok() => guard.lock().await.finalize().await,
    Some(_) => guard_failure,
    None => Ok(()),
  };
  drop(containment_metadata);
  drop(lock);
  result.and(guard_result)
}

async fn run_server_with_guard_supervision(
  server: DaemonServer,
  mut shutdown: watch::Receiver<bool>,
  guard: Arc<Mutex<runtime_guard::RuntimeGuardClient>>,
) -> (Result<()>, Result<()>) {
  let (local_shutdown, local_shutdown_rx) = watch::channel(false);
  let external_shutdown = local_shutdown.clone();
  let forward_shutdown = tokio::spawn(async move {
    if !*shutdown.borrow() {
      while shutdown.changed().await.is_ok() && !*shutdown.borrow() {}
    }
    let _ = external_shutdown.send(true);
  });
  let guard_shutdown = local_shutdown;
  let monitor = tokio::spawn(async move {
    loop {
      sleep(Duration::from_millis(20)).await;
      let Ok(mut guard) = guard.try_lock() else {
        continue;
      };
      if let Some(status) = guard.try_wait()? {
        let _ = guard_shutdown.send(true);
        bail!("runtime guard exited unexpectedly with status {status}");
      }
    }
  });

  let result = server.run_until(local_shutdown_rx).await;
  forward_shutdown.abort();
  let guard_failure = if monitor.is_finished() {
    match monitor.await {
      Ok(result) => result,
      Err(error) => Err(error).context("join runtime guard supervisor"),
    }
  } else {
    monitor.abort();
    let _ = monitor.await;
    Ok(())
  };
  (result, guard_failure)
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
    "previous cadderd owner did not release socket and daemon lock before timeout for runtime {}; retry after it exits before removing stale runtime files",
    paths.runtime_dir().display(),
  )
}

async fn acquire_daemon_lock_or_wait_for_ready(
  paths: &RuntimePaths,
) -> Result<Option<DaemonLockCandidate>> {
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

  if ipc::is_daemon_ready(paths).await? {
    return Ok(None);
  }

  bail!(
    "previous cadderd owner did not release socket and daemon lock before timeout for runtime {}; {}",
    paths.runtime_dir().display(),
    DaemonLock::active_owner_diagnostic(paths)
  )
}

async fn acquire_containment_lock(paths: &RuntimePaths) -> Result<RuntimeGuardGenerationLock> {
  for _ in 0..DAEMON_RUNTIME_RELEASE_ATTEMPTS {
    if let Some(lock) = RuntimeGuardGenerationLock::try_acquire(paths)? {
      return Ok(lock);
    }
    sleep(DAEMON_RUNTIME_RELEASE_POLL_INTERVAL).await;
  }

  bail!(
    "previous runtime guard did not release containment lock before timeout for runtime {}; leave recorded processes untouched and inspect the runtime profile before retrying",
    paths.runtime_dir().display()
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_protocol::{QueryStateRequest, QueryStateResponse, message_types, new_request_id};
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
}
