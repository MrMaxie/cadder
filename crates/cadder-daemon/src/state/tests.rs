use super::iis_handoff::{
  active_registration_conflict, backend_dial, backend_port_for_binding, caddy_front_door_needed,
  disable_iis_steps, duplicate_iis_hosts, enable_iis_steps, follow_up_actions_for_issue,
  iis_failure, iis_route_host_conflicts, legacy_backend_binding, mark_privileged_batch_approved,
  mark_privileged_batch_issue, mark_step_issue, mark_step_skipped, mark_step_succeeded,
  route_host_for_binding,
};
use super::*;
use crate::{
  CaddyConfigAdapter, CaddyConfigCoordinator, ProcessRuntime, RealCaddyResolver, RuntimePaths,
};
use cadder_protocol::{
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
  let resolver = RealCaddyResolver::new(Some("definitely-missing-caddy".to_string()));
  let adapter = CaddyConfigAdapter::new(resolver.clone());
  let runtime = ProcessRuntime::new(resolver, paths);
  StateFixture {
    state: DaemonState::new(CaddyConfigCoordinator::new(adapter, runtime)),
    _temp: temp,
  }
}

fn state_with_iis(provider: IisProvider) -> StateFixture {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().to_path_buf())).unwrap();
  let resolver = RealCaddyResolver::new(Some("definitely-missing-caddy".to_string()));
  let adapter = CaddyConfigAdapter::new(resolver.clone());
  let runtime = ProcessRuntime::new(resolver, paths);
  StateFixture {
    state: DaemonState::with_iis_provider(CaddyConfigCoordinator::new(adapter, runtime), provider),
    _temp: temp,
  }
}

fn state_with_fake_caddy(provider: IisProvider, caddy: &Path) -> StateFixture {
  let (state, _) = state_with_fake_caddy_paths(provider, caddy);
  state
}

fn state_with_fake_caddy_paths(
  provider: IisProvider,
  caddy: &Path,
) -> (StateFixture, RuntimePaths) {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().to_path_buf())).unwrap();
  let resolver = RealCaddyResolver::new(Some(caddy.display().to_string()));
  let adapter = CaddyConfigAdapter::new(resolver.clone());
  let runtime = ProcessRuntime::new(resolver, paths.clone());
  (
    StateFixture {
      state: DaemonState::with_iis_provider(
        CaddyConfigCoordinator::new(adapter, runtime),
        provider,
      ),
      _temp: temp,
    },
    paths,
  )
}

fn iis_binding(site: &str, protocol: &str, binding: &str) -> IisBindingRecord {
  IisBindingRecord::from_binding_information(site, protocol, binding).unwrap()
}

fn tls_certificate() -> crate::iis::IisTlsCertificate {
  crate::iis::IisTlsCertificate {
    thumbprint: "aabbcc".to_string(),
    store_name: "My".to_string(),
    ssl_flags: Some(1),
  }
}

fn https_iis_binding(site: &str, binding: &str) -> IisBindingRecord {
  let mut binding = iis_binding(site, "https", binding);
  binding.tls_certificate = Some(tls_certificate());
  binding
}

fn write_fake_caddy(path: &Path) {
  #[cfg(windows)]
    fs::write(
      path,
      r#"@echo off
if "%1"=="adapt" (
  echo {"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["app.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}
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
    )
    .unwrap();

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::write(
        path,
        r#"#!/usr/bin/env sh
case "$1" in
  adapt)
    printf '%s\n' '{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["app.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}'
    exit 0
    ;;
  reload|stop)
    exit 0
    ;;
  run)
    sleep 5
    exit 0
    ;;
esac
exit 1
"#,
      )
      .unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
  }
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

