use super::*;
use crate::{
  CaddyConfigAdapter, CaddyConfigCoordinator, ProcessRuntime, RealCaddyResolver, RuntimePaths,
  operation_fence::RevokeOutcome,
};
use cadder_ipc::{
  AutostartMode, AutostartStatus, EntrypointInstanceIdentity, LogAttributionKind, LogSeverity,
  LogStreamIdentity, LogStreamStatus, OwnerProcessIdentity, QueryLogsRequest, RegisteredDomain,
  SourcePath,
};
use chrono::Utc;
use std::{
  fs,
  path::{Path, PathBuf},
};

struct StateFixture {
  state: DaemonState,
  _temp: tempfile::TempDir,
}

impl std::ops::Deref for StateFixture {
  type Target = DaemonState;

  fn deref(&self) -> &Self::Target {
    &self.state
  }
}

impl std::ops::DerefMut for StateFixture {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.state
  }
}

fn state() -> StateFixture {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().to_path_buf())).unwrap();
  let resolver = RealCaddyResolver::for_daemon(
    Some(temp.path().join("definitely-missing-caddy")),
    paths.runtime_profile(),
  );
  let adapter = CaddyConfigAdapter::new(resolver.clone());
  let runtime = ProcessRuntime::new(resolver, paths);
  StateFixture {
    state: DaemonState::new(CaddyConfigCoordinator::new(adapter, runtime)),
    _temp: temp,
  }
}

#[tokio::test]
async fn shutdown_coordinator_prepare_shutdown_bounds_config_operation_wait() {
  let fixture = state();
  let operation = fixture.config_operation.acquire().await.unwrap();
  let budget = std::time::Duration::from_millis(25);
  let started = tokio::time::Instant::now();

  let preparation = fixture.prepare_shutdown_until(started + budget).await;

  assert!(!preparation.response.accepted);
  assert!(!preparation.runtime_quiescent);
  assert!(preparation.response.message.contains("runtime operation"));
  assert!(started.elapsed() <= budget + std::time::Duration::from_millis(100));
  drop(operation);
}

fn state_with_fake_caddy(caddy: &Path) -> StateFixture {
  let (state, _) = state_with_fake_caddy_paths(caddy);
  state
}

fn state_with_fake_caddy_paths(caddy: &Path) -> (StateFixture, RuntimePaths) {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().to_path_buf())).unwrap();
  let resolver = RealCaddyResolver::for_test_fixture(caddy.to_path_buf());
  let adapter = CaddyConfigAdapter::new(resolver.clone());
  let runtime = ProcessRuntime::new(resolver, paths.clone());
  (
    StateFixture {
      state: DaemonState::new(CaddyConfigCoordinator::new(adapter, runtime)),
      _temp: temp,
    },
    paths,
  )
}

fn write_slow_adapt_fake_caddy(path: &Path, marker: &Path) {
  #[cfg(windows)]
    fs::write(
      path,
      format!(
        r#"@echo off
if "%1"=="adapt" (
  echo started>"{}"
  ping -n 3 127.0.0.1 >nul
  echo {{"apps":{{"http":{{"servers":{{"srv0":{{"routes":[{{"match":[{{"host":["app.localhost"]}}],"handle":[{{"handler":"static_response","body":"ok"}}],"terminal":true}}]}}}}}}}}}}
  exit /b 0
)
if "%1"=="reload" exit /b 0
if "%1"=="stop" exit /b 0
if "%1"=="run" (
  ping -n 6 127.0.0.1 >nul
  exit /b 0
)
exit /b 1
"#,
        marker.display()
      ),
    )
    .unwrap();

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::write(
        path,
        format!(
          r#"#!/usr/bin/env sh
if [ "$1" = "adapt" ]; then
  printf '%s\n' started > '{}'
  sleep 1
  printf '%s\n' '{{"apps":{{"http":{{"servers":{{"srv0":{{"routes":[{{"match":[{{"host":["app.localhost"]}}],"handle":[{{"handler":"static_response","body":"ok"}}],"terminal":true}}]}}}}}}}}}}'
  exit 0
fi
if [ "$1" = "reload" ] || [ "$1" = "stop" ]; then
  exit 0
fi
if [ "$1" = "run" ]; then
  sleep 5
  exit 0
fi
exit 1
"#,
          marker.display()
        ),
      )
      .unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
  }
}

