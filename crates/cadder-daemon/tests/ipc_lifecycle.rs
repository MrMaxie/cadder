use cadder_daemon::{
  CadderClient, CadderSession, CaddyConfigAdapter, CaddyConfigCoordinator, DaemonServer,
  DaemonState, ProcessRuntime, RealCaddyResolver, RuntimePaths, RuntimeTimeouts,
  ensure_daemon_running,
};
use cadder_protocol::{
  ActivationState, BasicResponse, ConfigApplyStatus, EntrypointInstanceIdentity,
  EntrypointRegistration, HeartbeatEntrypointRequest, IpcEnvelope, LogAttributionKind, LogSeverity,
  LogStreamIdentity, LogStreamStatus, OwnerProcessIdentity, PROTOCOL_VERSION, ProtocolErrorKind,
  ProtocolErrorResponse, QueryAutostartRequest, QueryAutostartResponse, QueryIisBindingsRequest,
  QueryIisBindingsResponse, QueryLogsRequest, QueryLogsResponse, QueryStateRequest,
  QueryStateResponse, RegisterEntrypointRequest, RegisterEntrypointResponse, RuntimeStatus,
  SetAutostartRequest, SetDomainEnabledRequest, SetEntrypointEnabledRequest, SetIisHandoffRequest,
  SetIisHandoffResponse, ShimRunMetadata, SourcePath, StateChangeKind, UnregisterEntrypointRequest,
  message_types, new_request_id,
};
use chrono::Utc;
use interprocess::local_socket::{
  GenericNamespaced, ListenerOptions, ToNsName,
  tokio::{Stream, prelude::*},
};
use serde::Serialize;
use std::{
  fs,
  future::Future,
  path::{Path, PathBuf},
  time::Duration,
};
use tokio::{
  io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
  sync::watch,
  task::JoinHandle,
  time::sleep,
};

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
async fn raw_ipc_registration_records_history_in_memory_storage() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let (mut reader, mut writer) = raw_ipc_session(&harness.paths).await;

  write_raw_envelope(
    &mut writer,
    message_types::REGISTER_ENTRYPOINT_REQUEST,
    &RegisterEntrypointRequest {
      request_id: new_request_id("raw-register-contract"),
      registration: registration("shim-raw", "nonce-raw", &harness.config_path),
    },
  )
  .await;
  let response: RegisterEntrypointResponse = read_raw_envelope(&mut reader).await.decode().unwrap();
  let snapshot = query_state(&harness.client).await;
  let history = query_history(&harness.client, cadder_protocol::HistoryKind::Registration).await;
  let storage = history.storage.as_ref().unwrap();

  assert!(response.accepted, "{}", response.message);
  assert_eq!(snapshot.registrations.len(), 1);
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Applied);
  assert_eq!(storage.backend, "sqlite");
  assert!(storage.path.is_none());
  assert!(history.records.iter().any(|record| {
    record.registration_id.as_deref() == Some("shim-raw")
      && record.summary == "Registered entrypoint `shim-raw`."
  }));
  drop(writer);
  drop(reader);
  harness.shutdown().await;
}

#[tokio::test]
async fn ipc_local_operator_requests_cover_autostart_status() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;

  let autostart: QueryAutostartResponse = harness
    .client
    .request(
      message_types::QUERY_AUTOSTART_REQUEST,
      message_types::QUERY_AUTOSTART_RESPONSE,
      &QueryAutostartRequest {
        request_id: new_request_id("test-query-autostart"),
      },
    )
    .await
    .unwrap();
  #[cfg(all(unix, not(target_os = "macos")))]
  {
    assert!(!autostart.accepted, "{}", autostart.message);
    assert_eq!(
      autostart.status,
      cadder_protocol::AutostartStatus::Unsupported
    );
    assert!(
      autostart
        .diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.code == "autostart-config-dir-unavailable" })
    );
  }
  #[cfg(not(all(unix, not(target_os = "macos"))))]
  assert!(autostart.accepted, "{}", autostart.message);

  harness.shutdown().await;
}

#[tokio::test]
async fn ipc_set_autostart_request_records_history_over_wire() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;

  let response: cadder_protocol::SetAutostartResponse = harness
    .client
    .request(
      message_types::SET_AUTOSTART_REQUEST,
      message_types::SET_AUTOSTART_RESPONSE,
      &SetAutostartRequest {
        request_id: new_request_id("test-set-autostart"),
        mode: cadder_protocol::AutostartMode::Daemon,
      },
    )
    .await
    .unwrap();
  let history = query_history(&harness.client, cadder_protocol::HistoryKind::Autostart).await;

  assert!(!response.accepted);
  assert_eq!(
    response.status,
    cadder_protocol::AutostartStatus::Unsupported
  );
  assert!(response.target.is_none());
  assert!(history.records.iter().any(|record| {
    record.kind == cadder_protocol::HistoryKind::Autostart
      && record.summary == "Set autostart mode to Daemon."
  }));
  harness.shutdown().await;
}