#[test]
fn iis_and_step_helpers_cover_edge_branches() {
  let wildcard = iis_binding("Default Web Site", "http", "*:80:");
  let app = iis_binding("Default Web Site", "http", "*:80:app.localhost");
  let duplicate = iis_binding("Other Site", "http", "*:80:APP.localhost");
  assert_eq!(
    route_host_for_binding(&wildcard, Some("https://IIS.Localhost:443/path")).unwrap(),
    "iis.localhost"
  );
  assert_eq!(
    route_host_for_binding(&wildcard, None).unwrap_err().kind,
    IisIssueKind::MissingRoute
  );
  assert_eq!(
    route_host_for_binding(&wildcard, Some("bad host"))
      .unwrap_err()
      .kind,
    IisIssueKind::UnsupportedBindingShape
  );
  assert!(duplicate_iis_hosts(&[app.clone(), duplicate.clone()]).contains("app.localhost"));
  assert!(iis_route_host_conflicts(
    &[app.clone(), duplicate],
    &app.binding_id(),
    "app.localhost"
  ));

  let restore = IisRestoreRecord {
    binding: wildcard.clone(),
    domain_key: "iis.localhost".to_string(),
    registration_id: None,
    backend_binding: None,
  };
  let backend = legacy_backend_binding(&restore);
  assert_eq!(
    backend_dial(&backend),
    format!("127.0.0.1:{}", backend.port)
  );
  assert!(backend_port_for_binding(&wildcard) >= 41000);

  let mut registrations = BTreeMap::new();
  registrations.insert("shim-1".to_string(), registration("shim-1", "nonce-1"));
  assert!(active_registration_conflict(&registrations, "app.localhost").is_some());
  assert!(caddy_front_door_needed(&registrations, "iis.localhost"));
  assert!(!caddy_front_door_needed(&registrations, "app.localhost"));

  let mut steps = enable_iis_steps();
  mark_step_succeeded(&mut steps, "iis-discover-bindings");
  mark_step_issue(
    &mut steps,
    "iis-classify-binding",
    &IisIssue::new(IisIssueKind::IisUnavailable, "missing IIS"),
  );
  mark_step_issue(
    &mut steps,
    "iis-write-restore-metadata",
    &IisIssue::new(IisIssueKind::ElevationDenied, "denied"),
  );
  mark_step_issue(
    &mut steps,
    "caddy-apply-proxy-route",
    &IisIssue::new(IisIssueKind::ProviderError, "provider failed"),
  );
  mark_privileged_batch_approved(
    &mut steps,
    &["iis-create-loopback-binding", "iis-remove-public-binding"],
  );
  mark_privileged_batch_issue(
    &mut steps,
    &["iis-remove-public-binding"],
    &IisIssue::new(IisIssueKind::ElevationDenied, "denied"),
  );
  mark_privileged_batch_issue(
    &mut steps,
    &["iis-create-loopback-binding"],
    &IisIssue::new(IisIssueKind::ElevationUnsupported, "unsupported"),
  );
  mark_step_skipped(&mut steps, "caddy-apply-proxy-route");
  assert!(steps.iter().any(|step| {
    step.step_id == "iis-classify-binding" && step.status == IisOperationStepStatus::Unsupported
  }));
  assert!(steps.iter().any(|step| {
    step.step_id == "iis-write-restore-metadata" && step.status == IisOperationStepStatus::Denied
  }));
  assert!(steps.iter().any(|step| {
    step.step_id == "iis-remove-public-binding" && step.approval == IisElevationApproval::Denied
  }));
  assert!(steps.iter().any(|step| {
    step.step_id == "iis-create-loopback-binding"
      && step.approval == IisElevationApproval::Unsupported
  }));
  assert_eq!(
    follow_up_actions_for_issue(&IisIssue::new(IisIssueKind::ElevationDenied, "denied")),
    vec![IisFollowUpAction::RetryElevation]
  );
  assert_eq!(
    follow_up_actions_for_issue(&IisIssue::new(
      IisIssueKind::ElevationUnsupported,
      "unsupported"
    )),
    Vec::<IisFollowUpAction>::new()
  );
  assert_eq!(
    follow_up_actions_for_issue(&IisIssue::new(IisIssueKind::ProviderError, "provider")),
    vec![IisFollowUpAction::RetryElevation]
  );
  assert_eq!(
    follow_up_actions_for_issue(&IisIssue::new(IisIssueKind::RollbackFailed, "rollback")),
    vec![
      IisFollowUpAction::RollbackHandoff,
      IisFollowUpAction::RetryElevation
    ]
  );
  assert_eq!(
    follow_up_actions_for_issue(&IisIssue::new(IisIssueKind::Conflict, "conflict")),
    Vec::<IisFollowUpAction>::new()
  );
  assert_eq!(
    follow_up_actions_for_issue(&IisIssue::new(IisIssueKind::RestoreFailed, "restore")),
    vec![
      IisFollowUpAction::RetryRestore,
      IisFollowUpAction::RetryElevation
    ]
  );
  let response = iis_failure(
    "iis".to_string(),
    IisIssue::new(IisIssueKind::ProviderError, "failed"),
    None,
    Vec::new(),
  );
  assert_eq!(response.message, "failed");
  assert!(
    disable_iis_steps()
      .iter()
      .any(|step| step.step_id == "iis-clear-restore-metadata")
  );
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
    .query_history(cadder_protocol::QueryHistoryRequest {
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
async fn query_iis_bindings_reports_safety_states_from_fake_provider() {
  let provider = IisProvider::fake(vec![
    iis_binding("Default Web Site", "http", "*:80:app.localhost"),
    iis_binding("Default Web Site", "http", "127.0.0.1:80:app.localhost"),
    https_iis_binding("Default Web Site", "*:443:secure.localhost"),
    iis_binding("Other", "http", "*:80:missing.localhost"),
    iis_binding("Default Web Site", "https", "*:443:"),
  ]);
  let state = state_with_iis(provider);
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;

  let response = state.query_iis_bindings("iis".to_string()).await;

  assert!(response.accepted);
  assert_eq!(response.bindings.len(), 5);
  assert_eq!(
    response.bindings[0].handoff_state,
    IisHandoffState::Conflict
  );
  assert_eq!(
    response.bindings[2].handoff_state,
    IisHandoffState::Available
  );
  assert_eq!(
    response.bindings[3].handoff_state,
    IisHandoffState::Available
  );
  assert_eq!(
    response.bindings[4].handoff_state,
    IisHandoffState::MissingRoute
  );
}

#[tokio::test]
async fn query_iis_bindings_reports_provider_discovery_failure() {
  let provider = IisProvider::fake(Vec::new());
  provider
    .set_fail_discovery(IisIssue::new(
      IisIssueKind::IisUnavailable,
      "IIS discovery failed.",
    ))
    .await;
  let state = state_with_iis(provider);

  let response = state.query_iis_bindings("iis".to_string()).await;

  assert!(!response.accepted);
  assert_eq!(response.message, "IIS discovery failed.");
  assert!(response.bindings.is_empty());
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::IisUnavailable)
  );
}

#[tokio::test]
async fn query_iis_bindings_includes_orphaned_restore_metadata_as_handed_off() {
  let binding = iis_binding("Default Web Site", "http", "*:80:app.localhost");
  let restore = IisRestoreRecord {
    binding: binding.clone(),
    domain_key: "app.localhost".to_string(),
    registration_id: None,
    backend_binding: Some(binding.backend_binding(41080, "app.localhost").unwrap()),
  };
  let state = state_with_iis(IisProvider::fake(Vec::new()));
  state
    .iis_store
    .insert(binding.binding_id(), restore)
    .await
    .unwrap();

  let response = state.query_iis_bindings("iis".to_string()).await;

  assert!(response.accepted);
  assert_eq!(response.bindings.len(), 1);
  assert_eq!(
    response.bindings[0].handoff_state,
    IisHandoffState::HandedOff
  );
  assert_eq!(
    response.bindings[0].domain_key.as_deref(),
    Some("app.localhost")
  );
}

#[tokio::test]
async fn iis_handoff_proxies_wildcard_https_binding_with_route_host() {
  let temp = tempfile::tempdir().unwrap();
  let caddy = temp.path().join(if cfg!(windows) {
    "fake-caddy.cmd"
  } else {
    "fake-caddy"
  });
  write_fake_caddy(&caddy);
  let binding = https_iis_binding("Default Web Site", "*:443:");
  let provider = IisProvider::fake(vec![binding.clone()]);
  let (state, paths) = state_with_fake_caddy_paths(provider, &caddy);
  let backend_port = backend_port_for_binding(&binding);
  let expected_backend_dial = format!("127.0.0.1:{backend_port}");
  assert!((41000..49000).contains(&backend_port));

  let enabled = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: Some("https://iis-app.localhost/legacy/status".to_string()),
    })
    .await;
  let handed_off = state.query_iis_bindings("iis-handed-off".to_string()).await;
  let rendered = fs::read_to_string(paths.effective_config_path()).unwrap();
  let config: serde_json::Value = serde_json::from_str(&rendered).unwrap();
  let routes = config
    .pointer("/apps/http/servers/cadder_https/routes")
    .and_then(serde_json::Value::as_array)
    .unwrap();
  let iis_route = routes
    .iter()
    .find(|route| {
      route
        .pointer("/match/0/host/0")
        .and_then(serde_json::Value::as_str)
        == Some("iis-app.localhost")
    })
    .unwrap();
  let _ = state.shutdown().await;

  assert!(enabled.accepted, "{enabled:?}");
  assert_eq!(
    enabled
      .binding
      .as_ref()
      .and_then(|binding| binding.domain_key.as_deref()),
    Some("iis-app.localhost")
  );
  assert_eq!(handed_off.bindings.len(), 1);
  assert_eq!(
    handed_off.bindings[0].identity.binding_id,
    "Default Web Site|https|*:443:"
  );
  assert_eq!(
    handed_off.bindings[0].handoff_state,
    IisHandoffState::HandedOff
  );
  assert_eq!(
    handed_off.bindings[0].domain_key.as_deref(),
    Some("iis-app.localhost")
  );
  assert_eq!(
    iis_route
      .pointer("/handle/0/upstreams/0/dial")
      .and_then(serde_json::Value::as_str),
    Some(expected_backend_dial.as_str())
  );
  assert_eq!(
    iis_route
      .pointer("/handle/0/transport/tls/server_name")
      .and_then(serde_json::Value::as_str),
    Some("iis-app.localhost")
  );
  assert_eq!(
    iis_route
      .pointer("/handle/0/transport/tls/insecure_skip_verify")
      .and_then(serde_json::Value::as_bool),
    Some(true)
  );
}