fn write_transaction_fake_caddy(path: &Path, stop_marker: &Path) {
  #[cfg(windows)]
  fs::write(
    path,
    format!(
      r#"@echo off
if "%1"=="adapt" (
  echo {{"apps":{{"http":{{"servers":{{"srv0":{{"routes":[{{"match":[{{"host":["app.localhost"]}}],"handle":[{{"handler":"static_response","body":"ok"}}],"terminal":true}}]}}}}}}}}}}
  exit /b 0
)
if "%1"=="reload" exit /b 0
if "%1"=="stop" (
  echo stop>"{}"
  exit /b 0
)
if "%1"=="run" (
  :run_loop
  if exist "{}" exit /b 0
  ping -n 2 127.0.0.1 >nul
  goto run_loop
)
exit /b 1
"#,
      stop_marker.display(),
      stop_marker.display()
    ),
  )
  .unwrap();

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::write(
      path,
      format!(
        r#"#!/usr/bin/env sh
case "$1" in
  adapt)
    printf '%s\n' '{{"apps":{{"http":{{"servers":{{"srv0":{{"routes":[{{"match":[{{"host":["app.localhost"]}}],"handle":[{{"handler":"static_response","body":"ok"}}],"terminal":true}}]}}}}}}}}}}'
    exit 0
    ;;
  reload)
    exit 0
    ;;
  stop)
    : > '{}'
    exit 0
    ;;
  run)
    while [ ! -f '{}' ]; do sleep 0.02; done
    exit 0
    ;;
esac
exit 1
"#,
        stop_marker.display(),
        stop_marker.display()
      ),
    )
    .unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
  }
}

async fn wait_for_marker(path: &Path) {
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    while !path.exists() {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await
  .unwrap();
}

fn fake_caddy_path(dir: &Path) -> PathBuf {
  #[cfg(windows)]
  {
    dir.join("fake-caddy.cmd")
  }
  #[cfg(not(windows))]
  {
    dir.join("fake-caddy")
  }
}

fn registration(id: &str, nonce: &str) -> EntrypointRegistration {
  let now = Utc::now();
  EntrypointRegistration {
    registration_id: id.to_string(),
    entrypoint_instance: EntrypointInstanceIdentity {
      instance_id: id.to_string(),
      started_at_utc: now,
      shim_session_nonce: nonce.to_string(),
    },
    source_working_directory: SourcePath::new(".", None),
    source_config_path: SourcePath::new("Caddyfile", None),
    registered_domains: vec![RegisteredDomain::active("app.localhost")],
    activation_state: ActivationState::Active,
    owner_process: OwnerProcessIdentity {
      process_id: 1,
      process_start_time_utc: now,
      shim_session_nonce: nonce.to_string(),
      executable_path: None,
    },
    log_stream: LogStreamIdentity::entrypoint(id),
    shim_run: None,
    created_at_utc: now,
    last_heartbeat_utc: now,
  }
}
#[tokio::test]
async fn shutdown_signal_notifies_pending_and_pre_requested_waiters() {
  let signal = ShutdownSignal::default();
  let waiter_signal = signal.clone();
  let waiter = tokio::spawn(async move {
    waiter_signal.wait().await;
  });
  tokio::task::yield_now().await;

  signal.request();

  tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
    .await
    .unwrap()
    .unwrap();
  tokio::time::timeout(std::time::Duration::from_secs(1), signal.wait())
    .await
    .unwrap();
}

#[tokio::test]
async fn register_and_unregister_preserve_owner_boundary() {
  let state = state();
  let mut events = state.subscribe();
  let response = state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  assert!(response.accepted);
  let registered = events.try_recv().unwrap();
  assert_eq!(registered.sequence_number, 1);
  assert_eq!(registered.snapshot.registrations.len(), 1);

  let wrong = state
    .unregister("wrong".to_string(), "shim-1", "other")
    .await;
  assert!(!wrong.accepted);
  assert_eq!(state.snapshot().await.registrations.len(), 1);
  assert!(matches!(
    events.try_recv(),
    Err(broadcast::error::TryRecvError::Empty)
  ));

  let right = state
    .unregister("right".to_string(), "shim-1", "nonce-1")
    .await;
  assert!(right.accepted);
  assert!(state.snapshot().await.registrations.is_empty());
  let unregistered = events.try_recv().unwrap();
  assert_eq!(unregistered.sequence_number, 2);
  assert!(unregistered.snapshot.registrations.is_empty());
  assert_eq!(state.inner.lock().await.sequence, 2);
  let history = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "registration-history".to_string(),
      kind: Some(HistoryKind::Registration),
      limit: Some(10),
    })
    .await;
  assert_eq!(history.records.len(), 2);
  assert_eq!(
    history
      .records
      .iter()
      .filter(|record| record.summary == "Unregistered entrypoint `shim-1`.")
      .count(),
    1
  );
}

#[tokio::test]
async fn unregister_unknown_registration_returns_not_found() {
  let state = state();

  let response = state
    .unregister("unregister".to_string(), "missing", "nonce-1")
    .await;

  assert!(!response.accepted);
  assert_eq!(
    response.message,
    "Entrypoint was not found for the requested owner."
  );
}

#[tokio::test]
async fn ipc_disconnect_cleanup_logs_failed_unregister() {
  let state = state();

  state
    .unregister_for_ipc_disconnect("missing", "nonce-1")
    .await;
  let logs = state
    .query_logs(QueryLogsRequest {
      request_id: "logs".to_string(),
      stream: LogStreamIdentity::runtime_control(),
      limit: Some(10),
      cursor: None,
      minimum_severity: None,
    })
    .await;

  assert!(logs.entries.iter().any(|entry| {
    entry.operation.as_deref() == Some("ipc-disconnect-cleanup")
      && entry.severity == LogSeverity::Warn
      && entry
        .raw_message
        .contains("Entrypoint was not found for the requested owner.")
  }));
}

