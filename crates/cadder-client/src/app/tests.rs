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

fn offline_app() -> App {
  let mut app = App::new();
  app.runtime_status = RuntimeStatus {
    connection: ConnectionStatus::Offline,
    caddy_status: ProtocolRuntimeStatus::Unknown,
  };
  app.set_active_tab(1);
  app
}

#[test]
fn runtime_status_labels_preserve_authoritative_caddy_states() {
  let label = |caddy_status| {
    RuntimeStatus {
      connection: ConnectionStatus::Connected,
      caddy_status,
    }
    .caddy_label()
  };

  assert_eq!(label(ProtocolRuntimeStatus::Unknown), "unknown");
  assert_eq!(label(ProtocolRuntimeStatus::NotResolved), "not resolved");
  assert_eq!(label(ProtocolRuntimeStatus::Resolved), "resolved");
  assert_eq!(label(ProtocolRuntimeStatus::Running), "running");
  assert_eq!(label(ProtocolRuntimeStatus::Unhealthy), "unhealthy");
  assert_eq!(label(ProtocolRuntimeStatus::Idle), "idle");
}

#[test]
fn offline_status_uses_dashes_instead_of_unavailable_internal_states() {
  let status = RuntimeStatus {
    connection: ConnectionStatus::Offline,
    caddy_status: ProtocolRuntimeStatus::Unknown,
  };

  assert_eq!(status.connection_label(), "-");
  assert_eq!(status.caddy_label(), "-");
}

#[test]
fn offline_copy_offers_the_status_start_action_without_runtime_details() {
  let (message, guidance) = unavailable_copy(ConnectionStateView::NotRunning);

  assert_eq!(message, "Cadder is not running.");
  assert_eq!(guidance, "Open Status and press Enter to start it.");
  assert!(!message.contains("runtime"));
  assert!(!guidance.contains("cadderd"));
}

#[test]
fn status_start_action_is_available_without_opening_a_modal() {
  let mut app = offline_app();

  assert_eq!(app.active_row_len(), 0);
  assert!(app.prepare_start_daemon_from_status());
  assert!(app.pending);
  assert!(app.details().is_none());
  assert!(app.is_starting_daemon());
}

#[test]
fn daemon_start_failure_keeps_the_reported_error_visible() {
  let mut app = offline_app();
  app.pending = true;

  app.complete_daemon_start(Err(ActionFailure {
    message: "cadderd exited before it became ready.".to_string(),
    guidance: Some("Configure the real Caddy executable, then retry.".to_string()),
  }));

  assert_eq!(
    app.status_message(),
    "cadderd exited before it became ready."
  );
  assert_eq!(
    app.connection_guidance.as_deref(),
    Some("Configure the real Caddy executable, then retry.")
  );
}

#[test]
fn status_start_action_is_unavailable_when_the_daemon_is_connected() {
  let mut app = offline_app();
  app.runtime_status.connection = ConnectionStatus::Connected;

  assert!(!app.prepare_start_daemon_from_status());
  assert!(!app.pending);
}

#[test]
fn connection_error_recovery_preserves_guidance_without_suggesting_daemon_start() {
  let (title, lines) = connection_recovery_details(
    ConnectionStatus::Error,
    "The protocol is incompatible.",
    Some("Upgrade the older Cadder component."),
  );

  assert_eq!(title, " Connection error ");
  assert_eq!(lines[1], "Upgrade the older Cadder component.");
  assert!(!lines.iter().any(|line| line.contains("Press s")));
}

#[test]
fn offline_recovery_offers_explicit_daemon_start() {
  let (title, lines) =
    connection_recovery_details(ConnectionStatus::Offline, "cadderd is not running.", None);

  assert_eq!(title, " Offline ");
  assert_eq!(lines[1], "Open Status and press Enter to start Cadder.");
  assert!(ConnectionStatus::Offline.can_start_daemon());
  assert!(!ConnectionStatus::Error.can_start_daemon());
}

#[test]
fn status_screen_shows_connection_failures_without_a_start_action() {
  let mut app = offline_app();
  app.runtime_status.connection = ConnectionStatus::Error;

  assert!(app.shows_status_screen());
  assert!(!app.can_start_daemon());

  app.runtime_status.connection = ConnectionStatus::Connected;
  assert!(!app.shows_status_screen());
}

#[test]
fn connected_lifecycle_actions_require_explicit_confirmation() {
  let mut app = offline_app();
  app.runtime_status.connection = ConnectionStatus::Connected;

  app.prepare_lifecycle(LifecycleAction::Stop);

  assert!(!app.pending);
  assert!(app.details().is_some());
  assert_eq!(app.confirm_lifecycle(), Some(LifecycleAction::Stop));
  assert!(app.pending);
}

#[test]
fn closing_lifecycle_confirmation_cancels_the_action() {
  let mut app = offline_app();
  app.runtime_status.connection = ConnectionStatus::Connected;
  app.prepare_lifecycle(LifecycleAction::Restart);

  app.close_details();

  assert!(app.details().is_none());
  assert_eq!(app.confirm_lifecycle(), None);
  assert!(!app.pending);
}

#[test]
fn connected_refresh_exposes_status_navigation_and_details() {
  let mut app = App::new();
  app.apply_refresh(RefreshOutcome::Connected {
    snapshot: Box::new(empty_snapshot()),
    logs: Ok(LogsView {
      stream: LogStreamIdentity::runtime_control(),
      stream_status: LogStreamStatus::Empty,
      entries: Vec::new(),
    }),
  });

  assert!(app.runtime_status().is_connected());
  assert_eq!(app.status_message(), "Connected");
  assert_eq!(app.connection_guidance(), None);
  assert_eq!(app.domain_rows().len(), 0);
  assert_eq!(app.log_lines(), ["No log entries available."]);
  assert!(!app.should_quit());

  app.next_tab();
  assert_eq!(app.active_tab(), Tab::Status);
  assert_eq!(app.active_row_len(), 4);
  assert_eq!(app.settings_rows().len(), 4);
  app.open_details();
  assert!(app.details().is_some());
  app.next_details_item();
  app.previous_details_item();
  app.page_details_down();
  app.page_details_up();
  app.close_details();

  app.next_tab();
  assert_eq!(app.active_tab(), Tab::Logs);
  app.set_logs_viewport(2);
  app.scroll_logs_down(10);
  app.scroll_logs_up(10);
  app.previous_tab();
  assert_eq!(app.active_tab(), Tab::Status);
  app.quit();
  assert!(app.should_quit());
}

#[test]
fn refresh_and_action_outcomes_cover_success_and_failure_states() {
  let mut app = App::new();
  app.apply_refresh(RefreshOutcome::Unavailable {
    connection: ConnectionStatus::Error,
    message: "Connection failed.".to_string(),
    guidance: None,
  });
  assert_eq!(app.status_message(), "Connection failed.");

  assert!(!app.complete_mutation(Err(ActionFailure {
    message: "Mutation failed.".to_string(),
    guidance: Some("Refresh and retry.".to_string()),
  })));
  assert_eq!(app.details().unwrap().title(), " Action failed ");
  app.close_details();

  assert!(app.complete_mutation(Ok(())));
  assert!(app.complete_daemon_start(Ok(())));
  assert!(app.complete_lifecycle(Ok(())));
  assert!(!app.complete_lifecycle(Err(ActionFailure {
    message: "Stop failed.".to_string(),
    guidance: None,
  })));
  assert_eq!(app.details().unwrap().lines(), ["Stop failed."]);
}