#[tokio::test]
async fn iis_handoff_rejects_https_binding_without_certificate_before_mutation() {
  let binding = iis_binding("Default Web Site", "https", "*:443:secure.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  let state = state_with_iis(provider.clone());

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let rediscovered = provider.discover().await.unwrap();

  assert!(!response.accepted);
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::MissingTlsCertificate)
  );
  assert!(response.steps.iter().any(|step| {
    step.step_id == "iis-classify-binding" && step.status == IisOperationStepStatus::Failed
  }));
  assert_eq!(rediscovered, vec![binding]);
  assert!(state.iis_store.snapshot().await.is_empty());
}

#[tokio::test]
async fn iis_handoff_rejects_explicit_route_host_used_by_another_binding() {
  let selected = iis_binding("Default Web Site", "https", "*:443:");
  let existing = iis_binding("Default Web Site", "https", "*:443:iis-app.localhost");
  let provider = IisProvider::fake(vec![selected.clone(), existing]);
  let state = state_with_iis(provider);

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: selected.binding_id(),
      enabled: true,
      route_host: Some("iis-app.localhost".to_string()),
    })
    .await;

  assert!(!response.accepted);
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::Conflict)
  );
}

#[tokio::test]
async fn iis_handoff_rejects_invalid_explicit_route_host_before_mutation() {
  let binding = iis_binding("Default Web Site", "http", "*:80:");
  let provider = IisProvider::fake(vec![binding.clone()]);
  let state = state_with_iis(provider.clone());

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: Some("bad host".to_string()),
    })
    .await;
  let rediscovered = provider.discover().await.unwrap();

  assert!(!response.accepted);
  assert_eq!(
    response
      .binding
      .as_ref()
      .map(|binding| binding.handoff_state),
    Some(IisHandoffState::MissingRoute)
  );
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::UnsupportedBindingShape)
  );
  assert!(response.steps.iter().any(|step| {
    step.step_id == "iis-classify-binding" && step.status == IisOperationStepStatus::Failed
  }));
  assert_eq!(rediscovered, vec![binding]);
}