#[tokio::test]
async fn disabled_domain_log_stream_is_reported_stale() {
  let state = state();
  let response = state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  assert!(response.accepted);
  let stream = LogStreamIdentity::domain("app.localhost");
  state.logs().append(
    stream.clone(),
    LogSeverity::Info,
    "domain log",
    LogAttributionKind::Domain,
    None,
  );

  let toggle = state
    .set_domain_enabled(SetDomainEnabledRequest {
      request_id: "toggle".to_string(),
      registration_id: "shim-1".to_string(),
      domain_key: "app.localhost".to_string(),
      enabled: false,
    })
    .await;
  assert!(toggle.accepted);

  let logs = state
    .query_logs(QueryLogsRequest {
      request_id: "logs".to_string(),
      stream,
      limit: Some(10),
      cursor: None,
      minimum_severity: None,
    })
    .await;

  assert_eq!(logs.stream_status, LogStreamStatus::Stale);
  assert_eq!(logs.entries.len(), 1);
}

#[tokio::test]
async fn entrypoint_log_stream_tracks_activation_state() {
  let state = state();
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  let stream = LogStreamIdentity::entrypoint("shim-1");
  state.logs().append(
    stream.clone(),
    LogSeverity::Info,
    "entrypoint log",
    LogAttributionKind::Entrypoint,
    None,
  );

  let active = state
    .query_logs(QueryLogsRequest {
      request_id: "active-logs".to_string(),
      stream: stream.clone(),
      limit: Some(10),
      cursor: None,
      minimum_severity: None,
    })
    .await;
  let disabled = state
    .set_entrypoint_enabled(SetEntrypointEnabledRequest {
      request_id: "disable".to_string(),
      registration_id: "shim-1".to_string(),
      shim_session_nonce: Some("nonce-1".to_string()),
      enabled: false,
    })
    .await;
  let stale = state
    .query_logs(QueryLogsRequest {
      request_id: "stale-logs".to_string(),
      stream,
      limit: Some(10),
      cursor: None,
      minimum_severity: None,
    })
    .await;

  assert_eq!(active.stream_status, LogStreamStatus::Active);
  assert!(disabled.accepted);
  assert_eq!(stale.stream_status, LogStreamStatus::Stale);
  assert_eq!(stale.entries.len(), 1);
}
#[tokio::test]
async fn set_autostart_fails_closed_without_history() {
  let state = state();

  let response = state
    .set_autostart(SetAutostartRequest {
      request_id: "autostart-set".to_string(),
      mode: AutostartMode::Daemon,
    })
    .await;
  let history = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "history".to_string(),
      kind: Some(HistoryKind::Autostart),
      limit: Some(10),
    })
    .await;

  assert!(!response.accepted);
  assert_eq!(response.status, AutostartStatus::Unsupported);
  assert!(response.target.is_none());
  assert_eq!(
    response
      .diagnostics
      .iter()
      .map(|diagnostic| diagnostic.code.as_str())
      .collect::<Vec<_>>(),
    vec!["autostart-update-unavailable"]
  );
  assert!(history.records.is_empty());
}

#[tokio::test]
async fn revoked_autostart_fence_prevents_apply_and_history() {
  let state = state();
  let fence = state.issue_operation_fence().unwrap();
  fence.revoke();

  let result = state
    .set_autostart_fenced(
      SetAutostartRequest {
        request_id: "autostart-revoked".to_string(),
        mode: AutostartMode::Daemon,
      },
      &fence,
    )
    .await;
  let history = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "history".to_string(),
      kind: Some(HistoryKind::Autostart),
      limit: Some(10),
    })
    .await;

  assert_eq!(result.unwrap_err(), CommitRejection::Revoked);
  assert!(history.records.is_empty());
}

#[tokio::test]
async fn unavailable_autostart_response_finalizes_without_platform_apply() {
  let state = state();
  let fence = state.issue_operation_fence().unwrap();

  let response = state
    .set_autostart_fenced(
      SetAutostartRequest {
        request_id: "autostart-finalized".to_string(),
        mode: AutostartMode::Daemon,
      },
      &fence,
    )
    .await
    .unwrap();

  assert!(!response.accepted);
  assert_eq!(fence.try_revoke(), RevokeOutcome::Finalized);
}
#[tokio::test]
async fn query_autostart_reports_disabled_manager_status() {
  let state = state();

  let response = state.query_autostart("autostart".to_string()).await;

  assert_eq!(response.request_id, "autostart");
  assert!(response.accepted);
  assert_eq!(response.status, AutostartStatus::Disabled);
  assert_eq!(response.mode, AutostartMode::Disabled);
  assert!(response.target.is_none());
}