#[tokio::test]
async fn ipc_iis_operator_requests_return_typed_responses() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;

  let bindings: QueryIisBindingsResponse = harness
    .client
    .request(
      message_types::QUERY_IIS_BINDINGS_REQUEST,
      message_types::QUERY_IIS_BINDINGS_RESPONSE,
      &QueryIisBindingsRequest {
        request_id: new_request_id("test-query-iis"),
      },
    )
    .await
    .unwrap();
  let handoff: SetIisHandoffResponse = harness
    .client
    .request(
      message_types::SET_IIS_HANDOFF_REQUEST,
      message_types::SET_IIS_HANDOFF_RESPONSE,
      &SetIisHandoffRequest {
        request_id: new_request_id("test-set-iis"),
        binding_id: "missing|http|127.0.0.1:1:missing.localhost".to_string(),
        enabled: true,
        route_host: None,
      },
    )
    .await
    .unwrap();

  assert!(bindings.request_id.starts_with("test-query-iis-"));
  assert!(handoff.request_id.starts_with("test-set-iis-"));
  assert!(!handoff.accepted);
  assert!(handoff.issue.is_some() || handoff.binding.is_none());
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
async fn shutdown_daemon_request_reports_stop_timeout_and_keeps_server_available() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).slow_stop(short_runtime_timeouts())).await;
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
  let snapshot = query_state(&harness.client).await;
  let history = query_history(&harness.client, cadder_protocol::HistoryKind::Runtime).await;

  assert!(!response.accepted);
  assert!(response.message.contains("timed out"));
  assert_eq!(snapshot.registrations.len(), 1);
  assert!(
    history
      .records
      .iter()
      .all(|record| record.summary != "Daemon shutdown requested.")
  );
  harness.shutdown().await;
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
async fn ipc_parse_error_cleanup_removes_owned_registration() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let (mut reader, mut writer) = raw_ipc_session(&harness.paths).await;

  write_raw_envelope(
    &mut writer,
    message_types::REGISTER_ENTRYPOINT_REQUEST,
    &RegisterEntrypointRequest {
      request_id: new_request_id("raw-register"),
      registration: registration("shim-1", "nonce-1", &harness.config_path),
    },
  )
  .await;
  let response: RegisterEntrypointResponse = read_raw_envelope(&mut reader).await.decode().unwrap();
  assert!(response.accepted, "{}", response.message);
  assert_eq!(query_state(&harness.client).await.registrations.len(), 1);

  write_raw_line(&mut writer, "{not-json}\n").await;
  drop(writer);
  drop(reader);

  wait_for_empty_registrations(
    &harness.client,
    "parse error cleanup did not remove the owned registration",
  )
  .await;
  harness.shutdown().await;
}

#[tokio::test]
async fn ipc_failed_unregister_keeps_owner_for_disconnect_cleanup() {
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

  let wrong_owner: BasicResponse = session
    .request(
      message_types::UNREGISTER_ENTRYPOINT_REQUEST,
      message_types::UNREGISTER_ENTRYPOINT_RESPONSE,
      &UnregisterEntrypointRequest {
        request_id: new_request_id("wrong-owner-unregister"),
        registration_id: "shim-1".to_string(),
        shim_session_nonce: "wrong-nonce".to_string(),
      },
    )
    .await
    .unwrap();
  assert!(!wrong_owner.accepted);
  assert_eq!(query_state(&harness.client).await.registrations.len(), 1);

  drop(session);

  wait_for_empty_registrations(
    &harness.client,
    "failed unregister removed the session owner before disconnect cleanup",
  )
  .await;
  harness.shutdown().await;
}

#[tokio::test]
async fn ipc_subscription_disconnect_cleanup_removes_owned_registration() {
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
  let mut subscription = session
    .subscribe_state(new_request_id("subscription-cleanup"))
    .await
    .unwrap();
  let initial = tokio::time::timeout(Duration::from_secs(1), subscription.next_event())
    .await
    .unwrap()
    .unwrap();
  assert_eq!(initial.change_kind, StateChangeKind::Snapshot);
  assert_eq!(initial.snapshot.registrations.len(), 1);

  drop(subscription);

  for attempt in 0..50 {
    let _ =
      set_entrypoint_enabled(&harness.client, "shim-1", Some("nonce-1"), attempt % 2 == 0).await;
    if query_state(&harness.client).await.registrations.is_empty() {
      harness.shutdown().await;
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  panic!("subscription disconnect cleanup did not remove the owned registration");
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
  let logs = query_logs(
    &harness.client,
    LogStreamIdentity::runtime_control(),
    Some(20),
    None,
  )
  .await;
  assert!(logs.entries.iter().any(|entry| {
    entry.operation.as_deref() == Some("reload")
      && entry.severity == LogSeverity::Info
      && entry.raw_message.contains("runtime reloaded")
  }));

  let reenabled =
    set_domain_enabled(&harness.client, "shim-1", "api.smarketing.localhost", true).await;
  assert!(reenabled.accepted, "{}", reenabled.message);
  let restored = query_state(&harness.client).await;
  assert_eq!(
    restored.registrations[0].registered_domains[0].activation_state,
    ActivationState::Active
  );
  let history = query_history(&harness.client, cadder_protocol::HistoryKind::Registration).await;
  assert!(history.records.iter().any(|record| {
    record
      .summary
      .contains("Disabled domain `api.smarketing.localhost`")
  }));
  assert!(history.records.iter().any(|record| {
    record
      .summary
      .contains("Enabled domain `api.smarketing.localhost`")
  }));
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
async fn slow_adapt_does_not_block_state_queries() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness =
    Harness::start(FakeCaddy::new(fixture).slow_adapt(Duration::from_millis(500))).await;
  let paths = harness.paths.clone();
  let config_path = harness.config_path.clone();
  let register_task = tokio::spawn(async move {
    let mut session = CadderSession::connect(&paths).await.unwrap();
    let response = register_on_session(
      &mut session,
      registration("shim-1", "nonce-1", &config_path),
    )
    .await;
    (response, session)
  });
  wait_for_command_log(&harness.command_log_path, "adapt").await;

  let snapshot = tokio::time::timeout(Duration::from_millis(200), query_state(&harness.client))
    .await
    .expect("query-state should not wait for slow adapt");
  let (response, _session) = register_task.await.unwrap();
  let final_snapshot = query_state(&harness.client).await;

  assert!(snapshot.registrations.is_empty());
  assert!(response.accepted, "{response:?}");
  assert_eq!(final_snapshot.config.status, ConfigApplyStatus::Failed);
  assert_eq!(final_snapshot.config.diagnostics[0].code, "adapt-failed");
  assert!(
    final_snapshot.config.diagnostics[0]
      .message
      .contains("timed out")
  );
  harness.shutdown().await;
}

#[tokio::test]
async fn slow_reload_does_not_block_state_queries_and_reports_timeout() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).slow_reload(short_runtime_timeouts())).await;
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

  let client = harness.client.clone();
  let toggle_task = tokio::spawn(async move {
    set_domain_enabled(&client, "shim-1", "api.smarketing.localhost", false).await
  });
  wait_for_command_log(&harness.command_log_path, "reload").await;

  let snapshot = tokio::time::timeout(Duration::from_millis(200), query_state(&harness.client))
    .await
    .expect("query-state should not wait for slow reload");
  let response = toggle_task.await.unwrap();
  let final_snapshot = query_state(&harness.client).await;

  assert_eq!(snapshot.registrations.len(), 1);
  assert!(response.accepted, "{response:?}");
  assert_eq!(final_snapshot.config.status, ConfigApplyStatus::Failed);
  assert_eq!(
    final_snapshot.config.diagnostics[0].code,
    "runtime-apply-failed"
  );
  assert!(
    final_snapshot.config.diagnostics[0]
      .message
      .contains("timed out")
  );
  harness.shutdown().await;
}