#[tokio::test]
async fn iis_handoff_reports_metadata_write_failure_before_mutating_iis() {
  let temp = tempfile::tempdir().unwrap();
  let blocked_parent = temp.path().join("metadata-parent");
  fs::write(&blocked_parent, "not a directory").unwrap();
  let binding = iis_binding("Default Web Site", "http", "*:80:iis.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  let mut state = state_with_iis(provider.clone());
  state.iis_store = IisMetadataStore::load(blocked_parent.join("iis.json"))
    .await
    .unwrap();

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let rediscovered = provider.discover().await.unwrap();

  assert!(!response.accepted);
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::ProviderError)
  );
  assert!(response.message.contains("restore metadata"));
  assert!(response.steps.iter().any(|step| {
    step.step_id == "iis-write-restore-metadata" && step.status == IisOperationStepStatus::Failed
  }));
  assert_eq!(rediscovered, vec![binding]);
}

#[tokio::test]
async fn set_autostart_reports_unsupported_missing_targets_and_records_history() {
  let state = state();

  let response = state
    .set_autostart(SetAutostartRequest {
      request_id: "autostart-set".to_string(),
      mode: AutostartMode::Daemon,
    })
    .await;
  let history = state
    .query_history(cadder_protocol::QueryHistoryRequest {
      request_id: "history".to_string(),
      kind: Some(HistoryKind::Autostart),
      limit: Some(10),
    })
    .await;

  assert!(!response.accepted);
  assert_eq!(response.status, AutostartStatus::Unsupported);
  assert!(response.target.is_none());
  assert!(response.diagnostics.iter().any(|diagnostic| {
    diagnostic.code == "autostart-update-failed"
      && diagnostic.message.contains("cadderd executable")
  }));
  assert_eq!(history.records.len(), 1);
  assert_eq!(history.records[0].kind, HistoryKind::Autostart);
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
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
async fn iis_handoff_rejects_missing_binding_before_steps() {
  let state = state_with_iis(IisProvider::fake(Vec::new()));

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: "missing|http|*:80:app.localhost".to_string(),
      enabled: true,
      route_host: None,
    })
    .await;

  assert!(!response.accepted);
  assert!(response.binding.is_none());
  assert!(response.steps.is_empty());
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::MissingBinding)
  );
}

#[tokio::test]
async fn iis_handoff_reports_provider_discovery_failure() {
  let provider = IisProvider::fake(Vec::new());
  provider
    .set_fail_discovery(IisIssue::new(
      IisIssueKind::IisUnavailable,
      "IIS discovery failed.",
    ))
    .await;
  let state = state_with_iis(provider);

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: "Default Web Site|http|*:80:app.localhost".to_string(),
      enabled: true,
      route_host: None,
    })
    .await;

  assert!(!response.accepted);
  assert!(response.binding.is_none());
  assert!(response.steps.is_empty());
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::IisUnavailable)
  );
}

#[tokio::test]
async fn iis_handoff_rejects_unsupported_binding_with_failed_classification_step() {
  let binding = iis_binding("Default Web Site", "ftp", "*:21:files.localhost");
  let state = state_with_iis(IisProvider::fake(vec![binding.clone()]));

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;

  assert!(!response.accepted);
  assert_eq!(
    response
      .binding
      .as_ref()
      .map(|binding| binding.handoff_state),
    Some(IisHandoffState::Unsupported)
  );
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::UnsupportedBindingShape)
  );
  assert!(response.steps.iter().any(|step| {
    step.step_id == "iis-classify-binding" && step.status == IisOperationStepStatus::Failed
  }));
}

#[tokio::test]
async fn iis_handoff_rejects_wildcard_binding_without_route_host() {
  let binding = iis_binding("Default Web Site", "http", "*:80:");
  let state = state_with_iis(IisProvider::fake(vec![binding.clone()]));

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;

  assert!(!response.accepted);
  assert_eq!(
    response
      .binding
      .as_ref()
      .map(|binding| binding.handoff_state),
    Some(IisHandoffState::MissingRoute)
  );
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::MissingRoute)
  );
}