#[cfg(all(unix, not(target_os = "macos")))]
#[tokio::test]
async fn query_autostart_reports_unavailable_linux_config_dir() {
  let state = state();

  let response = state.query_autostart("autostart".to_string()).await;

  assert_eq!(response.request_id, "autostart");
  assert!(!response.accepted);
  assert_eq!(response.status, AutostartStatus::Unsupported);
  assert_eq!(response.mode, AutostartMode::Disabled);
  assert!(response.target.is_none());
  assert_eq!(
    response
      .diagnostics
      .iter()
      .map(|diagnostic| diagnostic.code.as_str())
      .collect::<Vec<_>>(),
    vec!["autostart-config-dir-unavailable"]
  );
}
#[tokio::test]
async fn register_rejects_invalid_owner_identity() {
  let state = state();
  let mut registration = registration("shim-1", "nonce-1");
  registration.owner_process.shim_session_nonce = "different".to_string();

  let response = state.register("register".to_string(), registration).await;

  assert!(!response.accepted);
  assert!(response.message.contains("nonce values must match"));
  assert!(state.snapshot().await.registrations.is_empty());
}

#[tokio::test]
async fn register_rejects_existing_registration_id_owned_by_different_nonce() {
  let state = state();
  let first = state
    .register("first".to_string(), registration("shim-1", "nonce-1"))
    .await;
  assert!(first.accepted, "{first:?}");

  let second = state
    .register("second".to_string(), registration("shim-1", "nonce-2"))
    .await;
  let snapshot = state.snapshot().await;

  assert!(!second.accepted);
  assert!(second.message.contains("already owned"));
  assert_eq!(snapshot.registrations.len(), 1);
  assert_eq!(
    snapshot.registrations[0]
      .entrypoint_instance
      .shim_session_nonce,
    "nonce-1"
  );
}

#[tokio::test]
async fn subscribe_receives_registration_change_event() {
  let state = state();
  let mut events = state.subscribe();

  let response = state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  let event = events.recv().await.unwrap();

  assert!(response.accepted);
  assert_eq!(event.sequence_number, 1);
  assert_eq!(event.change_kind, StateChangeKind::RegistrationsChanged);
  assert_eq!(event.registration_id.as_deref(), Some("shim-1"));
  assert_eq!(event.snapshot.registrations.len(), 1);
}

#[tokio::test]
async fn registration_event_matches_the_published_mock_runtime_state() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths));
  let mut events = state.subscribe();

  let response = state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  let event = events.recv().await.unwrap();
  let snapshot = state.snapshot().await;

  assert!(response.accepted);
  assert_eq!(event.snapshot.registrations, snapshot.registrations);
  assert_eq!(event.snapshot.runtime, snapshot.runtime);
  assert_eq!(event.snapshot.config, snapshot.config);
  assert_eq!(
    event.snapshot.runtime.status,
    cadder_ipc::RuntimeStatus::Running
  );
}

#[tokio::test]
async fn inactive_registration_commits_through_the_stop_transaction() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths.clone()));
  let mut candidate = registration("shim-1", "nonce-1");
  candidate.activation_state = ActivationState::Inactive;

  let response = state
    .register("register-inactive".to_string(), candidate)
    .await;
  let snapshot = state.snapshot().await;

  assert!(response.accepted);
  assert_eq!(snapshot.runtime.status, cadder_ipc::RuntimeStatus::Idle);
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Idle);
  assert_eq!(snapshot.registrations.len(), 1);
  assert!(!paths.effective_config_path().exists());
}

#[tokio::test]
async fn heartbeat_accepts_owner_and_rejects_wrong_nonce() {
  let state = state();
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  let sequence_before = state.inner.lock().await.sequence;
  let mut events = state.subscribe();

  let accepted = state
    .heartbeat(HeartbeatEntrypointRequest {
      request_id: "heartbeat".to_string(),
      registration_id: "shim-1".to_string(),
      shim_session_nonce: "nonce-1".to_string(),
    })
    .await;
  let rejected = state
    .heartbeat(HeartbeatEntrypointRequest {
      request_id: "heartbeat".to_string(),
      registration_id: "shim-1".to_string(),
      shim_session_nonce: "wrong".to_string(),
    })
    .await;

  assert!(accepted.accepted);
  assert_eq!(accepted.message, "Heartbeat accepted.");
  assert_eq!(state.inner.lock().await.sequence, sequence_before);
  assert!(matches!(
    events.try_recv(),
    Err(broadcast::error::TryRecvError::Empty)
  ));
  assert!(!rejected.accepted);
  assert_eq!(
    rejected.message,
    "Entrypoint was not found for the requested owner."
  );
}

#[tokio::test]
async fn revoked_heartbeat_does_not_renew_the_registration_lease() {
  let state = state();
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  let before = state
    .inner
    .lock()
    .await
    .registrations
    .get("shim-1")
    .unwrap()
    .last_heartbeat_utc;
  let fence = state.issue_operation_fence().unwrap();
  fence.revoke();

  let result = state
    .heartbeat_fenced(
      HeartbeatEntrypointRequest {
        request_id: "heartbeat-revoked".to_string(),
        registration_id: "shim-1".to_string(),
        shim_session_nonce: "nonce-1".to_string(),
      },
      &fence,
    )
    .await;
  let after = state
    .inner
    .lock()
    .await
    .registrations
    .get("shim-1")
    .unwrap()
    .last_heartbeat_utc;

  assert_eq!(result.unwrap_err(), CommitRejection::Revoked);
  assert_eq!(after, before);
}

