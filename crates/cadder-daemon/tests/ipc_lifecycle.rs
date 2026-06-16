use cadder_daemon::{
  CadderClient, CadderSession, CaddyConfigAdapter, CaddyConfigCoordinator, DaemonServer,
  DaemonState, ProcessRuntime, RealCaddyResolver, RuntimePaths,
};
use cadder_protocol::{
  ActivationState, BasicResponse, ConfigApplyStatus, EntrypointInstanceIdentity,
  EntrypointRegistration, HeartbeatEntrypointRequest, LogAttributionKind, LogSeverity,
  LogStreamIdentity, LogStreamStatus, OwnerProcessIdentity, QueryLogsRequest, QueryLogsResponse,
  QueryStateRequest, QueryStateResponse, RegisterEntrypointRequest, RegisterEntrypointResponse,
  SetDomainEnabledRequest, SetEntrypointEnabledRequest, ShimRunMetadata, SourcePath,
  StateChangeKind, UnregisterEntrypointRequest, message_types, new_request_id,
};
use chrono::Utc;
use std::{
  fs,
  path::{Path, PathBuf},
  time::Duration,
};
use tokio::{sync::watch, time::sleep};

#[tokio::test]
async fn ipc_lifecycle_starts_with_zero_registrations() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;

  let snapshot = query_state(&harness.client).await;

  assert!(snapshot.registrations.is_empty());
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Idle);
  harness.shutdown().await;
}

#[tokio::test]
async fn ipc_state_subscriptions_broadcast_updates_to_multiple_dashboard_clients() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let mut first = harness
    .client
    .subscribe_state(new_request_id("dashboard-1"))
    .await
    .unwrap();
  let mut second = harness
    .client
    .subscribe_state(new_request_id("dashboard-2"))
    .await
    .unwrap();

  let first_initial = tokio::time::timeout(Duration::from_secs(1), first.next_event())
    .await
    .unwrap()
    .unwrap();
  let second_initial = tokio::time::timeout(Duration::from_secs(1), second.next_event())
    .await
    .unwrap()
    .unwrap();
  assert_eq!(first_initial.change_kind, StateChangeKind::Snapshot);
  assert_eq!(second_initial.change_kind, StateChangeKind::Snapshot);
  assert!(first_initial.snapshot.registrations.is_empty());
  assert!(second_initial.snapshot.registrations.is_empty());

  let mut session = CadderSession::connect(&harness.paths).await.unwrap();
  assert!(
    register_on_session(
      &mut session,
      registration("shim-1", "nonce-1", &harness.config_path)
    )
    .await
    .accepted
  );

  let first_update = tokio::time::timeout(Duration::from_secs(1), first.next_event())
    .await
    .unwrap()
    .unwrap();
  let second_update = tokio::time::timeout(Duration::from_secs(1), second.next_event())
    .await
    .unwrap()
    .unwrap();
  assert_eq!(
    first_update.change_kind,
    StateChangeKind::RegistrationsChanged
  );
  assert_eq!(
    second_update.change_kind,
    StateChangeKind::RegistrationsChanged
  );
  assert_eq!(first_update.snapshot.registrations.len(), 1);
  assert_eq!(second_update.snapshot.registrations.len(), 1);

  drop(first);
  let toggle =
    set_domain_enabled(&harness.client, "shim-1", "api.smarketing.localhost", false).await;
  assert!(toggle.accepted, "{}", toggle.message);

  let second_follow_up = tokio::time::timeout(Duration::from_secs(1), second.next_event())
    .await
    .unwrap()
    .unwrap();
  assert_eq!(
    second_follow_up.change_kind,
    StateChangeKind::RegistrationsChanged
  );
  assert_eq!(second_follow_up.snapshot.registrations.len(), 1);
  harness.shutdown().await;
}

#[tokio::test]
async fn ipc_lifecycle_registers_one_shim_and_applies_config() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let registration = registration("shim-1", "nonce-1", &harness.config_path);
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();

  let response = register_on_session(&mut session, registration).await;

  assert!(response.accepted, "{}", response.message);
  let snapshot = query_state(&harness.client).await;
  assert_eq!(snapshot.registrations.len(), 1);
  assert_eq!(snapshot.registrations[0].registered_domains.len(), 4);
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Applied);
  assert!(snapshot.config.effective_config_hash.is_some());
  wait_for_command_log(&harness.command_log_path, "adapt").await;
  wait_for_command_log(&harness.command_log_path, "run").await;
  harness.shutdown().await;
}