#[tokio::test]
async fn iis_handoff_rejects_binding_when_active_registration_owns_route() {
  let temp = tempfile::tempdir().unwrap();
  let fake_caddy = fake_caddy_path(temp.path());
  write_fake_caddy(&fake_caddy);
  let binding = iis_binding("Default Web Site", "http", "*:80:app.localhost");
  let state = state_with_fake_caddy(IisProvider::fake(vec![binding.clone()]), &fake_caddy);
  let registered = state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  assert!(registered.accepted, "{registered:?}");

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let _ = state.shutdown().await;

  assert!(!response.accepted);
  assert_eq!(
    response
      .binding
      .as_ref()
      .map(|binding| binding.handoff_state),
    Some(IisHandoffState::Conflict)
  );
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::Conflict)
  );
  assert!(response.message.contains("already has an active route"));
}

#[tokio::test]
async fn iis_restore_rejects_missing_restore_metadata() {
  let state = state_with_iis(IisProvider::fake(Vec::new()));

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-off".to_string(),
      binding_id: "missing|http|*:80:app.localhost".to_string(),
      enabled: false,
      route_host: None,
    })
    .await;

  assert!(!response.accepted);
  assert!(response.binding.is_none());
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::MissingBinding)
  );
  assert!(response.steps.iter().any(|step| {
    step.step_id == "iis-read-restore-metadata" && step.status == IisOperationStepStatus::Failed
  }));
}

#[tokio::test]
async fn runtime_paths_hydrate_iis_proxy_routes_from_metadata() {
  let temp = tempfile::tempdir().unwrap();
  let caddy = temp.path().join(if cfg!(windows) {
    "fake-caddy.cmd"
  } else {
    "fake-caddy"
  });
  write_fake_caddy(&caddy);
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  paths.ensure_dirs().unwrap();
  let binding = https_iis_binding("Default Web Site", "*:443:");
  let domain_key = "iis-app.localhost".to_string();
  let backend_binding = binding
    .backend_binding(backend_port_for_binding(&binding), &domain_key)
    .unwrap();
  let store = IisMetadataStore::load(paths.metadata_path()).await.unwrap();
  store
    .insert(
      binding.binding_id(),
      IisRestoreRecord {
        binding: binding.clone(),
        domain_key: domain_key.clone(),
        registration_id: None,
        backend_binding: Some(backend_binding.clone()),
      },
    )
    .await
    .unwrap();
  let resolver = RealCaddyResolver::new(Some(caddy.display().to_string()));
  let adapter = CaddyConfigAdapter::new(resolver.clone());
  let runtime = ProcessRuntime::new(resolver, paths.clone());
  let state =
    DaemonState::with_runtime_paths(CaddyConfigCoordinator::new(adapter, runtime), paths.clone())
      .await
      .unwrap();

  let rendered = fs::read_to_string(paths.effective_config_path()).unwrap();
  let _ = state.shutdown().await;

  assert!(rendered.contains("iis-app.localhost"));
  assert!(rendered.contains(&format!("127.0.0.1:{}", backend_binding.port)));
  assert!(rendered.contains("\"server_name\": \"iis-app.localhost\""));
}

#[tokio::test]
async fn iis_handoff_rolls_back_when_caddy_apply_fails() {
  let binding = iis_binding("Default Web Site", "http", "*:80:iis.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  let state = state_with_iis(provider);
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let rediscovered = state.query_iis_bindings("iis".to_string()).await;

  assert!(!response.accepted);
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::RollbackSucceeded)
  );
  assert_eq!(rediscovered.bindings.len(), 1);
  assert_eq!(
    rediscovered.bindings[0].handoff_state,
    IisHandoffState::Available
  );
  assert!(state.iis_store.snapshot().await.is_empty());
}

#[tokio::test]
async fn iis_handoff_keeps_restore_metadata_when_caddy_apply_and_rollback_fail() {
  let binding = iis_binding("Default Web Site", "http", "*:80:iis.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  provider
    .set_fail_restore(IisIssue::new(IisIssueKind::ProviderError, "restore denied"))
    .await;
  let state = state_with_iis(provider);
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let rediscovered = state.query_iis_bindings("iis".to_string()).await;
  let handoffs = state.iis_store.snapshot().await;

  assert!(!response.accepted);
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::RollbackFailed)
  );
  assert!(handoffs.contains_key(&binding.binding_id()));
  assert_eq!(rediscovered.bindings.len(), 1);
  assert_eq!(
    rediscovered.bindings[0].handoff_state,
    IisHandoffState::HandedOff
  );
  assert_eq!(
    rediscovered.bindings[0].domain_key.as_deref(),
    Some("iis.localhost")
  );
}

#[tokio::test]
async fn iis_handoff_reports_busy_when_another_operation_is_running() {
  let binding = iis_binding("Default Web Site", "http", "*:80:iis.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  let state = state_with_iis(provider);
  let _operation = state.iis_operation.try_lock().unwrap();

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;

  assert!(!response.accepted);
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::Busy)
  );
}