#[tokio::test]
async fn set_entrypoint_enabled_accepts_optional_owner_and_rejects_wrong_owner() {
  let state = state();
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;

  let accepted = state
    .set_entrypoint_enabled(SetEntrypointEnabledRequest {
      request_id: "disable".to_string(),
      registration_id: "shim-1".to_string(),
      shim_session_nonce: None,
      enabled: false,
    })
    .await;
  let rejected = state
    .set_entrypoint_enabled(SetEntrypointEnabledRequest {
      request_id: "enable".to_string(),
      registration_id: "shim-1".to_string(),
      shim_session_nonce: Some("wrong".to_string()),
      enabled: true,
    })
    .await;
  let snapshot = state.snapshot().await;

  assert!(accepted.accepted);
  assert_eq!(accepted.message, "Entrypoint activation updated.");
  assert!(!rejected.accepted);
  assert_eq!(
    snapshot.registrations[0].activation_state,
    ActivationState::Inactive
  );
}

#[tokio::test]
async fn set_domain_enabled_rejects_unknown_domain() {
  let state = state();
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;

  let response = state
    .set_domain_enabled(SetDomainEnabledRequest {
      request_id: "toggle".to_string(),
      registration_id: "shim-1".to_string(),
      domain_key: "missing.localhost".to_string(),
      enabled: false,
    })
    .await;

  assert!(!response.accepted);
  assert_eq!(response.message, "Domain was not found.");
}

#[tokio::test]
async fn query_state_returns_snapshot_with_request_id() {
  let state = state();
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;

  let response = state.query_state("state".to_string()).await;

  assert!(response.accepted);
  assert_eq!(response.request_id, "state");
  assert_eq!(response.message, "State snapshot returned.");
  assert_eq!(response.snapshot.unwrap().registrations.len(), 1);
}

#[tokio::test]
async fn query_state_does_not_wait_for_stalled_history_worker() {
  let mut state = state();
  let (store, release) = RuntimeStore::memory_stalled_for_test(1);
  state.store = store;
  state.store.record_history(
    HistoryKind::Runtime,
    "Queued history event.",
    None,
    None,
    &serde_json::json!({ "queued": true }),
  );

  let response = tokio::time::timeout(
    std::time::Duration::from_millis(100),
    state.query_state("state".to_string()),
  )
  .await
  .unwrap();

  assert!(response.accepted);
  assert_eq!(
    response
      .snapshot
      .and_then(|snapshot| snapshot.storage)
      .map(|storage| storage.backend),
    Some("memory".to_string())
  );
  release.send(()).unwrap();
}

#[tokio::test]
async fn query_history_returns_persisted_registration_events() {
  let state = state();
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;

  let response = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "history".to_string(),
      kind: Some(HistoryKind::Registration),
      limit: Some(10),
    })
    .await;

  assert!(response.accepted);
  assert_eq!(response.request_id, "history");
  assert_eq!(response.records.len(), 1);
  assert_eq!(
    response.records[0].registration_id.as_deref(),
    Some("shim-1")
  );
  assert_eq!(
    response
      .storage
      .as_ref()
      .map(|state| state.backend.as_str()),
    Some("memory")
  );
}

#[tokio::test]
async fn query_history_reports_storage_diagnostics_when_worker_queue_is_full() {
  let mut state = state();
  let (store, release) = RuntimeStore::memory_stalled_for_test(1);
  state.store = store;
  state.store.record_history(
    HistoryKind::Runtime,
    "Queued history event.",
    None,
    None,
    &serde_json::json!({ "queued": true }),
  );

  let response = tokio::time::timeout(
    std::time::Duration::from_millis(100),
    state.query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "history".to_string(),
      kind: None,
      limit: Some(10),
    }),
  )
  .await
  .unwrap();
  let diagnostics = response.storage.unwrap().diagnostics;

  assert!(response.accepted);
  assert!(response.records.is_empty());
  assert!(
    diagnostics
      .iter()
      .any(|diagnostic| diagnostic.code == "storage-history-query-queue-full"),
    "{diagnostics:?}"
  );
  release.send(()).unwrap();
}

#[tokio::test]
async fn registration_storage_failure_rolls_back_runtime_and_memory_state() {
  let mut state = state();
  let (store, release) = RuntimeStore::memory_stalled_for_test(0);
  state.store = store;

  let response = state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  let snapshot = state
    .query_state("state".to_string())
    .await
    .snapshot
    .unwrap();

  assert!(!response.accepted);
  assert!(response.message.contains("durable storage failed"));
  assert!(snapshot.registrations.is_empty());
  assert_eq!(snapshot.runtime.status, cadder_ipc::RuntimeStatus::Idle);
  release.send(()).unwrap();
  state.store.contain_shutdown().await.unwrap();
}

