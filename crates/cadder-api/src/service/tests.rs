use super::*;
use cadder_ipc::{
  ActivationState, ConfigState, DomainName, EntrypointInstanceIdentity, EntrypointRegistration,
  HeartbeatEntrypointPayload, OwnerProcessIdentity, ProtocolError, ProtocolErrorCode,
  ProtocolErrorKind, RegisterEntrypointPayload, RegisteredDomain, RuntimeState, SourcePath,
  UnregisterEntrypointPayload, new_request_id,
};
use chrono::{TimeZone, Utc};
use tokio::sync::watch;
use tokio::time::{Duration, sleep};

fn timestamp() -> chrono::DateTime<Utc> {
  Utc
    .with_ymd_and_hms(2026, 6, 17, 11, 30, 0)
    .single()
    .unwrap()
}

fn registration(id: &str, domains: &[&str]) -> EntrypointRegistration {
  let now = timestamp();
  let identity = EntrypointInstanceIdentity {
    instance_id: id.to_string(),
    started_at_utc: now,
    shim_session_nonce: format!("{id}-nonce"),
  };
  EntrypointRegistration {
    registration_id: id.to_string(),
    entrypoint_instance: identity.clone(),
    source_working_directory: SourcePath::new("D:/Projects/App", None),
    source_config_path: SourcePath::new("D:/Projects/App/Caddyfile", None),
    registered_domains: domains
      .iter()
      .map(|domain| RegisteredDomain {
        name: DomainName::parse(*domain),
        activation_state: ActivationState::Active,
        upstream: None,
        log_stream: LogStreamIdentity::domain(domain),
      })
      .collect(),
    activation_state: ActivationState::Active,
    owner_process: OwnerProcessIdentity {
      process_id: 42,
      process_start_time_utc: now,
      shim_session_nonce: identity.shim_session_nonce.clone(),
      executable_path: Some("caddy.exe".to_string()),
    },
    log_stream: LogStreamIdentity::entrypoint(id),
    shim_run: None,
    created_at_utc: now,
    last_heartbeat_utc: now,
  }
}

fn snapshot(registrations: Vec<EntrypointRegistration>) -> GuiStateSnapshot {
  GuiStateSnapshot {
    captured_at_utc: timestamp(),
    registrations,
    runtime: RuntimeState::idle(),
    config: ConfigState::idle(),
    storage: None,
  }
}

fn context() -> (tempfile::TempDir, OperatorContext) {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  (
    temp,
    OperatorContext::from_paths(paths, DaemonLaunchOptions::default()),
  )
}

#[test]
fn context_preserves_runtime_paths_and_launch_options() {
  let temp = tempfile::tempdir().unwrap();
  let runtime_dir = temp.path().join("runtime");
  let daemon_path = temp.path().join("cadderd.exe");
  let context = OperatorContext::new(
    "test",
    Some(runtime_dir.clone()),
    DaemonLaunchOptions {
      explicit_daemon: Some(daemon_path.clone()),
      real_caddy_override: Some(PathBuf::from("caddy-real")),
      launch_mode: cadder_daemon::DaemonLaunchMode::ForegroundDiagnostic,
      ..DaemonLaunchOptions::default()
    },
  )
  .unwrap();

  assert_eq!(context.paths().runtime_dir(), runtime_dir.as_path());
  assert_eq!(
    context.launch_options().explicit_daemon.as_deref(),
    Some(daemon_path.as_path())
  );
  assert_eq!(
    context.launch_options().real_caddy_override.as_deref(),
    Some(std::path::Path::new("caddy-real"))
  );
  assert_eq!(
    context.launch_options().launch_mode,
    cadder_daemon::DaemonLaunchMode::ForegroundDiagnostic
  );
}

#[tokio::test]
async fn typed_error_operator_preserves_local_ipc_source_and_machine_fields() {
  let (_temp, context) = context();

  let error = context
    .query_snapshot("status", "query state")
    .await
    .unwrap_err();

  let ipc_error = std::error::Error::source(&error)
    .and_then(|source| source.downcast_ref::<Box<IpcClientError>>())
    .map(Box::as_ref)
    .expect("operator error should retain the typed IPC error");
  let local = ipc_error
    .local_error()
    .expect("missing daemon should remain a local IPC error");
  assert_eq!(local.phase(), cadder_daemon::IpcClientPhase::Connect);
  assert!(std::error::Error::source(local).is_some());

  let json = serde_json::to_value(&error).unwrap();
  assert_eq!(json["local_error"]["code"], local.code().as_str());
  assert_eq!(json["local_error"]["message"], local.message());
  assert_eq!(
    json["local_error"]["guidance"],
    local
      .guidance()
      .expect("connection error should include guidance")
  );
  assert_eq!(json["local_error"]["retryable"], local.retryable());
}