#[tokio::test]
async fn iis_handoff_success_and_restore_use_fake_provider() {
  let temp = tempfile::tempdir().unwrap();
  let caddy = temp.path().join(if cfg!(windows) {
    "fake-caddy.cmd"
  } else {
    "fake-caddy"
  });
  write_fake_caddy(&caddy);
  let binding = iis_binding("Default Web Site", "http", "*:80:app.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  let state = state_with_fake_caddy(provider, &caddy);
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  state
    .set_domain_enabled(SetDomainEnabledRequest {
      request_id: "disable".to_string(),
      registration_id: "shim-1".to_string(),
      domain_key: "app.localhost".to_string(),
      enabled: false,
    })
    .await;

  let enabled = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let handed_off = state.query_iis_bindings("iis-handed-off".to_string()).await;
  let restored = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-off".to_string(),
      binding_id: binding.binding_id(),
      enabled: false,
      route_host: None,
    })
    .await;
  let rediscovered = state.query_iis_bindings("iis".to_string()).await;
  let _ = state.shutdown().await;

  assert!(enabled.accepted, "{enabled:?}");
  assert_eq!(
    enabled
      .binding
      .as_ref()
      .map(|binding| binding.handoff_state),
    Some(IisHandoffState::HandedOff)
  );
  assert!(enabled.steps.iter().any(|step| {
    step.step_id == "iis-remove-public-binding"
      && step.approval == IisElevationApproval::Approved
      && step.status == IisOperationStepStatus::Succeeded
  }));
  assert_eq!(handed_off.bindings.len(), 1);
  assert_eq!(
    handed_off.bindings[0].handoff_state,
    IisHandoffState::HandedOff
  );
  assert!(restored.accepted, "{restored:?}");
  assert_eq!(rediscovered.bindings.len(), 1);
  assert_eq!(
    rediscovered.bindings[0].handoff_state,
    IisHandoffState::Available
  );
}

#[tokio::test]
async fn iis_https_handoff_success_and_restore_use_https_backend_binding() {
  let temp = tempfile::tempdir().unwrap();
  let caddy = temp.path().join(if cfg!(windows) {
    "fake-caddy.cmd"
  } else {
    "fake-caddy"
  });
  write_fake_caddy(&caddy);
  let binding = https_iis_binding("Default Web Site", "*:443:secure.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  let state = state_with_fake_caddy(provider.clone(), &caddy);

  let enabled = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let after_enable = provider.discover().await.unwrap();
  let restored = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-off".to_string(),
      binding_id: binding.binding_id(),
      enabled: false,
      route_host: None,
    })
    .await;
  let after_restore = provider.discover().await.unwrap();
  let _ = state.shutdown().await;

  assert!(enabled.accepted, "{enabled:?}");
  assert_eq!(after_enable.len(), 1);
  assert_eq!(after_enable[0].protocol, "https");
  assert_eq!(after_enable[0].ip_address, "127.0.0.1");
  assert_eq!(after_enable[0].host_header, "secure.localhost");
  assert_eq!(after_enable[0].tls_certificate, Some(tls_certificate()));
  assert!(restored.accepted, "{restored:?}");
  assert_eq!(after_restore, vec![binding]);
}

#[tokio::test]
async fn iis_handoff_denied_by_elevation_keeps_user_level_state_usable() {
  let binding = iis_binding("Default Web Site", "http", "*:80:app.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  provider
    .set_elevation_issue(IisIssue::new(
      IisIssueKind::ElevationDenied,
      "Administrator approval was denied.",
    ))
    .await;
  let state = state_with_iis(provider);

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let rediscovered = state.query_iis_bindings("iis".to_string()).await;

  assert!(!response.accepted);
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::ElevationDenied)
  );
  assert!(response.steps.iter().any(|step| {
    step.step_id == "iis-remove-public-binding"
      && step.approval == IisElevationApproval::Denied
      && step.status == IisOperationStepStatus::Denied
  }));
  assert_eq!(
    response.follow_up_actions,
    vec![IisFollowUpAction::RetryElevation]
  );
  assert!(state.iis_store.snapshot().await.is_empty());
  assert_eq!(rediscovered.bindings.len(), 1);
  assert_eq!(
    rediscovered.bindings[0].handoff_state,
    IisHandoffState::Available
  );
}

#[tokio::test]
async fn iis_handoff_reports_unsupported_elevation_without_mutating_iis() {
  let binding = https_iis_binding("Default Web Site", "*:443:secure.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  provider
    .set_elevation_issue(IisIssue::new(
      IisIssueKind::ElevationUnsupported,
      "IIS elevation prompts are only available on Windows.",
    ))
    .await;
  let state = state_with_iis(provider);

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let rediscovered = state.query_iis_bindings("iis".to_string()).await;

  assert!(!response.accepted);
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::ElevationUnsupported)
  );
  assert!(response.follow_up_actions.is_empty());
  assert!(response.steps.iter().any(|step| {
    step.step_id == "iis-create-loopback-binding"
      && step.approval == IisElevationApproval::Unsupported
      && step.status == IisOperationStepStatus::Unsupported
  }));
  assert!(state.iis_store.snapshot().await.is_empty());
  assert_eq!(rediscovered.bindings.len(), 1);
  assert_eq!(
    rediscovered.bindings[0].handoff_state,
    IisHandoffState::Available
  );
}