#[tokio::test]
async fn shutdown_daemon_request_stops_server_and_rejects_new_clients() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;

  let response: BasicResponse = harness
    .client
    .request(
      message_types::SHUTDOWN_DAEMON_REQUEST,
      message_types::SHUTDOWN_DAEMON_RESPONSE,
      &cadder_protocol::ShutdownDaemonRequest {
        request_id: new_request_id("shutdown-daemon"),
      },
    )
    .await
    .unwrap();
  assert!(response.accepted, "{}", response.message);

  for _ in 0..50 {
    let result = harness
      .client
      .request::<_, QueryStateResponse>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: new_request_id("after-shutdown"),
        },
      )
      .await;
    if result.is_err() {
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }

  panic!("daemon server still accepted new IPC clients after shutdown request");
}

#[tokio::test]
async fn ipc_lifecycle_supports_many_registrations_and_owner_cleanup() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;

  let mut owners = Vec::new();
  let mut sessions = Vec::new();
  for index in 0..10 {
    let registration = registration(
      &format!("shim-{index}"),
      &format!("nonce-{index}"),
      &harness.config_path,
    );
    owners.push((
      registration.registration_id.clone(),
      registration.entrypoint_instance.shim_session_nonce.clone(),
    ));
    let mut session = CadderSession::connect(&harness.paths).await.unwrap();
    let response: RegisterEntrypointResponse = session
      .request(
        message_types::REGISTER_ENTRYPOINT_REQUEST,
        message_types::REGISTER_ENTRYPOINT_RESPONSE,
        &RegisterEntrypointRequest {
          request_id: new_request_id("test-register"),
          registration,
        },
      )
      .await
      .unwrap();
    assert!(response.accepted, "{}", response.message);
    sessions.push(session);
  }

  let snapshot = query_state(&harness.client).await;
  assert_eq!(snapshot.registrations.len(), 10);
  assert_eq!(snapshot.registrations[0].registered_domains.len(), 4);

  let wrong_owner: BasicResponse = harness
    .client
    .request(
      message_types::UNREGISTER_ENTRYPOINT_REQUEST,
      message_types::UNREGISTER_ENTRYPOINT_RESPONSE,
      &UnregisterEntrypointRequest {
        request_id: new_request_id("test-unregister"),
        registration_id: owners[0].0.clone(),
        shim_session_nonce: "wrong".to_string(),
      },
    )
    .await
    .unwrap();
  assert!(!wrong_owner.accepted);
  assert_eq!(query_state(&harness.client).await.registrations.len(), 10);

  let right_owner: BasicResponse = sessions[0]
    .request(
      message_types::UNREGISTER_ENTRYPOINT_REQUEST,
      message_types::UNREGISTER_ENTRYPOINT_RESPONSE,
      &UnregisterEntrypointRequest {
        request_id: new_request_id("test-unregister"),
        registration_id: owners[0].0.clone(),
        shim_session_nonce: owners[0].1.clone(),
      },
    )
    .await
    .unwrap();
  assert!(right_owner.accepted);
  assert_eq!(query_state(&harness.client).await.registrations.len(), 9);

  drop(sessions);
  for _ in 0..50 {
    if query_state(&harness.client).await.registrations.is_empty() {
      harness.shutdown().await;
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  panic!("pipe disconnect cleanup did not remove owned registrations");
}

#[tokio::test]
async fn ipc_disconnect_cleanup_removes_only_that_session_registration() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let mut first = CadderSession::connect(&harness.paths).await.unwrap();
  let mut second = CadderSession::connect(&harness.paths).await.unwrap();

  let first_registration = registration("shim-1", "nonce-1", &harness.config_path);
  let second_registration = registration("shim-2", "nonce-2", &harness.config_path);
  assert!(
    register_on_session(&mut first, first_registration)
      .await
      .accepted
  );
  assert!(
    register_on_session(&mut second, second_registration)
      .await
      .accepted
  );
  assert_eq!(query_state(&harness.client).await.registrations.len(), 2);

  drop(first);

  for _ in 0..50 {
    let snapshot = query_state(&harness.client).await;
    if snapshot.registrations.len() == 1 {
      assert_eq!(snapshot.registrations[0].registration_id, "shim-2");
      drop(second);
      harness.shutdown().await;
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  panic!("pipe disconnect cleanup did not remove only the dropped session");
}

#[tokio::test]
async fn fake_caddy_reload_tracks_effective_config_after_domain_toggle() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();
  assert!(
    register_on_session(
      &mut session,
      registration("shim-1", "nonce-1", &harness.config_path)
    )
    .await
    .accepted
  );
  wait_for_command_log(&harness.command_log_path, "run").await;

  let response =
    set_domain_enabled(&harness.client, "shim-1", "api.smarketing.localhost", false).await;

  assert!(response.accepted, "{}", response.message);
  let snapshot = query_state(&harness.client).await;
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Applied);
  let effective = fs::read_to_string(harness.paths.effective_config_path()).unwrap();
  assert!(!effective.contains("api.smarketing.localhost"));
  assert!(effective.contains("app.smarketing.localhost"));
  wait_for_command_log(&harness.command_log_path, "reload").await;
  harness.shutdown().await;
}