#[test]
fn select_domain_returns_precise_operator_errors() {
  let (_temp, context) = context();
  let snapshot = snapshot(vec![
    registration("shim-1", &["app.localhost"]),
    registration("shim-2", &["app.localhost", "api.localhost"]),
  ]);

  let selected = context
    .select_domain(
      "domains enable",
      &snapshot,
      &DomainSelector {
        domain: "api.localhost".to_string(),
        registration: None,
      },
    )
    .unwrap();
  assert_eq!(selected.registration_id, "shim-2");
  assert_eq!(selected.canonical_domain, "api.localhost");

  let missing = context
    .select_domain(
      "domains enable",
      &snapshot,
      &DomainSelector {
        domain: "missing.localhost".to_string(),
        registration: Some("shim-1".to_string()),
      },
    )
    .unwrap_err();
  assert_eq!(missing.kind, crate::AppExit::TargetNotFound);
  assert!(missing.message.contains("shim-1"));

  let missing_any_entrypoint = context
    .select_domain(
      "domains enable",
      &snapshot,
      &DomainSelector {
        domain: "missing.localhost".to_string(),
        registration: None,
      },
    )
    .unwrap_err();
  assert_eq!(missing_any_entrypoint.kind, crate::AppExit::TargetNotFound);
  assert_eq!(
    missing_any_entrypoint.message,
    "Domain `missing.localhost` was not found."
  );

  let ambiguous = context
    .select_domain(
      "domains enable",
      &snapshot,
      &DomainSelector {
        domain: "app.localhost".to_string(),
        registration: None,
      },
    )
    .unwrap_err();
  assert_eq!(ambiguous.kind, crate::AppExit::ConflictOrRejected);
  assert!(ambiguous.message.contains("shim-1, shim-2"));
}

#[test]
fn selector_and_logs_target_contracts_are_stable() {
  let selector = DomainSelector {
    domain: "App.Localhost".to_string(),
    registration: Some("shim-1".to_string()),
  };
  let target = LogsTarget::Domain(selector.clone());

  assert_eq!(selector.clone(), selector);
  assert_eq!(target.clone(), LogsTarget::Domain(selector));
  assert!(format!("{target:?}").contains("Domain"));
  assert_eq!(LogsTarget::Runtime, LogsTarget::Runtime.clone());
  assert_eq!(
    LogsTarget::Entrypoint {
      registration_id: "shim-1".to_string(),
    },
    LogsTarget::Entrypoint {
      registration_id: "shim-1".to_string(),
    }
    .clone()
  );
}

#[tokio::test]
async fn connected_and_unavailable_statuses_include_operator_context() {
  let (_temp, context) = context();
  let status = connected_status(
    &context,
    &snapshot(vec![registration(
      "shim-1",
      &["app.localhost", "api.localhost"],
    )]),
  );

  assert_eq!(status.connection_state, ConnectionStateView::Connected);
  assert_eq!(status.counts.entrypoints, 1);
  assert_eq!(status.counts.domains, 2);
  assert_eq!(status.counts.active_domains, 2);

  let not_running = context.query_state_response().await.unwrap_err();
  let status = unavailable_status(&context, &not_running);
  assert_eq!(status.connection_state, ConnectionStateView::NotRunning);
  assert!(status.message.contains("is not running"));
  assert!(status.guidance.as_deref().is_some_and(|guidance| {
    guidance
      == "Run `cadder tui --start-daemon` to start cadderd and open the operator, then retry."
  }));

  let failed = IpcClientError::Daemon(ProtocolError::new(
    ProtocolErrorKind::IncompatibleProtocolVersion,
    ProtocolErrorCode::parse("incompatible_protocol").unwrap(),
    "protocol mismatch",
    Some("Upgrade the older Cadder component.".into()),
    false,
  ));
  let status = unavailable_status(&context, &failed);
  assert_eq!(
    status.connection_state,
    ConnectionStateView::ConnectionFailed
  );
  assert!(status.message.contains("protocol mismatch"));
  assert_eq!(
    status.guidance.as_deref(),
    Some("Upgrade the older Cadder component.")
  );
  assert!(!status.guidance.unwrap().contains("daemon start"));

  let stale = IpcClientError::Daemon(ProtocolError::new(
    ProtocolErrorKind::StaleInstance,
    ProtocolErrorCode::parse("stale_instance").unwrap(),
    "the published daemon instance is no longer reachable",
    None,
    true,
  ));
  assert_eq!(
    connection_state_from_error(&stale),
    ConnectionStateView::NotRunning
  );
}