#[tokio::test]
async fn iis_handoff_provider_error_requests_loopback_cleanup_follow_up() {
  let binding = iis_binding("Default Web Site", "http", "*:80:app.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  provider
    .set_elevation_issue(IisIssue::new(
      IisIssueKind::ProviderError,
      "privileged mutation failed",
    ))
    .await;
  let state = state_with_iis(provider);

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let handoffs = state.iis_store.snapshot().await;

  assert!(!response.accepted);
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::ProviderError)
  );
  assert!(response.steps.iter().any(|step| {
    step.step_id == "iis-create-loopback-binding"
      && step.approval == IisElevationApproval::Approved
      && step.status == IisOperationStepStatus::Failed
  }));
  assert_eq!(
    response.follow_up_actions,
    vec![
      IisFollowUpAction::RetryElevation,
      IisFollowUpAction::RemoveLoopbackBinding
    ]
  );
  assert!(handoffs.is_empty());
}

#[tokio::test]
async fn iis_handoff_partial_privileged_batch_exposes_loopback_cleanup_follow_up() {
  let binding = iis_binding("Default Web Site", "http", "*:80:app.localhost");
  let backend_binding =
    binding.backend_http_binding(backend_port_for_binding(&binding), "app.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  provider
    .set_fail_remove(IisIssue::new(
      IisIssueKind::ProviderError,
      "public binding removal failed",
    ))
    .await;
  let state = state_with_iis(provider.clone());

  let response = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let discovered = provider.discover().await.unwrap();
  let handoffs = state.iis_store.snapshot().await;

  assert!(!response.accepted);
  assert_eq!(
    response.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::ProviderError)
  );
  assert!(response.steps.iter().any(|step| {
    step.step_id == "iis-create-loopback-binding"
      && step.approval == IisElevationApproval::Approved
      && step.status == IisOperationStepStatus::Failed
  }));
  assert!(response.steps.iter().any(|step| {
    step.step_id == "iis-remove-public-binding"
      && step.approval == IisElevationApproval::Approved
      && step.status == IisOperationStepStatus::Failed
  }));
  assert_eq!(
    response.follow_up_actions,
    vec![
      IisFollowUpAction::RetryElevation,
      IisFollowUpAction::RemoveLoopbackBinding
    ]
  );
  assert!(handoffs.is_empty());
  assert!(
    discovered
      .iter()
      .any(|candidate| candidate.binding_id() == binding.binding_id())
  );
  assert!(
    discovered
      .iter()
      .any(|candidate| { candidate.binding_id() == backend_binding.binding_id() })
  );
}

#[tokio::test]
async fn iis_restore_rejects_when_other_cadder_routes_need_front_door() {
  let temp = tempfile::tempdir().unwrap();
  let caddy = fake_caddy_path(temp.path());
  write_fake_caddy(&caddy);
  let binding = iis_binding("Default Web Site", "http", "*:80:iis.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  let state = state_with_fake_caddy(provider, &caddy);
  let registered = state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  assert!(registered.accepted, "{registered:?}");

  let enabled = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  let restored = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-off".to_string(),
      binding_id: binding.binding_id(),
      enabled: false,
      route_host: None,
    })
    .await;
  let _ = state.shutdown().await;

  assert!(enabled.accepted, "{enabled:?}");
  assert!(!restored.accepted);
  assert_eq!(
    restored.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::Conflict)
  );
  assert!(
    restored
      .message
      .contains("other Cadder routes still need the front-door port")
  );
  assert!(restored.steps.iter().any(|step| {
    step.step_id == "caddy-remove-proxy-route" && step.status == IisOperationStepStatus::Failed
  }));
}

#[tokio::test]
async fn iis_restore_keeps_metadata_when_backend_cleanup_fails() {
  let temp = tempfile::tempdir().unwrap();
  let caddy = temp.path().join(if cfg!(windows) {
    "fake-caddy.cmd"
  } else {
    "fake-caddy"
  });
  write_fake_caddy(&caddy);
  let binding = iis_binding("Default Web Site", "http", "*:80:app.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  let state = state_with_fake_caddy(provider.clone(), &caddy);
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  state
    .set_domain_enabled(SetDomainEnabledRequest {
      request_id: "disable".to_string(),
      registration_id: "shim-1".to_string(),
      domain_key: "app.localhost".to_string(),
      enabled: false,
    })
    .await;

  let enabled = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  provider
    .set_fail_remove(IisIssue::new(IisIssueKind::ProviderError, "cleanup failed"))
    .await;
  let restored = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-off".to_string(),
      binding_id: binding.binding_id(),
      enabled: false,
      route_host: None,
    })
    .await;
  let handoffs = state.iis_store.snapshot().await;
  let _ = state.shutdown().await;

  assert!(enabled.accepted, "{enabled:?}");
  assert!(!restored.accepted);
  assert_eq!(
    restored.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::RestoreFailed)
  );
  assert!(handoffs.contains_key(&binding.binding_id()));
}

