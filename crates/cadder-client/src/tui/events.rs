use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::app::{App, LifecycleAction, Tab};
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

  if app.details().is_some() {
    match code {
      KeyCode::Esc => app.close_details(),
      KeyCode::Enter => {
        return app.confirm_lifecycle().map(|action| match action {
          LifecycleAction::Stop => UiAction::StopDaemon,
          LifecycleAction::Restart => UiAction::RestartDaemon,
        });
      }
      KeyCode::Left | KeyCode::Up => app.previous_details_item(),
      KeyCode::Right | KeyCode::Down => app.next_details_item(),
      KeyCode::PageUp => app.page_details_up(),
      KeyCode::PageDown => app.page_details_down(),
      _ => {}
    }
    return None;
  }

  match code {
    KeyCode::Esc => app.quit(),
    KeyCode::Left | KeyCode::BackTab => app.previous_tab(),
    KeyCode::Right | KeyCode::Tab => app.next_tab(),
    KeyCode::Up if app.active_tab() == Tab::Logs => app.scroll_logs_up(1),
    KeyCode::Down if app.active_tab() == Tab::Logs => app.scroll_logs_down(1),
    KeyCode::PageUp if app.active_tab() == Tab::Logs => app.scroll_logs_up(10),
    KeyCode::PageDown if app.active_tab() == Tab::Logs => app.scroll_logs_down(10),
    KeyCode::Up => app.select_previous(),
    KeyCode::Down => app.select_next(),
    KeyCode::Enter | KeyCode::Char(' ') if app.prepare_start_daemon_from_status() => {
      return Some(UiAction::StartDaemon);
    }
    KeyCode::Char(' ') => return app.prepare_toggle_current().map(UiAction::Mutate),
    KeyCode::Char('r') => return Some(UiAction::Refresh),
    KeyCode::Char('x' | 'X') => app.prepare_lifecycle(LifecycleAction::Stop),
    KeyCode::Char('R') => app.prepare_lifecycle(LifecycleAction::Restart),
    KeyCode::Char('s' | 'S') if app.prepare_start_daemon() => {
      return Some(UiAction::StartDaemon);
    }
    KeyCode::Enter => app.open_details(),
    _ => {}
  }
  None
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

  #[test]
  fn keys_cover_navigation_refresh_start_lifecycle_and_quit() {
    let mut app = App::new();

    assert!(matches!(
      handle_key(KeyCode::Char('r'), KeyModifiers::NONE, &mut app),
      Some(UiAction::Refresh)
    ));
    handle_key(KeyCode::Tab, KeyModifiers::NONE, &mut app);
    handle_key(KeyCode::BackTab, KeyModifiers::NONE, &mut app);
    handle_key(KeyCode::Right, KeyModifiers::NONE, &mut app);
    handle_key(KeyCode::Left, KeyModifiers::NONE, &mut app);
    handle_key(KeyCode::Up, KeyModifiers::NONE, &mut app);
    handle_key(KeyCode::Down, KeyModifiers::NONE, &mut app);

    app.apply_refresh(RefreshOutcome::Unavailable {
      connection: crate::app::ConnectionStatus::Offline,
      message: "Cadder is not running.".to_string(),
      guidance: None,
    });
    app.next_tab();
    assert!(matches!(
      handle_key(KeyCode::Enter, KeyModifiers::NONE, &mut app),
      Some(UiAction::StartDaemon)
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
      logs: Ok(cadder_api::LogsView {
        stream: cadder_ipc::LogStreamIdentity::runtime_control(),
        stream_status: cadder_ipc::LogStreamStatus::Empty,
        entries: Vec::new(),
      }),
    });
    handle_key(KeyCode::Char('x'), KeyModifiers::NONE, &mut connected);
    assert!(connected.details().is_some());
    handle_key(KeyCode::Left, KeyModifiers::NONE, &mut connected);
    handle_key(KeyCode::Right, KeyModifiers::NONE, &mut connected);
    handle_key(KeyCode::PageUp, KeyModifiers::NONE, &mut connected);
    handle_key(KeyCode::PageDown, KeyModifiers::NONE, &mut connected);
    assert!(matches!(
      handle_key(KeyCode::Enter, KeyModifiers::NONE, &mut connected),
      Some(UiAction::StopDaemon)
    ));

    let mut quitting = App::new();
    assert!(handle_event(Event::Resize(100, 40), &mut quitting).is_none());
    handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL, &mut quitting);
    assert!(quitting.should_quit());
  }
}