#[tokio::test]
async fn slow_reload_subscription_event_reports_coherent_failed_snapshot() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).slow_reload(short_runtime_timeouts())).await;
  let mut subscription = harness
    .client
    .subscribe_state(new_request_id("dashboard"))
    .await
    .unwrap();
  let initial = tokio::time::timeout(Duration::from_secs(1), subscription.next_event())
    .await
    .unwrap()
    .unwrap();
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();
  assert!(!initial.snapshot.registrations.iter().any(|registration| {
    registration
      .registered_domains
      .iter()
      .any(|domain| domain.name.canonical == "api.smarketing.localhost")
  }));
  assert!(
    register_on_session(
      &mut session,
      registration("shim-1", "nonce-1", &harness.config_path)
    )
    .await
    .accepted
  );
  wait_for_command_log(&harness.command_log_path, "run").await;
  let _registered = tokio::time::timeout(Duration::from_secs(1), subscription.next_event())
    .await
    .unwrap()
    .unwrap();

  let response =
    set_domain_enabled(&harness.client, "shim-1", "api.smarketing.localhost", false).await;
  let event = tokio::time::timeout(Duration::from_secs(2), subscription.next_event())
    .await
    .unwrap()
    .unwrap();
  let api_domain = event.snapshot.registrations[0]
    .registered_domains
    .iter()
    .find(|domain| domain.name.canonical == "api.smarketing.localhost")
    .unwrap();

  assert!(response.accepted, "{response:?}");
  assert_eq!(event.change_kind, StateChangeKind::RegistrationsChanged);
  assert_eq!(api_domain.activation_state, ActivationState::Inactive);
  assert_eq!(event.snapshot.config.status, ConfigApplyStatus::Failed);
  assert_eq!(
    event.snapshot.config.diagnostics[0].code,
    "runtime-apply-failed"
  );
  assert!(
    event.snapshot.config.diagnostics[0]
      .message
      .contains("timed out")
  );
  harness.shutdown().await;
}

#[tokio::test]
async fn slow_stop_does_not_block_state_queries_and_logs_timeout() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).slow_stop(short_runtime_timeouts())).await;
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

  let client = harness.client.clone();
  let toggle_task = tokio::spawn(async move {
    client
      .request::<_, BasicResponse>(
        message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
        message_types::SET_ENTRYPOINT_ENABLED_RESPONSE,
        &SetEntrypointEnabledRequest {
          request_id: new_request_id("test-disable-entrypoint"),
          registration_id: "shim-1".to_string(),
          shim_session_nonce: Some("nonce-1".to_string()),
          enabled: false,
        },
      )
      .await
      .unwrap()
  });
  wait_for_command_log(&harness.command_log_path, "stop").await;

  let snapshot = tokio::time::timeout(Duration::from_millis(200), query_state(&harness.client))
    .await
    .expect("query-state should not wait for slow stop");
  let response = toggle_task.await.unwrap();
  let logs = query_logs(
    &harness.client,
    LogStreamIdentity::runtime_control(),
    Some(10),
    None,
  )
  .await;

  assert_eq!(snapshot.registrations.len(), 1);
  assert!(response.accepted, "{response:?}");
  assert!(logs.entries.iter().any(|entry| {
    entry.severity == LogSeverity::Error && entry.raw_message.contains("timed out")
  }));
  harness.shutdown().await;
}

