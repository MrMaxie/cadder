use super::*;
use crate::{CaddyConfigCoordinator, RuntimePaths};
use cadder_ipc::{
  EntrypointInstanceIdentity, LogAttributionKind, LogSeverity, LogStreamIdentity, LogStreamStatus,
  OwnerProcessIdentity, QueryLogsPayload, RegisteredDomain, SetDomainEnabledPayload,
  SetEntrypointEnabledPayload, SourcePath,
};

struct Fixture {
  state: DaemonState,
  _temp: tempfile::TempDir,
}

fn fixture() -> Fixture {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  Fixture {
    state: DaemonState::new(CaddyConfigCoordinator::new_mock(paths)),
    _temp: temp,
  }
}

fn registration(id: &str, nonce: &str, domain: &str) -> EntrypointRegistration {
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
    registered_domains: vec![RegisteredDomain::active(domain)],
    activation_state: ActivationState::Active,
    owner_process: OwnerProcessIdentity {
      process_id: std::process::id(),
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
async fn starts_with_an_empty_snapshot() {
  let fixture = fixture();
  assert!(fixture.state.snapshot().await.registrations.is_empty());
}

#[tokio::test]
async fn register_heartbeat_and_unregister_preserve_session_ownership() {
  let fixture = fixture();
  let registered = fixture
    .state
    .register(registration("entry-1", "nonce-1", "app.localhost"))
    .await;
  assert!(registered.accepted, "{}", registered.message);

  let wrong_owner = fixture
    .state
    .heartbeat(HeartbeatEntrypointPayload {
      registration_id: "entry-1".to_string(),
      shim_session_nonce: "wrong".to_string(),
    })
    .await;
  assert!(!wrong_owner.accepted);

  let removed = fixture.state.unregister("entry-1", "nonce-1").await;
  assert!(removed.accepted, "{}", removed.message);
  assert!(fixture.state.snapshot().await.registrations.is_empty());
}

#[tokio::test]
async fn duplicate_active_domain_is_rejected() {
  let fixture = fixture();
  assert!(
    fixture
      .state
      .register(registration("entry-1", "nonce-1", "app.localhost"))
      .await
      .accepted
  );

  let conflict = fixture
    .state
    .register(registration("entry-2", "nonce-2", "APP.localhost"))
    .await;
  assert!(!conflict.accepted);
  assert_eq!(fixture.state.snapshot().await.registrations.len(), 1);
}

#[tokio::test]
async fn shutdown_signal_wakes_existing_and_future_waiters() {
  let signal = ShutdownSignal::default();
  let waiting = signal.clone();
  let waiter = tokio::spawn(async move { waiting.wait().await });
  tokio::task::yield_now().await;
  signal.request();
  waiter.await.unwrap();
  signal.wait().await;
}

#[tokio::test]
async fn activation_mutations_cover_success_missing_owner_and_domain_conflicts() {
  let fixture = fixture();
  assert!(
    fixture
      .state
      .register(registration("entry-1", "nonce-1", "app.localhost"))
      .await
      .accepted
  );

  let missing_entrypoint = fixture
    .state
    .set_entrypoint_enabled(SetEntrypointEnabledPayload {
      registration_id: "missing".to_string(),
      shim_session_nonce: None,
      enabled: false,
    })
    .await;
  assert!(!missing_entrypoint.accepted);

  let wrong_owner = fixture
    .state
    .set_entrypoint_enabled(SetEntrypointEnabledPayload {
      registration_id: "entry-1".to_string(),
      shim_session_nonce: Some("wrong".to_string()),
      enabled: false,
    })
    .await;
  assert!(!wrong_owner.accepted);

  for enabled in [false, true] {
    let response = fixture
      .state
      .set_entrypoint_enabled(SetEntrypointEnabledPayload {
        registration_id: "entry-1".to_string(),
        shim_session_nonce: Some("nonce-1".to_string()),
        enabled,
      })
      .await;
    assert!(response.accepted, "{}", response.message);
  }

  let missing_domain = fixture
    .state
    .set_domain_enabled(SetDomainEnabledPayload {
      registration_id: "entry-1".to_string(),
      domain_key: "missing.localhost".to_string(),
      enabled: false,
    })
    .await;
  assert!(!missing_domain.accepted);

  for enabled in [false, true] {
    let response = fixture
      .state
      .set_domain_enabled(SetDomainEnabledPayload {
        registration_id: "entry-1".to_string(),
        domain_key: "APP.LOCALHOST".to_string(),
        enabled,
      })
      .await;
    assert!(response.accepted, "{}", response.message);
  }

  let mut inactive_entrypoint = registration("entry-2", "nonce-2", "app.localhost");
  inactive_entrypoint.activation_state = ActivationState::Inactive;
  assert!(fixture.state.register(inactive_entrypoint).await.accepted);
  let conflict = fixture
    .state
    .set_entrypoint_enabled(SetEntrypointEnabledPayload {
      registration_id: "entry-2".to_string(),
      shim_session_nonce: None,
      enabled: true,
    })
    .await;
  assert!(!conflict.accepted);
  assert!(conflict.message.contains("already owned"));

  let mut inactive_domain = registration("entry-3", "nonce-3", "app.localhost");
  inactive_domain.registered_domains[0].activation_state = ActivationState::Inactive;
  assert!(fixture.state.register(inactive_domain).await.accepted);
  let conflict = fixture
    .state
    .set_domain_enabled(SetDomainEnabledPayload {
      registration_id: "entry-3".to_string(),
      domain_key: "app.localhost".to_string(),
      enabled: true,
    })
    .await;
  assert!(!conflict.accepted);
  assert!(conflict.message.contains("already owned"));
}

#[tokio::test]
async fn registration_rejects_invalid_or_replaced_owners_and_missing_cleanup() {
  let fixture = fixture();
  let mut invalid = registration("entry-1", "nonce-1", "app.localhost");
  invalid.owner_process.shim_session_nonce = "different".to_string();
  assert!(!fixture.state.register(invalid).await.accepted);

  assert!(
    fixture
      .state
      .register(registration("entry-1", "nonce-1", "app.localhost"))
      .await
      .accepted
  );
  let replacement = fixture
    .state
    .register(registration("entry-1", "nonce-2", "other.localhost"))
    .await;
  assert!(!replacement.accepted);
  assert!(replacement.message.contains("another shim session"));

  assert!(
    fixture
      .state
      .heartbeat(HeartbeatEntrypointPayload {
        registration_id: "entry-1".to_string(),
        shim_session_nonce: "nonce-1".to_string(),
      })
      .await
      .accepted
  );
  assert!(
    !fixture
      .state
      .heartbeat(HeartbeatEntrypointPayload {
        registration_id: "missing".to_string(),
        shim_session_nonce: "nonce-1".to_string(),
      })
      .await
      .accepted
  );
  assert!(!fixture.state.unregister("entry-1", "wrong").await.accepted);
  assert!(
    !fixture
      .state
      .unregister("missing", "nonce-1")
      .await
      .accepted
  );
}

#[tokio::test]
async fn log_queries_track_active_stale_removed_and_bounded_streams() {
  let fixture = fixture();
  let entrypoint_stream = LogStreamIdentity::entrypoint("entry-1");
  assert_eq!(
    fixture
      .state
      .query_logs(QueryLogsPayload {
        stream: entrypoint_stream.clone(),
        limit: None,
      })
      .await
      .stream_status,
    LogStreamStatus::Removed
  );

  assert!(
    fixture
      .state
      .register(registration("entry-1", "nonce-1", "app.localhost"))
      .await
      .accepted
  );
  assert_eq!(
    fixture
      .state
      .query_logs(QueryLogsPayload {
        stream: entrypoint_stream.clone(),
        limit: Some(0),
      })
      .await
      .stream_status,
    LogStreamStatus::Empty
  );

  for message in ["first", "second"] {
    fixture
      .state
      .logs()
      .append(
        entrypoint_stream.clone(),
        LogSeverity::Info,
        message,
        LogAttributionKind::Entrypoint,
        Some("test".to_string()),
      )
      .await;
  }
  let active = fixture
    .state
    .query_logs(QueryLogsPayload {
      stream: entrypoint_stream.clone(),
      limit: Some(1),
    })
    .await;
  assert_eq!(active.stream_status, LogStreamStatus::Active);
  assert_eq!(active.entries.len(), 1);
  assert_eq!(active.entries[0].raw_message, "second");

  assert!(
    fixture
      .state
      .set_entrypoint_enabled(SetEntrypointEnabledPayload {
        registration_id: "entry-1".to_string(),
        shim_session_nonce: None,
        enabled: false,
      })
      .await
      .accepted
  );
  assert_eq!(
    fixture
      .state
      .query_logs(QueryLogsPayload {
        stream: entrypoint_stream,
        limit: Some(500),
      })
      .await
      .stream_status,
    LogStreamStatus::Stale
  );
}

#[tokio::test]
async fn shutdown_drains_future_mutations_and_closes_memory_storage() {
  let fixture = fixture();
  let storage = fixture.state.storage_state();
  assert_eq!(storage.backend, "memory");
  assert_eq!(storage.schema_version, 0);

  let response = fixture.state.shutdown().await;
  assert!(response.accepted, "{}", response.message);
  fixture.state.shutdown_signal().wait().await;
  assert!(fixture.state.shutdown_signal().started_at().is_some());
  assert!(
    fixture
      .state
      .shutdown_storage_until(tokio::time::Instant::now())
      .await
      .is_ok()
  );

  let rejected = fixture
    .state
    .register(registration("entry-1", "nonce-1", "app.localhost"))
    .await;
  assert!(!rejected.accepted);
  assert!(rejected.message.contains("drain"));
}

#[tokio::test]
async fn bounded_shutdown_reports_lock_deadlines_and_quiescent_success() {
  let blocked_operation = fixture();
  let _operation = blocked_operation
    .state
    .config_operation
    .acquire()
    .await
    .unwrap();
  let timed_out = blocked_operation
    .state
    .prepare_shutdown_until(tokio::time::Instant::now() + std::time::Duration::from_millis(5))
    .await;
  assert!(!timed_out.response.accepted);
  assert!(!timed_out.runtime_quiescent);
  assert!(timed_out.response.message.contains("runtime operation"));

  let blocked_coordinator = fixture();
  let _coordinator = blocked_coordinator.state.coordinator.lock().await;
  let timed_out = blocked_coordinator
    .state
    .prepare_shutdown_until(tokio::time::Instant::now() + std::time::Duration::from_millis(5))
    .await;
  assert!(!timed_out.response.accepted);
  assert!(!timed_out.runtime_quiescent);
  assert!(timed_out.response.message.contains("runtime ownership"));

  let successful = fixture();
  let prepared_at = tokio::time::Instant::now();
  successful.state.prepare_shutdown_at(prepared_at);
  assert_eq!(
    successful.state.shutdown_signal().started_at(),
    Some(prepared_at)
  );
  assert!(successful.state.force_stop_runtime().await.is_ok());
  let prepared = successful
    .state
    .prepare_shutdown_until(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
    .await;
  assert!(prepared.response.accepted, "{}", prepared.response.message);
  assert!(prepared.runtime_quiescent);
  successful.state.request_shutdown();
  successful.state.shutdown_signal().wait().await;
}