#[tokio::test]
async fn conflict_reporting_includes_domain_and_source_paths() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let mut first = CadderSession::connect(&harness.paths).await.unwrap();
  let mut second = CadderSession::connect(&harness.paths).await.unwrap();
  let second_config_path = harness.config_path.with_file_name("Second.Caddyfile");
  fs::write(&second_config_path, fixture).unwrap();

  assert!(
    register_on_session(
      &mut first,
      registration("shim-1", "nonce-1", &harness.config_path)
    )
    .await
    .accepted
  );
  assert!(
    register_on_session(
      &mut second,
      registration("shim-2", "nonce-2", &second_config_path)
    )
    .await
    .accepted
  );

  let snapshot = query_state(&harness.client).await;
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Failed);
  assert_eq!(snapshot.config.diagnostics.len(), 4);
  let expected_paths = vec![
    harness.config_path.display().to_string(),
    second_config_path.display().to_string(),
  ];
  let diagnostic = snapshot
    .config
    .diagnostics
    .iter()
    .find(|diagnostic| diagnostic.domain_key.as_deref() == Some("api.smarketing.localhost"))
    .unwrap();
  assert_eq!(diagnostic.code, "domain-conflict");
  assert_eq!(diagnostic.source_config_paths, expected_paths);
  harness.shutdown().await;
}

#[tokio::test]
async fn adapt_failure_reports_diagnostic_after_registration_apply() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).fail_adapt()).await;
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();

  let response = register_on_session(
    &mut session,
    registration("shim-1", "nonce-1", &harness.config_path),
  )
  .await;

  assert!(response.accepted, "{}", response.message);
  let snapshot = query_state(&harness.client).await;
  assert_eq!(snapshot.registrations.len(), 1);
  assert!(snapshot.registrations[0].registered_domains.is_empty());
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Failed);
  assert_eq!(snapshot.config.diagnostics.len(), 1);
  assert_eq!(snapshot.config.diagnostics[0].code, "adapt-failed");
  assert_eq!(
    snapshot.config.diagnostics[0].source_config_paths,
    vec![harness.config_path.display().to_string()]
  );
  harness.shutdown().await;
}

#[tokio::test]
async fn disabled_adapt_failure_does_not_block_effective_config() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).fail_adapt()).await;
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();

  assert!(
    register_on_session(
      &mut session,
      registration("shim-1", "nonce-1", &harness.config_path),
    )
    .await
    .accepted
  );
  assert_eq!(
    query_state(&harness.client).await.config.status,
    ConfigApplyStatus::Failed
  );

  let disabled: BasicResponse = harness
    .client
    .request(
      message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
      message_types::SET_ENTRYPOINT_ENABLED_RESPONSE,
      &SetEntrypointEnabledRequest {
        request_id: new_request_id("test-disable-invalid-entrypoint"),
        registration_id: "shim-1".to_string(),
        shim_session_nonce: Some("nonce-1".to_string()),
        enabled: false,
      },
    )
    .await
    .unwrap();
  let snapshot = query_state(&harness.client).await;

  assert!(disabled.accepted, "{}", disabled.message);
  assert_eq!(
    snapshot.registrations[0].activation_state,
    ActivationState::Inactive
  );
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Idle);
  assert!(snapshot.config.diagnostics.is_empty());
  harness.shutdown().await;
}