#[tokio::test]
async fn register_revalidates_existing_owner_after_slow_adapt() {
  let temp = tempfile::tempdir().unwrap();
  let fake_caddy = fake_caddy_path(temp.path());
  let marker = temp.path().join("adapt-started");
  write_slow_adapt_fake_caddy(&fake_caddy, &marker);
  let state = state_with_fake_caddy(&fake_caddy);
  let registering_state = state.clone();

  let pending = tokio::spawn(async move {
    registering_state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await
  });
  wait_for_marker(&marker).await;
  {
    let mut inner = state.inner.lock().await;
    inner.registrations.insert(
      "shim-1".to_string(),
      registration("shim-1", "different-nonce"),
    );
  }

  let response = pending.await.unwrap();
  let snapshot = state.snapshot().await;

  assert!(!response.accepted, "{response:?}");
  assert_eq!(
    response.message,
    "Entrypoint registration ID is already owned by another shim session."
  );
  assert_eq!(snapshot.registrations.len(), 1);
  assert_eq!(
    snapshot.registrations[0]
      .entrypoint_instance
      .shim_session_nonce,
    "different-nonce"
  );
}

#[tokio::test]
async fn operation_fence_timeout_before_delayed_commit_preserves_state() {
  let temp = tempfile::tempdir().unwrap();
  let fake_caddy = fake_caddy_path(temp.path());
  let marker = temp.path().join("adapt-started");
  write_slow_adapt_fake_caddy(&fake_caddy, &marker);
  let state = state_with_fake_caddy(&fake_caddy);
  let registering_state = state.clone();
  let fence = state.issue_operation_fence().unwrap();
  let pending_fence = fence.clone();
  let mut events = state.subscribe();

  let pending = tokio::spawn(async move {
    registering_state
      .register_fenced(
        "operation-fence-timeout".to_string(),
        registration("shim-1", "nonce-1"),
        &pending_fence,
      )
      .await
  });
  wait_for_marker(&marker).await;
  fence.revoke();

  let result = pending.await.unwrap();
  let snapshot = state.snapshot().await;
  let history = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "operation-fence-history".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await;
  let sequence = state.inner.lock().await.sequence;

  assert_eq!(result.unwrap_err(), CommitRejection::Revoked);
  assert!(snapshot.registrations.is_empty());
  assert!(history.records.is_empty());
  assert_eq!(sequence, 0);
  assert!(matches!(
    events.try_recv(),
    Err(broadcast::error::TryRecvError::Empty)
  ));
}

#[tokio::test]
async fn operation_fence_revoke_after_runtime_apply_rolls_back_before_worker_finishes() {
  let temp = tempfile::tempdir().unwrap();
  let fake_caddy = fake_caddy_path(temp.path());
  let stop_marker = temp.path().join("stop-runtime");
  write_transaction_fake_caddy(&fake_caddy, &stop_marker);
  let (mut state, paths) = state_with_fake_caddy_paths(&fake_caddy);
  let hook = RegistrationPublishTestHook::new();
  state.registration_publish_hook = Some(hook.clone());
  let registering_state = state.state.clone();
  let fence = state.issue_operation_fence().unwrap();
  let pending_fence = fence.clone();
  let mut events = state.subscribe();

  let pending = tokio::spawn(async move {
    registering_state
      .register_fenced(
        "operation-fence-after-apply".to_string(),
        registration("shim-1", "nonce-1"),
        &pending_fence,
      )
      .await
  });
  hook.wait_until_reached().await;
  assert!(!paths.effective_config_path().exists());
  fence.revoke();
  hook.release();

  let result = pending.await.unwrap();
  let snapshot = state.snapshot().await;
  let history = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "operation-fence-after-apply-history".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await;

  assert_eq!(result.unwrap_err(), CommitRejection::Revoked);
  assert!(snapshot.registrations.is_empty());
  assert_eq!(snapshot.runtime.status, cadder_ipc::RuntimeStatus::Idle);
  assert!(!paths.effective_config_path().exists());
  assert!(history.records.is_empty());
  assert_eq!(state.inner.lock().await.sequence, 0);
  assert!(matches!(
    events.try_recv(),
    Err(broadcast::error::TryRecvError::Empty)
  ));
}