#[tokio::test]
async fn operator_context_drives_the_complete_daemon_protocol() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  paths.ensure_dirs().unwrap();
  let state = cadder_daemon::DaemonState::new(cadder_daemon::CaddyConfigCoordinator::new_mock(
    paths.clone(),
  ));
  let server = cadder_daemon::DaemonServer::new(paths.clone(), state);
  let (_shutdown_tx, shutdown_rx) = watch::channel(false);
  let server_task = tokio::spawn(server.run_until(shutdown_rx));

  let mut session = loop {
    match cadder_daemon::CadderSession::connect(&paths).await {
      Ok(session) => break session,
      Err(_) => sleep(Duration::from_millis(10)).await,
    }
  };
  assert_eq!(
    session.negotiated_version(),
    cadder_ipc::CURRENT_PROTOCOL_VERSION
  );

  let context = OperatorContext::from_paths(paths, DaemonLaunchOptions::default());
  assert!(
    context
      .query_snapshot("test", "query empty state")
      .await
      .unwrap()
      .registrations
      .is_empty()
  );

  let registered = session
    .request(
      new_request_id("register"),
      &RegisterEntrypointPayload {
        registration: registration("entry-1", &["App.Localhost", "api.localhost"]),
      },
    )
    .await
    .unwrap();
  assert!(registered.accepted, "{}", registered.message);

  let snapshot = context.query_snapshot("test", "query state").await.unwrap();
  assert_eq!(snapshot.registrations.len(), 1);
  let status = connected_status(&context, &snapshot);
  assert_eq!(status.counts.domains, 2);

  assert_eq!(
    context
      .resolve_logs_target("test", LogsTarget::Runtime)
      .await
      .unwrap(),
    LogStreamIdentity::runtime_control()
  );
  assert_eq!(
    context
      .resolve_logs_target(
        "test",
        LogsTarget::Entrypoint {
          registration_id: "entry-1".to_string(),
        },
      )
      .await
      .unwrap(),
    LogStreamIdentity::entrypoint("entry-1")
  );
  assert_eq!(
    context
      .resolve_logs_target(
        "test",
        LogsTarget::Domain(DomainSelector {
          domain: "APP.LOCALHOST".to_string(),
          registration: None,
        }),
      )
      .await
      .unwrap(),
    LogStreamIdentity::domain("App.Localhost")
  );

  let runtime_logs = context
    .query_logs(
      "test",
      "query logs",
      LogStreamIdentity::runtime_control(),
      50,
    )
    .await
    .unwrap();
  assert_eq!(runtime_logs.stream, LogStreamIdentity::runtime_control());

  assert!(
    context
      .set_entrypoint_enabled("test", "entry-1".into(), false)
      .await
      .unwrap()
      .accepted
  );
  assert!(
    context
      .set_entrypoint_enabled("test", "entry-1".into(), true)
      .await
      .unwrap()
      .accepted
  );
  let selector = DomainSelector {
    domain: "api.localhost".to_string(),
    registration: Some("entry-1".to_string()),
  };
  assert!(
    context
      .set_domain_enabled("test", &selector, false)
      .await
      .unwrap()
      .accepted
  );
  assert!(
    context
      .set_domain_enabled("test", &selector, true)
      .await
      .unwrap()
      .accepted
  );

  let heartbeat = context
    .request_basic(
      "test",
      "heartbeat",
      new_request_id("heartbeat"),
      &HeartbeatEntrypointPayload {
        registration_id: "entry-1".to_string(),
        shim_session_nonce: "entry-1-nonce".to_string(),
      },
    )
    .await
    .unwrap();
  assert!(heartbeat.accepted);

  let unregistered = context
    .request_basic(
      "test",
      "unregister",
      new_request_id("unregister"),
      &UnregisterEntrypointPayload {
        registration_id: "entry-1".to_string(),
        shim_session_nonce: "entry-1-nonce".to_string(),
      },
    )
    .await
    .unwrap();
  assert!(unregistered.accepted);
  assert!(
    context
      .query_snapshot("test", "query final state")
      .await
      .unwrap()
      .registrations
      .is_empty()
  );

  context.stop_daemon("test").await.unwrap();
  server_task.await.unwrap().unwrap();
}