#[tokio::test]
async fn runtime_reload_failure_reports_diagnostic_and_control_log() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).fail_reload()).await;
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();
  assert!(
    register_on_session(
      &mut session,
      registration("shim-1", "nonce-1", &harness.config_path)
    )
    .await
    .accepted
  );
  wait_for_command_log(&harness.command_log_path, "run").await;

  let response =
    set_domain_enabled(&harness.client, "shim-1", "api.smarketing.localhost", false).await;

  assert!(response.accepted, "{}", response.message);
  wait_for_command_log(&harness.command_log_path, "reload").await;
  let snapshot = query_state(&harness.client).await;
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Failed);
  assert_eq!(snapshot.config.diagnostics[0].code, "runtime-apply-failed");
  let logs = query_logs(
    &harness.client,
    LogStreamIdentity::runtime_control(),
    Some(10),
    None,
  )
  .await;
  assert_eq!(logs.stream_status, LogStreamStatus::Active);
  assert!(logs.entries.iter().any(|entry| {
    entry.severity == LogSeverity::Error && entry.raw_message.contains("reload failed")
  }));
  harness.shutdown().await;
}

#[tokio::test]
async fn per_domain_log_queries_report_status_and_cursor() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();
  assert!(
    register_on_session(
      &mut session,
      registration("shim-1", "nonce-1", &harness.config_path)
    )
    .await
    .accepted
  );
  let stream = LogStreamIdentity::domain("api.smarketing.localhost");
  harness.state.logs().append(
    stream.clone(),
    LogSeverity::Info,
    "first domain log",
    LogAttributionKind::Domain,
    None,
  );
  harness.state.logs().append(
    stream.clone(),
    LogSeverity::Error,
    "second domain log",
    LogAttributionKind::Domain,
    None,
  );

  let first_page = query_logs(&harness.client, stream.clone(), Some(1), None).await;

  assert_eq!(first_page.stream_status, LogStreamStatus::Active);
  assert_eq!(first_page.entries.len(), 1);
  assert_eq!(first_page.entries[0].raw_message, "second domain log");
  assert!(first_page.has_more_before);
  let cursor = first_page.next_cursor.clone();

  let next_page = query_logs(&harness.client, stream, Some(10), cursor).await;

  assert_eq!(next_page.stream_status, LogStreamStatus::Active);
  assert!(next_page.entries.is_empty());
  harness.shutdown().await;
}

#[tokio::test]
async fn ipc_heartbeat_and_entrypoint_toggle_update_registered_state() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();
  assert!(
    register_on_session(
      &mut session,
      registration("shim-1", "nonce-1", &harness.config_path)
    )
    .await
    .accepted
  );

  let heartbeat: BasicResponse = harness
    .client
    .request(
      message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
      message_types::HEARTBEAT_ENTRYPOINT_RESPONSE,
      &HeartbeatEntrypointRequest {
        request_id: new_request_id("test-heartbeat"),
        registration_id: "shim-1".to_string(),
        shim_session_nonce: "nonce-1".to_string(),
      },
    )
    .await
    .unwrap();
  let disabled: BasicResponse = harness
    .client
    .request(
      message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
      message_types::SET_ENTRYPOINT_ENABLED_RESPONSE,
      &SetEntrypointEnabledRequest {
        request_id: new_request_id("test-entrypoint-toggle"),
        registration_id: "shim-1".to_string(),
        shim_session_nonce: Some("nonce-1".to_string()),
        enabled: false,
      },
    )
    .await
    .unwrap();
  let snapshot = query_state(&harness.client).await;

  assert!(heartbeat.accepted);
  assert_eq!(heartbeat.message, "Heartbeat accepted.");
  assert!(disabled.accepted);
  assert_eq!(
    snapshot.registrations[0].activation_state,
    ActivationState::Inactive
  );
  harness.shutdown().await;
}

#[tokio::test]
async fn ipc_reports_unsupported_message_type() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();

  let response: BasicResponse = session
    .request(
      "unknown-message",
      "error-response",
      &QueryStateRequest {
        request_id: "unsupported".to_string(),
      },
    )
    .await
    .unwrap();

  assert!(!response.accepted);
  assert_eq!(response.request_id, "unsupported");
  assert!(
    response
      .message
      .contains("Unsupported IPC message type `unknown-message`")
  );
  harness.shutdown().await;
}

