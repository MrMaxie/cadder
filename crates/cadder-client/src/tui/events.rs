use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::app::{App, LifecycleAction};
use crate::data::MutationTarget;

pub(super) fn handle_event(event: Event, app: &mut App) -> Option<UiAction> {
  match event {
    Event::Key(key) if key.kind == KeyEventKind::Press => handle_key(key.code, key.modifiers, app),
    _ => None,
  }
}

fn handle_key(code: KeyCode, modifiers: KeyModifiers, app: &mut App) -> Option<UiAction> {
  if is_force_quit_key(code, modifiers) {
    app.quit();
    return None;
  }
  if matches!(code, KeyCode::Char('q' | 'Q')) {
    app.quit();
    return None;
  }

  if app.confirmation().is_some() {
    return match code {
      KeyCode::Esc => {
        app.cancel_confirmation();
        None
      }
      KeyCode::Enter => app.confirm_lifecycle().map(lifecycle_action),
      _ => None,
    };
  }

  match code {
    KeyCode::Esc => app.quit(),
    KeyCode::Up => app.select_previous(),
    KeyCode::Down => app.select_next(),
    KeyCode::Enter if app.prepare_start_daemon() => return Some(UiAction::StartDaemon),
    KeyCode::Enter => {
      return app.prepare_toggle_current().map(UiAction::Mutate);
    }
    KeyCode::Char('r') => return Some(UiAction::Refresh),
    KeyCode::Char('x' | 'X') => app.prepare_lifecycle(LifecycleAction::Stop),
    KeyCode::Char('R') => app.prepare_lifecycle(LifecycleAction::Restart),
    _ => {}
  }
  None
}

const fn lifecycle_action(action: LifecycleAction) -> UiAction {
  match action {
    LifecycleAction::Stop => UiAction::StopDaemon,
    LifecycleAction::Restart => UiAction::RestartDaemon,
  }
}

fn is_force_quit_key(code: KeyCode, modifiers: KeyModifiers) -> bool {
  modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c' | 'C' | 'x' | 'X'))
}

pub(super) enum UiAction {
  Refresh,
  StartDaemon,
  StopDaemon,
  RestartDaemon,
  Mutate(MutationTarget),
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::RefreshOutcome;
  use cadder_ipc::{
    ActivationState, EntrypointInstanceIdentity, EntrypointRegistration, LogStreamIdentity,
    OwnerProcessIdentity, SourcePath,
  };
  use chrono::Utc;

  fn registration() -> EntrypointRegistration {
    let now = Utc::now();
    EntrypointRegistration {
      registration_id: "entry-1".to_string(),
      entrypoint_instance: EntrypointInstanceIdentity {
        instance_id: "entry-1".to_string(),
        started_at_utc: now,
        shim_session_nonce: "nonce-1".to_string(),
      },
      source_working_directory: SourcePath::new("workspace/project-1", None),
      source_config_path: SourcePath::new("workspace/project-1/Caddyfile", None),
      registered_domains: Vec::new(),
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 42,
        process_start_time_utc: now,
        shim_session_nonce: "nonce-1".to_string(),
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint("entry-1"),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
    }
  }

  #[test]
  fn keys_cover_navigation_refresh_start_lifecycle_and_quit() {
    let mut app = App::new();

    assert!(matches!(
      handle_key(KeyCode::Char('r'), KeyModifiers::NONE, &mut app),
      Some(UiAction::Refresh)
    ));
    handle_key(KeyCode::Up, KeyModifiers::NONE, &mut app);
    handle_key(KeyCode::Down, KeyModifiers::NONE, &mut app);

    app.apply_refresh(RefreshOutcome::Unavailable {
      connection: crate::app::ConnectionStatus::Offline,
      message: "Cadder is not running.".to_string(),
      guidance: None,
    });
    assert!(matches!(
      handle_key(KeyCode::Enter, KeyModifiers::NONE, &mut app),
      Some(UiAction::StartDaemon)
    ));

    let mut toggle = App::new();
    toggle.apply_refresh(RefreshOutcome::Connected {
      snapshot: Box::new(cadder_ipc::GuiStateSnapshot {
        captured_at_utc: Utc::now(),
        registrations: vec![registration()],
        runtime: cadder_ipc::RuntimeState::idle(),
        config: cadder_ipc::ConfigState::idle(),
        storage: None,
      }),
    });
    assert!(handle_key(KeyCode::Char(' '), KeyModifiers::NONE, &mut toggle).is_none());
    assert!(!toggle.is_pending());
    assert!(matches!(
      handle_key(KeyCode::Enter, KeyModifiers::NONE, &mut toggle),
      Some(UiAction::Mutate(_))
    ));

    let mut connected = App::new();
    connected.apply_refresh(RefreshOutcome::Connected {
      snapshot: Box::new(cadder_ipc::GuiStateSnapshot {
        captured_at_utc: chrono::Utc::now(),
        registrations: Vec::new(),
        runtime: cadder_ipc::RuntimeState::idle(),
        config: cadder_ipc::ConfigState::idle(),
        storage: None,
      }),
    });
    handle_key(KeyCode::Char('x'), KeyModifiers::NONE, &mut connected);
    assert!(connected.confirmation().is_some());
    assert!(matches!(
      handle_key(KeyCode::Enter, KeyModifiers::NONE, &mut connected),
      Some(UiAction::StopDaemon)
    ));

    let mut quitting = App::new();
    assert!(handle_event(Event::Resize(100, 40), &mut quitting).is_none());
    handle_key(KeyCode::Char('q'), KeyModifiers::NONE, &mut quitting);
    assert!(quitting.should_quit());
  }
}
