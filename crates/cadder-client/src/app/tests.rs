use super::*;
use cadder_ipc::{ConfigState, GuiStateSnapshot, RuntimeState};
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
  });
  app
}

#[test]
fn runtime_status_reports_daemon_and_caddy_independently() {
  let running = RuntimeStatus {
    connection: ConnectionStatus::Connected,
    caddy_status: ProtocolRuntimeStatus::Running,
  };
  assert!(running.daemon_is_running());
  assert!(running.caddy_is_running());
  assert!(!running.daemon_is_offline());

  let offline = RuntimeStatus {
    connection: ConnectionStatus::Offline,
    caddy_status: ProtocolRuntimeStatus::Unknown,
  };
  assert!(!offline.daemon_is_running());
  assert!(!offline.caddy_is_running());
  assert!(offline.daemon_is_offline());
}

#[test]
fn offline_state_exposes_start_without_a_separate_status_view() {
  let mut app = App::new();
  app.apply_refresh(RefreshOutcome::Unavailable {
    connection: ConnectionStatus::Offline,
    message: "Cadder is not running.".to_string(),
    guidance: Some("Run `cadder tui --start-daemon`.".to_string()),
  });

  assert!(app.can_start_daemon());
  assert!(app.notice().is_none());
  assert!(app.prepare_start_daemon());
  assert!(app.is_pending());
}

#[test]
fn daemon_start_animates_without_repeating_failures_above_shortcuts() {
  let mut app = App::new();
  app.apply_refresh(RefreshOutcome::Unavailable {
    connection: ConnectionStatus::Offline,
    message: "Cadder is not running.".to_string(),
    guidance: None,
  });

  assert!(app.prepare_start_daemon());
  assert_eq!(app.pending_marker(), Some("⠋"));
  assert_eq!(app.notice().as_deref(), Some("Starting cadderd..."));
  app.advance_pending_animation();
  assert_eq!(app.pending_marker(), Some("⠙"));

  assert!(!app.complete_daemon_start(Err(ActionFailure {
    message: "Could not start cadderd.".to_string(),
    guidance: Some("Check the daemon status, then retry.".to_string()),
  })));
  assert!(app.pending_marker().is_none());
  assert_eq!(app.notice().as_deref(), Some("Could not start cadderd."));
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