#[tokio::test]
async fn session_rejects_unexpected_response_type() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();

  let error = session
    .request::<_, QueryStateResponse>(
      message_types::QUERY_STATE_REQUEST,
      message_types::QUERY_LOGS_RESPONSE,
      &QueryStateRequest {
        request_id: "wrong-response".to_string(),
      },
    )
    .await
    .unwrap_err();

  assert!(
    error
      .to_string()
      .contains("unexpected response type `query-state-response`")
  );
  harness.shutdown().await;
}

async fn query_state(client: &CadderClient) -> cadder_protocol::GuiStateSnapshot {
  let response: QueryStateResponse = client
    .request(
      message_types::QUERY_STATE_REQUEST,
      message_types::QUERY_STATE_RESPONSE,
      &QueryStateRequest {
        request_id: new_request_id("test-query"),
      },
    )
    .await
    .unwrap();
  response.snapshot.unwrap()
}

async fn query_logs(
  client: &CadderClient,
  stream: LogStreamIdentity,
  limit: Option<usize>,
  cursor: Option<String>,
) -> QueryLogsResponse {
  client
    .request(
      message_types::QUERY_LOGS_REQUEST,
      message_types::QUERY_LOGS_RESPONSE,
      &QueryLogsRequest {
        request_id: new_request_id("test-logs"),
        stream,
        limit,
        cursor,
        minimum_severity: None,
      },
    )
    .await
    .unwrap()
}

async fn set_domain_enabled(
  client: &CadderClient,
  registration_id: &str,
  domain_key: &str,
  enabled: bool,
) -> BasicResponse {
  client
    .request(
      message_types::SET_DOMAIN_ENABLED_REQUEST,
      message_types::SET_DOMAIN_ENABLED_RESPONSE,
      &SetDomainEnabledRequest {
        request_id: new_request_id("test-domain-toggle"),
        registration_id: registration_id.to_string(),
        domain_key: domain_key.to_string(),
        enabled,
      },
    )
    .await
    .unwrap()
}

async fn register_on_session(
  session: &mut CadderSession,
  registration: EntrypointRegistration,
) -> RegisterEntrypointResponse {
  session
    .request(
      message_types::REGISTER_ENTRYPOINT_REQUEST,
      message_types::REGISTER_ENTRYPOINT_RESPONSE,
      &RegisterEntrypointRequest {
        request_id: new_request_id("test-register"),
        registration,
      },
    )
    .await
    .unwrap()
}

struct Harness {
  client: CadderClient,
  state: DaemonState,
  paths: RuntimePaths,
  shutdown_tx: watch::Sender<bool>,
  config_path: PathBuf,
  command_log_path: PathBuf,
  _temp: tempfile::TempDir,
}

impl Harness {
  async fn start(fake_caddy: FakeCaddy<'_>) -> Self {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("run");
    let paths = RuntimePaths::resolve(Some(runtime_dir)).unwrap();
    paths.ensure_dirs().unwrap();
    let config_path = temp.path().join("Caddyfile");
    fs::write(&config_path, fake_caddy.caddyfile).unwrap();
    let command_log_path = temp.path().join("fake-caddy-commands.log");
    let fake_caddy_path = write_fake_caddy(temp.path(), &command_log_path, fake_caddy);

    let resolver = RealCaddyResolver::new(Some(fake_caddy_path.display().to_string()));
    let adapter = CaddyConfigAdapter::new(resolver.clone());
    let runtime = ProcessRuntime::new(resolver, paths.clone());
    let state = DaemonState::new(CaddyConfigCoordinator::new(adapter, runtime));
    let server = DaemonServer::new(paths.clone(), state.clone());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
      let _ = server.run_until(shutdown_rx).await;
    });

    let client = CadderClient::new(paths.clone());
    for _ in 0..50 {
      if client
        .request::<_, QueryStateResponse>(
          message_types::QUERY_STATE_REQUEST,
          message_types::QUERY_STATE_RESPONSE,
          &QueryStateRequest {
            request_id: new_request_id("wait"),
          },
        )
        .await
        .is_ok()
      {
        return Self {
          client,
          state,
          paths,
          shutdown_tx,
          config_path,
          command_log_path,
          _temp: temp,
        };
      }
      sleep(Duration::from_millis(20)).await;
    }
    panic!("server did not become ready");
  }

  async fn shutdown(self) {
    let _ = self.shutdown_tx.send(true);
  }
}