#[tokio::test]
async fn exited_runtime_child_is_not_reported_running() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).short_run()).await;
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
  wait_for_command_log(&harness.command_log_path, "run-exited").await;

  let snapshot = wait_for_runtime_status(&harness.client, RuntimeStatus::Unhealthy).await;

  assert_eq!(snapshot.runtime.status, RuntimeStatus::Unhealthy);
  assert_eq!(snapshot.runtime.diagnostics[0].code, "runtime-exited");
  let follow_up = query_state(&harness.client).await;
  assert_eq!(follow_up.runtime.status, RuntimeStatus::Idle);
  assert!(follow_up.runtime.process_id.is_none());
  harness.shutdown().await;
}

#[tokio::test]
async fn immediate_runtime_exit_reports_start_failure() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).exit_run().runtime_timeouts(
    RuntimeTimeouts {
      start_check: Duration::from_secs(2),
      ..RuntimeTimeouts::default()
    },
  ))
  .await;
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();

  let response = register_on_session(
    &mut session,
    registration("shim-1", "nonce-1", &harness.config_path),
  )
  .await;
  let snapshot = query_state(&harness.client).await;

  assert!(response.accepted, "{response:?}");
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Failed);
  assert_eq!(snapshot.config.diagnostics[0].code, "runtime-apply-failed");
  assert!(
    snapshot.config.diagnostics[0]
      .message
      .contains("exited immediately")
  );
  harness.shutdown().await;
}

#[tokio::test]
async fn shutdown_after_runtime_child_exit_is_accepted() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).short_run()).await;
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
  wait_for_command_log(&harness.command_log_path, "run-exited").await;

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
}

#[tokio::test]
async fn apply_after_runtime_child_exit_restarts_without_prior_inspect() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).short_run()).await;
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
  wait_for_command_log(&harness.command_log_path, "run-exited").await;

  let response =
    set_domain_enabled(&harness.client, "shim-1", "api.smarketing.localhost", false).await;
  wait_for_command_count(&harness.command_log_path, "run", 2).await;
  let snapshot = query_state(&harness.client).await;
  let logs = wait_for_runtime_control_log(&harness.client, "runtime-liveness", "restarting").await;

  assert!(response.accepted, "{response:?}");
  assert_eq!(snapshot.config.status, ConfigApplyStatus::Applied);
  assert!(logs.entries.iter().any(|entry| {
    entry.operation.as_deref() == Some("runtime-liveness")
      && entry.raw_message.contains("restarting")
  }));
  harness.shutdown().await;
}

#[tokio::test]
async fn runtime_restart_after_child_exit_publishes_recovered_snapshot() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture).short_run()).await;
  let mut subscription = harness
    .client
    .subscribe_state(new_request_id("dashboard"))
    .await
    .unwrap();
  let _initial = tokio::time::timeout(Duration::from_secs(1), subscription.next_event())
    .await
    .unwrap()
    .unwrap();
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
  wait_for_command_log(&harness.command_log_path, "run-exited").await;
  let _registered = tokio::time::timeout(Duration::from_secs(1), subscription.next_event())
    .await
    .unwrap()
    .unwrap();

  let response =
    set_domain_enabled(&harness.client, "shim-1", "api.smarketing.localhost", false).await;
  wait_for_command_count(&harness.command_log_path, "run", 2).await;
  let event = tokio::time::timeout(Duration::from_secs(1), subscription.next_event())
    .await
    .unwrap()
    .unwrap();
  let logs = wait_for_runtime_control_log(&harness.client, "runtime-liveness", "restarting").await;

  assert!(response.accepted, "{response:?}");
  assert_eq!(event.change_kind, StateChangeKind::RegistrationsChanged);
  assert_eq!(event.snapshot.config.status, ConfigApplyStatus::Applied);
  assert_ne!(event.snapshot.runtime.status, RuntimeStatus::Idle);
  assert!(logs.entries.iter().any(|entry| {
    entry.operation.as_deref() == Some("runtime-liveness")
      && entry.raw_message.contains("restarting")
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

  let response: ProtocolErrorResponse = session
    .request(
      "unknown-message",
      message_types::PROTOCOL_ERROR_RESPONSE,
      &QueryStateRequest {
        request_id: "unsupported".to_string(),
      },
    )
    .await
    .unwrap();

  assert!(!response.accepted);
  assert_eq!(response.request_id, "unsupported");
  assert_eq!(
    response.error.kind,
    ProtocolErrorKind::UnsupportedCapability
  );
  assert_eq!(
    response.error.required_capability.as_deref(),
    Some("message-type:unknown-message")
  );
  assert!(
    response
      .capabilities
      .as_ref()
      .expect("protocol error response should advertise daemon capabilities")
      .supports(cadder_protocol::capabilities::LOGS)
  );
  harness.shutdown().await;
}

#[tokio::test]
async fn ipc_accepts_newer_protocol_version_when_capabilities_are_compatible() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let (mut reader, mut writer) = raw_ipc_session(&harness.paths).await;
  let mut request = IpcEnvelope::new(
    message_types::QUERY_STATE_REQUEST,
    &QueryStateRequest {
      request_id: "newer-protocol".to_string(),
    },
  )
  .unwrap();
  request.protocol_version = PROTOCOL_VERSION.saturating_add(1);
  let rendered = serde_json::to_string(&request).unwrap();

  write_raw_line(&mut writer, &format!("{rendered}\n")).await;
  let envelope = read_raw_envelope(&mut reader).await;
  let response: QueryStateResponse = envelope.decode().unwrap();

  assert_eq!(envelope.message_type, message_types::QUERY_STATE_RESPONSE);
  assert!(response.accepted);
  assert_eq!(response.request_id, "newer-protocol");
  assert!(response.snapshot.is_some());
  harness.shutdown().await;
}

#[tokio::test]
async fn ipc_rejects_protocol_version_below_compatibility_floor_with_typed_guidance() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let (mut reader, mut writer) = raw_ipc_session(&harness.paths).await;
  let mut request = IpcEnvelope::new(
    message_types::QUERY_STATE_REQUEST,
    &QueryStateRequest {
      request_id: "too-old-protocol".to_string(),
    },
  )
  .unwrap();
  request.protocol_version = cadder_protocol::MIN_COMPATIBLE_PROTOCOL_VERSION.saturating_sub(1);
  let rendered = serde_json::to_string(&request).unwrap();

  write_raw_line(&mut writer, &format!("{rendered}\n")).await;
  let envelope = read_raw_envelope(&mut reader).await;
  let response: ProtocolErrorResponse = envelope.decode().unwrap();

  assert_eq!(
    envelope.message_type,
    message_types::PROTOCOL_ERROR_RESPONSE
  );
  assert!(!response.accepted);
  assert_eq!(response.request_id, "too-old-protocol");
  assert_eq!(
    response.error.kind,
    ProtocolErrorKind::IncompatibleProtocolVersion
  );
  assert!(
    response
      .error
      .guidance
      .as_deref()
      .is_some_and(|guidance| guidance.contains("Upgrade the Cadder client"))
  );
  assert!(
    response
      .capabilities
      .as_ref()
      .expect("protocol error response should advertise daemon capabilities")
      .supports(cadder_protocol::capabilities::LOGS)
  );
  harness.shutdown().await;
}