#[tokio::test]
async fn operation_fence_revoke_after_runtime_stop_restores_previous_registration() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let mut state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths.clone()));
  assert!(
    state
      .register(
        "register-active".to_string(),
        registration("shim-1", "nonce-1")
      )
      .await
      .accepted
  );
  let previous_config = fs::read(paths.effective_config_path()).unwrap();
  let sequence_before = state.inner.lock().await.sequence;
  let history_before = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "stop-rollback-history-before".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await
    .records
    .len();
  let mut events = state.subscribe();
  let hook = RegistrationPublishTestHook::new();
  state.registration_publish_hook = Some(hook.clone());
  let registering_state = state.clone();
  let fence = state.issue_operation_fence().unwrap();
  let pending_fence = fence.clone();

  let pending = tokio::spawn(async move {
    registering_state
      .unregister_fenced(
        "unregister-active".to_string(),
        "shim-1",
        "nonce-1",
        &pending_fence,
      )
      .await
  });
  hook.wait_until_reached().await;
  fence.revoke();
  hook.release();

  assert_eq!(
    pending.await.unwrap().unwrap_err(),
    CommitRejection::Revoked
  );
  let snapshot = state.snapshot().await;
  let history_after = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "stop-rollback-history-after".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await
    .records
    .len();

  assert_eq!(
    snapshot.registrations[0].activation_state,
    ActivationState::Active
  );
  assert_eq!(snapshot.runtime.status, cadder_ipc::RuntimeStatus::Running);
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Applied);
  assert_eq!(
    fs::read(paths.effective_config_path()).unwrap(),
    previous_config
  );
  assert_eq!(state.inner.lock().await.sequence, sequence_before);
  assert_eq!(history_after, history_before);
  assert!(matches!(
    events.try_recv(),
    Err(broadcast::error::TryRecvError::Empty)
  ));
}

#[tokio::test]
async fn operation_fence_revoke_after_unregister_reload_restores_all_registrations() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let mut state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths.clone()));
  assert!(
    state
      .register(
        "register-first".to_string(),
        registration("shim-1", "nonce-1")
      )
      .await
      .accepted
  );
  let mut second = registration("shim-2", "nonce-2");
  second.registered_domains = vec![RegisteredDomain::active("other.localhost")];
  assert!(
    state
      .register("register-second".to_string(), second)
      .await
      .accepted
  );
  let previous_config = fs::read(paths.effective_config_path()).unwrap();
  let sequence_before = state.inner.lock().await.sequence;
  let history_before = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "reload-rollback-history-before".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await
    .records
    .len();
  let mut events = state.subscribe();
  let hook = RegistrationPublishTestHook::new();
  state.registration_publish_hook = Some(hook.clone());
  let unregistering_state = state.clone();
  let fence = state.issue_operation_fence().unwrap();
  let pending_fence = fence.clone();

  let pending = tokio::spawn(async move {
    unregistering_state
      .unregister_fenced(
        "unregister-second".to_string(),
        "shim-2",
        "nonce-2",
        &pending_fence,
      )
      .await
  });
  hook.wait_until_reached().await;
  fence.revoke();
  hook.release();

  assert_eq!(
    pending.await.unwrap().unwrap_err(),
    CommitRejection::Revoked
  );
  let snapshot = state.snapshot().await;
  let history_after = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "reload-rollback-history-after".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await
    .records
    .len();

  assert_eq!(snapshot.registrations.len(), 2);
  assert_eq!(snapshot.runtime.status, cadder_ipc::RuntimeStatus::Running);
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Applied);
  assert_eq!(
    fs::read(paths.effective_config_path()).unwrap(),
    previous_config
  );
  assert_eq!(state.inner.lock().await.sequence, sequence_before);
  assert_eq!(history_after, history_before);
  assert!(matches!(
    events.try_recv(),
    Err(broadcast::error::TryRecvError::Empty)
  ));
}

#[tokio::test]
async fn revoked_operation_fence_rejects_missing_activation_targets() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths));
  let fence = state.issue_operation_fence().unwrap();
  fence.revoke();

  let entrypoint_result = state
    .set_entrypoint_enabled_fenced(
      SetEntrypointEnabledRequest {
        request_id: "missing-entrypoint".to_string(),
        registration_id: "missing".to_string(),
        shim_session_nonce: None,
        enabled: false,
      },
      &fence,
    )
    .await;
  let domain_result = state
    .set_domain_enabled_fenced(
      SetDomainEnabledRequest {
        request_id: "missing-domain".to_string(),
        registration_id: "missing".to_string(),
        domain_key: "missing.localhost".to_string(),
        enabled: false,
      },
      &fence,
    )
    .await;

  assert_eq!(entrypoint_result.unwrap_err(), CommitRejection::Revoked);
  assert_eq!(domain_result.unwrap_err(), CommitRejection::Revoked);
}

#[tokio::test]
async fn operation_fence_revoke_after_entrypoint_disable_restores_active_runtime() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let mut state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths.clone()));
  assert!(
    state
      .register(
        "register-active".to_string(),
        registration("shim-1", "nonce-1")
      )
      .await
      .accepted
  );
  let previous_config = fs::read(paths.effective_config_path()).unwrap();
  let sequence_before = state.inner.lock().await.sequence;
  let history_before = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "entrypoint-rollback-history-before".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await
    .records
    .len();
  let mut events = state.subscribe();
  let hook = RegistrationPublishTestHook::new();
  state.registration_publish_hook = Some(hook.clone());
  let toggling_state = state.clone();
  let fence = state.issue_operation_fence().unwrap();
  let pending_fence = fence.clone();

  let pending = tokio::spawn(async move {
    toggling_state
      .set_entrypoint_enabled_fenced(
        SetEntrypointEnabledRequest {
          request_id: "disable-entrypoint".to_string(),
          registration_id: "shim-1".to_string(),
          shim_session_nonce: Some("nonce-1".to_string()),
          enabled: false,
        },
        &pending_fence,
      )
      .await
  });
  hook.wait_until_reached().await;
  fence.revoke();
  hook.release();

  assert_eq!(
    pending.await.unwrap().unwrap_err(),
    CommitRejection::Revoked
  );
  let snapshot = state.snapshot().await;
  let history_after = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "entrypoint-rollback-history-after".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await
    .records
    .len();

  assert_eq!(
    snapshot.registrations[0].activation_state,
    ActivationState::Active
  );
  assert_eq!(snapshot.runtime.status, cadder_ipc::RuntimeStatus::Running);
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Applied);
  assert_eq!(
    fs::read(paths.effective_config_path()).unwrap(),
    previous_config
  );
  assert_eq!(state.inner.lock().await.sequence, sequence_before);
  assert_eq!(history_after, history_before);
  assert!(matches!(
    events.try_recv(),
    Err(broadcast::error::TryRecvError::Empty)
  ));
}