#[derive(Debug, Clone, Copy)]
struct FakeCaddy<'a> {
  caddyfile: &'a str,
  fail_adapt: bool,
  fail_reload: bool,
}

impl<'a> FakeCaddy<'a> {
  fn new(caddyfile: &'a str) -> Self {
    Self {
      caddyfile,
      fail_adapt: false,
      fail_reload: false,
    }
  }

  fn fail_adapt(mut self) -> Self {
    self.fail_adapt = true;
    self
  }

  fn fail_reload(mut self) -> Self {
    self.fail_reload = true;
    self
  }
}

fn registration(id: &str, nonce: &str, config_path: &Path) -> EntrypointRegistration {
  let now = Utc::now();
  let identity = EntrypointInstanceIdentity {
    instance_id: id.to_string(),
    started_at_utc: now,
    shim_session_nonce: nonce.to_string(),
  };
  EntrypointRegistration {
    registration_id: id.to_string(),
    entrypoint_instance: identity.clone(),
    source_working_directory: SourcePath::new(".", None),
    source_config_path: SourcePath::new(
      config_path.display().to_string(),
      Some(config_path.display().to_string()),
    ),
    registered_domains: Vec::new(),
    activation_state: ActivationState::Active,
    owner_process: OwnerProcessIdentity {
      process_id: 1,
      process_start_time_utc: now,
      shim_session_nonce: nonce.to_string(),
      executable_path: None,
    },
    log_stream: LogStreamIdentity::entrypoint(id),
    shim_run: Some(ShimRunMetadata {
      adapter: Some("caddyfile".to_string()),
      raw_arguments: vec!["run".to_string()],
      command_line: "run".to_string(),
    }),
    created_at_utc: now,
    last_heartbeat_utc: now,
  }
}

async fn wait_for_command_log(path: &Path, command: &str) {
  for _ in 0..50 {
    let log = fs::read_to_string(path).unwrap_or_default();
    if log.lines().any(|line| line.starts_with(command)) {
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  let log = fs::read_to_string(path).unwrap_or_default();
  panic!("expected command `{command}` in fake Caddy log:\n{log}");
}

fn write_fake_caddy(dir: &Path, command_log_path: &Path, fake_caddy: FakeCaddy<'_>) -> PathBuf {
  const ADAPTED_JSON: &str = r#"{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["api.smarketing.localhost","app.smarketing.localhost","mailbox.smarketing.localhost","storage.smarketing.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}"#;
  let adapt_failure = if fake_caddy.fail_adapt {
    "adapt failed"
  } else {
    ""
  };
  let reload_failure = if fake_caddy.fail_reload {
    "reload failed"
  } else {
    ""
  };
  #[cfg(windows)]
  {
    let path = dir.join("fake-caddy.cmd");
    fs::write(
      &path,
      format!(
        r#"@echo off
echo %*>> "{command_log}"
if "%1"=="adapt" (
  if not "{adapt_failure}"=="" (
    echo {adapt_failure} 1>&2
    exit 6
  )
  echo {adapted_json}
  exit 0
)
if "%1"=="reload" (
  if not "{reload_failure}"=="" (
    echo {reload_failure} 1>&2
    exit 7
  )
  exit 0
)
if "%1"=="run" (
  echo fake runtime started
  exit 0
)
exit 0
"#,
        command_log = command_log_path.display(),
        adapted_json = ADAPTED_JSON,
        adapt_failure = adapt_failure,
        reload_failure = reload_failure,
      ),
    )
    .unwrap();
    path
  }

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-caddy");
    fs::write(
      &path,
      format!(
        r#"#!/usr/bin/env sh
printf '%s\n' "$*" >> '{command_log}'
if [ "$1" = "adapt" ]; then
  if [ -n '{adapt_failure}' ]; then
    printf '%s\n' '{adapt_failure}' >&2
    exit 6
  fi
  printf '%s\n' '{adapted_json}'
  exit 0
fi
if [ "$1" = "reload" ]; then
  if [ -n '{reload_failure}' ]; then
    printf '%s\n' '{reload_failure}' >&2
    exit 7
  fi
  exit 0
fi
if [ "$1" = "run" ]; then echo fake runtime started; exit 0; fi
exit 0
"#,
        command_log = command_log_path.display(),
        adapted_json = ADAPTED_JSON,
        adapt_failure = adapt_failure,
        reload_failure = reload_failure,
      ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
  }
}
