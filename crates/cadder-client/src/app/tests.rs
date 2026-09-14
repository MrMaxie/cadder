use super::*;
use cadder_ipc::{ConfigState, GuiStateSnapshot, LogStreamStatus, RuntimeState};
use chrono::Utc;

fn empty_snapshot() -> GuiStateSnapshot {
  GuiStateSnapshot {
    captured_at_utc: Utc::now(),
    registrations: Vec::new(),
    runtime: RuntimeState::idle(),
    config: ConfigState::idle(),
    storage: None,
  }
}

fn connected_app() -> App {
  let mut app = App::new();
  app.apply_refresh(RefreshOutcome::Connected {
    snapshot: Box::new(empty_snapshot()),
    logs: Ok(LogsView {
      stream: LogStreamIdentity::runtime_control(),
      stream_status: LogStreamStatus::Empty,
      entries: Vec::new(),
    }),
  });
  app
}

#[test]
fn runtime_status_keeps_global_health_compact() {
  let running = RuntimeStatus {
    connection: ConnectionStatus::Connected,
    caddy_status: ProtocolRuntimeStatus::Running,
  };
  assert_eq!(running.service_label(), "Caddy");
  assert_eq!(running.state_label(), "running");
  assert!(running.is_healthy());

  let offline = RuntimeStatus {
    connection: ConnectionStatus::Offline,
    caddy_status: ProtocolRuntimeStatus::Unknown,
  };
  assert_eq!(offline.service_label(), "Cadder");
  assert_eq!(offline.state_label(), "offline");
  assert!(!offline.is_healthy());
}

#[test]
fn offline_state_exposes_start_without_a_separate_status_view() {
  let mut app = App::new();
  app.apply_refresh(RefreshOutcome::Unavailable {
    connection: ConnectionStatus::Offline,
    message: "Cadder is not running.".to_string(),
    guidance: Some("Press Enter to start it.".to_string()),
  });

  assert!(app.can_start_daemon());
  assert_eq!(
    app.notice().as_deref(),
    Some("Cadder is not running.  Press Enter to start it.")
  );
  assert!(app.prepare_start_daemon());
  assert!(app.is_pending());
}

#[test]
fn lifecycle_confirmation_stays_inline_and_explicit() {
  let mut app = connected_app();
  app.prepare_lifecycle(LifecycleAction::Restart);

  assert_eq!(app.confirmation(), Some(LifecycleAction::Restart));
  assert!(app.notice().unwrap().contains("Enter confirm"));
  app.cancel_confirmation();
  assert_eq!(app.confirmation(), None);

  app.prepare_lifecycle(LifecycleAction::Stop);
  assert_eq!(app.confirm_lifecycle(), Some(LifecycleAction::Stop));
  assert!(app.is_pending());
}

#[test]
fn logs_are_contextual_and_toggle_without_changing_views() {
  let mut app = connected_app();
  assert!(!app.logs_open());
  assert!(app.toggle_logs());
  assert_eq!(app.log_title(), "Logs");
  app.set_logs_viewport(2);
  app.scroll_logs_down(10);
  app.scroll_logs_up(10);
  assert!(!app.toggle_logs());
}

#[test]
fn action_failures_use_the_inline_notice() {
  let mut app = connected_app();
  assert!(!app.complete_mutation(Err(ActionFailure {
    message: "Mutation failed.".to_string(),
    guidance: Some("Refresh and retry.".to_string()),
  })));
  assert_eq!(
    app.notice().as_deref(),
    Some("Mutation failed.  Refresh and retry.")
  );

  assert!(app.complete_mutation(Ok(())));
  assert!(app.notice().is_none());
  app.quit();
  assert!(app.should_quit());
}