#[tokio::test]
async fn iis_restore_reports_metadata_clear_failure_after_iis_restore() {
  let temp = tempfile::tempdir().unwrap();
  let caddy = fake_caddy_path(temp.path());
  write_fake_caddy(&caddy);
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  paths.ensure_dirs().unwrap();
  let resolver = RealCaddyResolver::new(Some(caddy.display().to_string()));
  let adapter = CaddyConfigAdapter::new(resolver.clone());
  let runtime = ProcessRuntime::new(resolver, paths.clone());
  let binding = iis_binding("Default Web Site", "http", "*:80:app.localhost");
  let provider = IisProvider::fake(vec![binding.clone()]);
  let mut state =
    DaemonState::with_runtime_paths(CaddyConfigCoordinator::new(adapter, runtime), paths.clone())
      .await
      .unwrap();
  state.iis_provider = provider;
  state
    .register("register".to_string(), registration("shim-1", "nonce-1"))
    .await;
  state
    .set_domain_enabled(SetDomainEnabledRequest {
      request_id: "disable".to_string(),
      registration_id: "shim-1".to_string(),
      domain_key: "app.localhost".to_string(),
      enabled: false,
    })
    .await;
  let enabled = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: binding.binding_id(),
      enabled: true,
      route_host: None,
    })
    .await;
  assert!(enabled.accepted, "{enabled:?}");
  let metadata_path = paths.metadata_path();
  let original_permissions = fs::metadata(&metadata_path).unwrap().permissions();
  let mut readonly_permissions = original_permissions.clone();
  readonly_permissions.set_readonly(true);
  fs::set_permissions(&metadata_path, readonly_permissions).unwrap();

  let restored = state
    .set_iis_handoff(SetIisHandoffRequest {
      request_id: "iis-off".to_string(),
      binding_id: binding.binding_id(),
      enabled: false,
      route_host: None,
    })
    .await;
  let _ = fs::set_permissions(&metadata_path, original_permissions);
  let _ = state.shutdown().await;

  assert!(!restored.accepted);
  assert_eq!(
    restored.issue.as_ref().map(|issue| issue.kind),
    Some(IisIssueKind::ProviderError)
  );
  assert_eq!(
    restored.follow_up_actions,
    vec![IisFollowUpAction::ClearRestoreMetadata]
  );
  assert!(restored.steps.iter().any(|step| {
    step.step_id == "iis-clear-restore-metadata" && step.status == IisOperationStepStatus::Failed
  }));
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
    cadder_protocol::RuntimeStatus::Running
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
  assert_eq!(
    snapshot.runtime.status,
    cadder_protocol::RuntimeStatus::Idle
  );
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
    Some("sqlite".to_string())
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
    .query_history(cadder_protocol::QueryHistoryRequest {
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
    Some("sqlite")
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
    state.query_history(cadder_protocol::QueryHistoryRequest {
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
async fn register_revalidates_existing_owner_after_slow_adapt() {
  let temp = tempfile::tempdir().unwrap();
  let fake_caddy = fake_caddy_path(temp.path());
  let marker = temp.path().join("adapt-started");
  write_slow_adapt_fake_caddy(&fake_caddy, &marker);
  let state = state_with_fake_caddy(IisProvider::fake(Vec::new()), &fake_caddy);
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
  let state = state_with_fake_caddy(IisProvider::fake(Vec::new()), &fake_caddy);
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
    .query_history(cadder_protocol::QueryHistoryRequest {
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
  let (mut state, paths) = state_with_fake_caddy_paths(IisProvider::fake(Vec::new()), &fake_caddy);
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
    .query_history(cadder_protocol::QueryHistoryRequest {
      request_id: "operation-fence-after-apply-history".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await;

  assert_eq!(result.unwrap_err(), CommitRejection::Revoked);
  assert!(snapshot.registrations.is_empty());
  assert_eq!(
    snapshot.runtime.status,
    cadder_protocol::RuntimeStatus::Idle
  );
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
    .query_history(cadder_protocol::QueryHistoryRequest {
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
    .query_history(cadder_protocol::QueryHistoryRequest {
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
  assert_eq!(
    snapshot.runtime.status,
    cadder_protocol::RuntimeStatus::Running
  );
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
    .query_history(cadder_protocol::QueryHistoryRequest {
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
    .query_history(cadder_protocol::QueryHistoryRequest {
      request_id: "reload-rollback-history-after".to_string(),
      kind: None,
      limit: Some(10),
    })
    .await
    .records
    .len();

  assert_eq!(snapshot.registrations.len(), 2);
  assert_eq!(
    snapshot.runtime.status,
    cadder_protocol::RuntimeStatus::Running
  );
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
    .query_history(cadder_protocol::QueryHistoryRequest {
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
    .query_history(cadder_protocol::QueryHistoryRequest {
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
  assert_eq!(
    snapshot.runtime.status,
    cadder_protocol::RuntimeStatus::Running
  );
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
    .query_history(cadder_protocol::QueryHistoryRequest {
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
    .query_history(cadder_protocol::QueryHistoryRequest {
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
  assert_eq!(
    snapshot.runtime.status,
    cadder_protocol::RuntimeStatus::Running
  );
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