#[tokio::test]
async fn operation_fence_revoke_after_domain_disable_restores_active_domains() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let mut state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths.clone()));
  let mut registered = registration("shim-1", "nonce-1");
  registered.registered_domains = vec![
    RegisteredDomain::active("app.localhost"),
    RegisteredDomain::active("other.localhost"),
  ];
  assert!(
    state
      .register("register-domains".to_string(), registered)
      .await
      .accepted
  );
  let previous_config = fs::read(paths.effective_config_path()).unwrap();
  let sequence_before = state.inner.lock().await.sequence;
  let history_before = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "domain-rollback-history-before".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await
    .records
    .len();
  let mut events = state.subscribe();
  let hook = RegistrationPublishTestHook::new();
  state.registration_publish_hook = Some(hook.clone());
  let toggling_state = state.clone();
  let fence = state.issue_operation_fence().unwrap();
  let pending_fence = fence.clone();

  let pending = tokio::spawn(async move {
    toggling_state
      .set_domain_enabled_fenced(
        SetDomainEnabledRequest {
          request_id: "disable-domain".to_string(),
          registration_id: "shim-1".to_string(),
          domain_key: "app.localhost".to_string(),
          enabled: false,
        },
        &pending_fence,
      )
      .await
  });
  hook.wait_until_reached().await;
  fence.revoke();
  hook.release();

  assert_eq!(
    pending.await.unwrap().unwrap_err(),
    CommitRejection::Revoked
  );
  let snapshot = state.snapshot().await;
  let history_after = state
    .query_history(cadder_ipc::QueryHistoryRequest {
      request_id: "domain-rollback-history-after".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await
    .records
    .len();

  assert!(
    snapshot.registrations[0]
      .registered_domains
      .iter()
      .all(|domain| domain.activation_state == ActivationState::Active)
  );
  assert_eq!(snapshot.runtime.status, cadder_ipc::RuntimeStatus::Running);
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Applied);
  assert_eq!(
    fs::read(paths.effective_config_path()).unwrap(),
    previous_config
  );
  assert_eq!(state.inner.lock().await.sequence, sequence_before);
  assert_eq!(history_after, history_before);
  assert!(matches!(
    events.try_recv(),
    Err(broadcast::error::TryRecvError::Empty)
  ));
}

#[tokio::test]
async fn runtime_control_logs_are_active_and_limit_is_clamped() {
  let state = state();
  let stream = LogStreamIdentity::runtime_control();
  state.logs().append(
    stream.clone(),
    LogSeverity::Info,
    "first",
    LogAttributionKind::RuntimeControl,
    Some("start".to_string()),
  );
  state.logs().append(
    stream.clone(),
    LogSeverity::Warn,
    "second",
    LogAttributionKind::RuntimeControl,
    Some("reload".to_string()),
  );

  let response = state
    .query_logs(QueryLogsRequest {
      request_id: "logs".to_string(),
      stream: stream.clone(),
      limit: Some(0),
      cursor: Some("not-a-sequence".to_string()),
      minimum_severity: Some(LogSeverity::Info),
    })
    .await;

  assert_eq!(response.stream, stream);
  assert_eq!(response.stream_status, LogStreamStatus::Active);
  assert_eq!(response.entries.len(), 1);
  assert_eq!(response.entries[0].raw_message, "second");
  assert!(response.next_cursor.is_some());
}

#[tokio::test]
async fn shutdown_returns_success_when_runtime_is_idle() {
  let state = state();

  let response = state.shutdown().await;

  assert!(response.accepted);
  assert_eq!(response.message, "Daemon shutdown requested.");
}

#[tokio::test]
async fn shutdown_queue_failure_keeps_the_daemon_lifecycle_active_for_retry() {
  let mut state = state();
  let (store, release) = RuntimeStore::memory_stalled_for_test(0);
  state.store = store;

  let response = state.shutdown().await;

  assert!(!response.accepted);
  assert!(response.message.contains("could not queue"));
  assert!(state.issue_operation_fence().is_ok());
  release.send(()).unwrap();
  state.store.contain_shutdown().await.unwrap();
}