#[tokio::test]
async fn session_reports_protocol_error_response_as_request_error() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let mut session = CadderSession::connect(&harness.paths).await.unwrap();

  let error = session
    .request::<_, QueryStateResponse>(
      "unknown-message",
      message_types::QUERY_STATE_RESPONSE,
      &QueryStateRequest {
        request_id: "unsupported".to_string(),
      },
    )
    .await
    .unwrap_err();

  assert!(
    error
      .to_string()
      .contains("daemon rejected IPC request `unsupported`"),
    "{error:?}"
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

#[tokio::test]
async fn ipc_rejects_invalid_payload_shapes_for_supported_messages() {
  let fixture = include_str!("fixtures/SmarketingReverseProxy.Caddyfile");
  let harness = Harness::start(FakeCaddy::new(fixture)).await;
  let supported_messages = [
    message_types::REGISTER_ENTRYPOINT_REQUEST,
    message_types::UNREGISTER_ENTRYPOINT_REQUEST,
    message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
    message_types::QUERY_STATE_REQUEST,
    message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
    message_types::SET_DOMAIN_ENABLED_REQUEST,
    message_types::QUERY_IIS_BINDINGS_REQUEST,
    message_types::SET_IIS_HANDOFF_REQUEST,
    message_types::QUERY_LOGS_REQUEST,
    message_types::QUERY_HISTORY_REQUEST,
    message_types::QUERY_AUTOSTART_REQUEST,
    message_types::SET_AUTOSTART_REQUEST,
    message_types::SUBSCRIBE_STATE_REQUEST,
    message_types::SHUTDOWN_DAEMON_REQUEST,
  ];

  for message_type in supported_messages {
    let (mut reader, mut writer) = raw_ipc_session(&harness.paths).await;
    write_raw_envelope(&mut writer, message_type, &serde_json::Value::Null).await;

    let envelope = read_raw_envelope(&mut reader).await;
    let response: ProtocolErrorResponse = envelope.decode().unwrap();

    assert_eq!(
      envelope.message_type,
      message_types::PROTOCOL_ERROR_RESPONSE
    );
    assert!(!response.accepted);
    assert_eq!(response.request_id, "unknown");
    assert_eq!(response.error.kind, ProtocolErrorKind::PayloadDecodeFailed);
  }

  assert!(query_state(&harness.client).await.registrations.is_empty());
  harness.shutdown().await;
}

#[tokio::test]
async fn session_reports_eof_when_peer_closes_without_response() {
  let peer = ScriptedIpcPeer::start(|conn| async move {
    let (read_half, write_half) = tokio::io::split(conn);
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    drop(write_half);
  });
  let mut session = CadderSession::connect(&peer.paths).await.unwrap();

  let error = session
    .request::<_, QueryStateResponse>(
      message_types::QUERY_STATE_REQUEST,
      message_types::QUERY_STATE_RESPONSE,
      &QueryStateRequest {
        request_id: new_request_id("eof-query-state"),
      },
    )
    .await
    .unwrap_err();

  assert!(
    error.to_string().contains("without a response"),
    "{error:?}"
  );
  peer.finish().await;
}

#[tokio::test]
async fn session_rejects_malformed_response_json() {
  let peer = ScriptedIpcPeer::start(|conn| async move {
    let (_line, mut writer) = read_peer_request(conn).await;
    write_raw_line(&mut writer, "{not-json}\n").await;
  });
  let mut session = CadderSession::connect(&peer.paths).await.unwrap();

  let error = session
    .request::<_, QueryStateResponse>(
      message_types::QUERY_STATE_REQUEST,
      message_types::QUERY_STATE_RESPONSE,
      &QueryStateRequest {
        request_id: new_request_id("bad-json-query-state"),
      },
    )
    .await
    .unwrap_err();

  assert!(
    error.to_string().contains("key must be a string"),
    "{error:?}"
  );
  peer.finish().await;
}

#[tokio::test]
async fn session_rejects_invalid_response_payload() {
  let peer = ScriptedIpcPeer::start(|conn| async move {
    let (_line, mut writer) = read_peer_request(conn).await;
    write_raw_envelope(
      &mut writer,
      message_types::QUERY_STATE_RESPONSE,
      &serde_json::json!({ "accepted": true }),
    )
    .await;
  });
  let mut session = CadderSession::connect(&peer.paths).await.unwrap();

  let error = session
    .request::<_, QueryStateResponse>(
      message_types::QUERY_STATE_REQUEST,
      message_types::QUERY_STATE_RESPONSE,
      &QueryStateRequest {
        request_id: new_request_id("bad-payload-query-state"),
      },
    )
    .await
    .unwrap_err();

  assert!(error.to_string().contains("missing field"), "{error:?}");
  peer.finish().await;
}

#[tokio::test]
async fn state_subscription_reports_eof_when_peer_closes_before_event() {
  let peer = ScriptedIpcPeer::start(|conn| async move {
    let (read_half, write_half) = tokio::io::split(conn);
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    drop(write_half);
  });
  let session = CadderSession::connect(&peer.paths).await.unwrap();
  let mut subscription = session
    .subscribe_state(new_request_id("eof-subscribe"))
    .await
    .unwrap();

  let error = subscription.next_event().await.unwrap_err();

  assert!(
    error
      .to_string()
      .contains("daemon closed the state subscription"),
    "{error:?}"
  );
  peer.finish().await;
}

#[tokio::test]
async fn state_subscription_rejects_non_event_message_type() {
  let peer = ScriptedIpcPeer::start(|conn| async move {
    let (_line, mut writer) = read_peer_request(conn).await;
    write_raw_envelope(
      &mut writer,
      message_types::QUERY_STATE_RESPONSE,
      &BasicResponse {
        request_id: "wrong-event-type".to_string(),
        accepted: true,
        message: "ok".to_string(),
      },
    )
    .await;
  });
  let session = CadderSession::connect(&peer.paths).await.unwrap();
  let mut subscription = session
    .subscribe_state(new_request_id("wrong-event-type"))
    .await
    .unwrap();

  let error = subscription.next_event().await.unwrap_err();

  assert!(
    error
      .to_string()
      .contains("unexpected response type `query-state-response`"),
    "{error:?}"
  );
  peer.finish().await;
}

#[tokio::test]
async fn state_subscription_rejects_invalid_event_payload() {
  let peer = ScriptedIpcPeer::start(|conn| async move {
    let (_line, mut writer) = read_peer_request(conn).await;
    write_raw_envelope(
      &mut writer,
      message_types::STATE_CHANGED_EVENT,
      &BasicResponse {
        request_id: "invalid-event-payload".to_string(),
        accepted: true,
        message: "ok".to_string(),
      },
    )
    .await;
  });
  let session = CadderSession::connect(&peer.paths).await.unwrap();
  let mut subscription = session
    .subscribe_state(new_request_id("invalid-event-payload"))
    .await
    .unwrap();

  let error = subscription.next_event().await.unwrap_err();

  assert!(error.to_string().contains("missing field"), "{error:?}");
  peer.finish().await;
}

#[tokio::test]
async fn ensure_daemon_running_returns_ok_when_socket_is_already_accepting() {
  let peer = ScriptedIpcPeer::start(|_conn| async move {});

  ensure_daemon_running(&peer.paths, None).await.unwrap();

  peer.finish().await;
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

async fn wait_for_empty_registrations(client: &CadderClient, failure_message: &str) {
  for _ in 0..50 {
    if query_state(client).await.registrations.is_empty() {
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  panic!("{failure_message}");
}

async fn wait_for_runtime_status(
  client: &CadderClient,
  status: RuntimeStatus,
) -> cadder_protocol::GuiStateSnapshot {
  for _ in 0..COMMAND_LOG_WAIT_ATTEMPTS {
    let snapshot = query_state(client).await;
    if snapshot.runtime.status == status {
      return snapshot;
    }
    sleep(Duration::from_millis(20)).await;
  }
  let snapshot = query_state(client).await;
  panic!(
    "expected runtime status {:?}, got {:?}",
    status, snapshot.runtime.status
  );
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

async fn wait_for_runtime_control_log(
  client: &CadderClient,
  operation: &str,
  message_fragment: &str,
) -> QueryLogsResponse {
  for _ in 0..COMMAND_LOG_WAIT_ATTEMPTS {
    let logs = query_logs(client, LogStreamIdentity::runtime_control(), Some(20), None).await;
    if logs.entries.iter().any(|entry| {
      entry.operation.as_deref() == Some(operation) && entry.raw_message.contains(message_fragment)
    }) {
      return logs;
    }
    sleep(Duration::from_millis(20)).await;
  }
  let logs = query_logs(client, LogStreamIdentity::runtime_control(), Some(20), None).await;
  panic!(
    "expected runtime control log operation `{operation}` containing `{message_fragment}`, got {:?}",
    logs.entries
  );
}

async fn query_history(
  client: &CadderClient,
  kind: cadder_protocol::HistoryKind,
) -> cadder_protocol::QueryHistoryResponse {
  client
    .request(
      message_types::QUERY_HISTORY_REQUEST,
      message_types::QUERY_HISTORY_RESPONSE,
      &cadder_protocol::QueryHistoryRequest {
        request_id: new_request_id("test-history"),
        kind: Some(kind),
        limit: Some(20),
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

async fn set_entrypoint_enabled(
  client: &CadderClient,
  registration_id: &str,
  shim_session_nonce: Option<&str>,
  enabled: bool,
) -> BasicResponse {
  client
    .request(
      message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
      message_types::SET_ENTRYPOINT_ENABLED_RESPONSE,
      &SetEntrypointEnabledRequest {
        request_id: new_request_id("test-entrypoint-toggle"),
        registration_id: registration_id.to_string(),
        shim_session_nonce: shim_session_nonce.map(str::to_string),
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

async fn raw_ipc_session(
  paths: &RuntimePaths,
) -> (
  BufReader<tokio::io::ReadHalf<Stream>>,
  tokio::io::WriteHalf<Stream>,
) {
  let name = paths
    .socket_name()
    .to_ns_name::<GenericNamespaced>()
    .unwrap();
  let conn = Stream::connect(name).await.unwrap();
  let (read_half, writer) = tokio::io::split(conn);
  (BufReader::new(read_half), writer)
}

async fn write_raw_envelope<W, T>(writer: &mut W, message_type: &str, payload: &T)
where
  W: AsyncWrite + Unpin,
  T: Serialize,
{
  let envelope = IpcEnvelope::new(message_type, payload).unwrap();
  let rendered = serde_json::to_string(&envelope).unwrap();
  writer.write_all(rendered.as_bytes()).await.unwrap();
  writer.write_all(b"\n").await.unwrap();
  writer.flush().await.unwrap();
}

async fn write_raw_line<W>(writer: &mut W, line: &str)
where
  W: AsyncWrite + Unpin,
{
  writer.write_all(line.as_bytes()).await.unwrap();
  writer.flush().await.unwrap();
}

async fn read_raw_envelope(reader: &mut BufReader<tokio::io::ReadHalf<Stream>>) -> IpcEnvelope {
  let mut line = String::new();
  reader.read_line(&mut line).await.unwrap();
  assert!(!line.is_empty(), "daemon closed the raw IPC session");
  serde_json::from_str(line.trim_end()).unwrap()
}

struct ScriptedIpcPeer {
  paths: RuntimePaths,
  task: JoinHandle<()>,
  _temp: tempfile::TempDir,
}

impl ScriptedIpcPeer {
  fn start<F, Fut>(handler: F) -> Self
  where
    F: FnOnce(Stream) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
  {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let name = paths
      .socket_name()
      .to_ns_name::<GenericNamespaced>()
      .unwrap();
    let listener = ListenerOptions::new()
      .name(name)
      .try_overwrite(true)
      .create_tokio()
      .unwrap();
    let task = tokio::spawn(async move {
      let conn = listener.accept().await.unwrap();
      handler(conn).await;
    });

    Self {
      paths,
      task,
      _temp: temp,
    }
  }

  async fn finish(self) {
    self.task.await.unwrap();
  }
}

async fn read_peer_request(conn: Stream) -> (String, tokio::io::WriteHalf<Stream>) {
  let (read_half, writer) = tokio::io::split(conn);
  let mut reader = BufReader::new(read_half);
  let mut line = String::new();
  reader.read_line(&mut line).await.unwrap();
  assert!(!line.is_empty(), "client did not send an IPC request");
  (line, writer)
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
    let adapter = fake_caddy
      .adapt_timeout
      .map(|timeout| CaddyConfigAdapter::with_command_timeout(resolver.clone(), timeout))
      .unwrap_or_else(|| CaddyConfigAdapter::new(resolver.clone()));
    let runtime = fake_caddy
      .runtime_timeouts
      .map(|timeouts| ProcessRuntime::with_timeouts(resolver.clone(), paths.clone(), timeouts))
      .unwrap_or_else(|| ProcessRuntime::new(resolver, paths.clone()));
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
  slow_adapt: bool,
  slow_reload: bool,
  slow_stop: bool,
  exit_run: bool,
  short_run: bool,
  adapt_timeout: Option<Duration>,
  runtime_timeouts: Option<RuntimeTimeouts>,
}

impl<'a> FakeCaddy<'a> {
  fn new(caddyfile: &'a str) -> Self {
    Self {
      caddyfile,
      fail_adapt: false,
      fail_reload: false,
      slow_adapt: false,
      slow_reload: false,
      slow_stop: false,
      exit_run: false,
      short_run: false,
      adapt_timeout: None,
      runtime_timeouts: None,
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

  fn slow_adapt(mut self, timeout: Duration) -> Self {
    self.slow_adapt = true;
    self.adapt_timeout = Some(timeout);
    self
  }

  fn slow_reload(mut self, timeouts: RuntimeTimeouts) -> Self {
    self.slow_reload = true;
    self.runtime_timeouts = Some(timeouts);
    self
  }

  fn slow_stop(mut self, timeouts: RuntimeTimeouts) -> Self {
    self.slow_stop = true;
    self.runtime_timeouts = Some(timeouts);
    self
  }

  fn exit_run(mut self) -> Self {
    self.exit_run = true;
    self
  }

  fn runtime_timeouts(mut self, timeouts: RuntimeTimeouts) -> Self {
    self.runtime_timeouts = Some(timeouts);
    self
  }

  fn short_run(mut self) -> Self {
    self.short_run = true;
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

const COMMAND_LOG_WAIT_ATTEMPTS: usize = 500;

async fn wait_for_command_log(path: &Path, command: &str) {
  for _ in 0..COMMAND_LOG_WAIT_ATTEMPTS {
    let log = fs::read_to_string(path).unwrap_or_default();
    if log.lines().any(|line| line.starts_with(command)) {
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  let log = fs::read_to_string(path).unwrap_or_default();
  panic!("expected command `{command}` in fake Caddy log:\n{log}");
}

async fn wait_for_command_count(path: &Path, command: &str, expected: usize) {
  for _ in 0..COMMAND_LOG_WAIT_ATTEMPTS {
    let log = fs::read_to_string(path).unwrap_or_default();
    let count = log
      .lines()
      .filter(|line| line.split_whitespace().next() == Some(command))
      .count();
    if count >= expected {
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  let log = fs::read_to_string(path).unwrap_or_default();
  panic!("expected {expected} `{command}` commands in fake Caddy log:\n{log}");
}

fn short_runtime_timeouts() -> RuntimeTimeouts {
  RuntimeTimeouts {
    start_check: Duration::from_millis(250),
    reload: Duration::from_millis(500),
    graceful_stop: Duration::from_millis(500),
    stop_wait: Duration::from_millis(500),
    kill_wait: Duration::from_secs(1),
  }
}

fn write_fake_caddy(dir: &Path, command_log_path: &Path, fake_caddy: FakeCaddy<'_>) -> PathBuf {
  const ADAPTED_JSON: &str = r#"{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["api.smarketing.localhost","app.smarketing.localhost","mailbox.smarketing.localhost","storage.smarketing.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}"#;
  let adapted_json = ADAPTED_JSON;
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
  let slow_adapt = if fake_caddy.slow_adapt {
    #[cfg(windows)]
    {
      "ping -n 2 127.0.0.1 >nul"
    }
    #[cfg(not(windows))]
    {
      "sleep 1"
    }
  } else {
    ""
  };
  let slow_reload = if fake_caddy.slow_reload {
    #[cfg(windows)]
    {
      "ping -n 2 127.0.0.1 >nul"
    }
    #[cfg(not(windows))]
    {
      "sleep 1"
    }
  } else {
    ""
  };
  let slow_stop = if fake_caddy.slow_stop {
    #[cfg(windows)]
    {
      "ping -n 2 127.0.0.1 >nul"
    }
    #[cfg(not(windows))]
    {
      "sleep 1"
    }
  } else {
    ""
  };
  let run_behavior = if fake_caddy.exit_run {
    #[cfg(windows)]
    {
      "exit /b 0".to_string()
    }
    #[cfg(not(windows))]
    {
      "exit 0".to_string()
    }
  } else if fake_caddy.short_run {
    #[cfg(windows)]
    {
      format!(
        "ping -n 2 127.0.0.1 >nul\r\n  echo run-exited>> \"{}\"\r\n  exit /b 0",
        command_log_path.display()
      )
    }
    #[cfg(not(windows))]
    {
      format!(
        "sleep 1\n  printf '%s\\n' 'run-exited' >> '{}'\n  exit 0",
        command_log_path.display()
      )
    }
  } else {
    #[cfg(windows)]
    {
      "ping -n 6 127.0.0.1 >nul\r\n  exit /b 0".to_string()
    }
    #[cfg(not(windows))]
    {
      "sleep 5\n  exit 0".to_string()
    }
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
  {slow_adapt}
  if not "{adapt_failure}"=="" (
    echo {adapt_failure} 1>&2
    exit 6
  )
  echo {adapted_json}
  exit 0
)
if "%1"=="reload" (
  {slow_reload}
  if not "{reload_failure}"=="" (
    echo {reload_failure} 1>&2
    exit 7
  )
  exit 0
)
if "%1"=="stop" (
  {slow_stop}
  exit 0
)
if "%1"=="run" (
  echo fake runtime started
  {run_behavior}
)
exit 0
"#,
        command_log = command_log_path.display(),
        adapted_json = adapted_json,
        adapt_failure = adapt_failure,
        reload_failure = reload_failure,
        slow_adapt = slow_adapt,
        slow_reload = slow_reload,
        slow_stop = slow_stop,
        run_behavior = run_behavior,
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
  {slow_adapt}
  if [ -n '{adapt_failure}' ]; then
    printf '%s\n' '{adapt_failure}' >&2
    exit 6
  fi
  printf '%s\n' '{adapted_json}'
  exit 0
fi
if [ "$1" = "reload" ]; then
  {slow_reload}
  if [ -n '{reload_failure}' ]; then
    printf '%s\n' '{reload_failure}' >&2
    exit 7
  fi
  exit 0
fi
if [ "$1" = "stop" ]; then
  {slow_stop}
  exit 0
fi
if [ "$1" = "run" ]; then echo fake runtime started; {run_behavior}; fi
exit 0
"#,
        command_log = command_log_path.display(),
        adapted_json = adapted_json,
        adapt_failure = adapt_failure,
        reload_failure = reload_failure,
        slow_adapt = slow_adapt,
        slow_reload = slow_reload,
        slow_stop = slow_stop,
        run_behavior = run_behavior,
      ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
  }
}
