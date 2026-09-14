mod effects;
mod events;
mod view;

use std::time::Duration;

use cadder_api::OperatorContext;
use color_eyre::Result;
use crossterm::event::EventStream;
use futures_util::StreamExt;
use ratatui::DefaultTerminal;

use self::effects::{Effect, EffectResult, spawn_effect};
use self::events::{UiAction, handle_event};
use self::view::render;
use crate::app::App;

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

pub async fn run(context: OperatorContext) -> Result<()> {
  let mut terminal = ratatui::try_init()?;
  let _terminal_guard = TerminalRestoreGuard;
  let mut app = App::new();
  run_event_loop(&mut terminal, &mut app, context).await
}

async fn run_event_loop(
  terminal: &mut DefaultTerminal,
  app: &mut App,
  context: OperatorContext,
) -> Result<()> {
  let mut events = EventStream::new();
  let mut refresh = tokio::time::interval(REFRESH_INTERVAL);
  refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
  let mut effects = tokio::task::JoinSet::new();
  let mut refresh_in_flight = true;
  spawn_effect(
    &mut effects,
    context.clone(),
    Effect::Refresh(app.log_stream()),
  );

  while !app.should_quit() {
    terminal.draw(|frame| render(frame, app))?;

    tokio::select! {
      _ = refresh.tick(), if !refresh_in_flight && !app.is_pending() => {
        refresh_in_flight = true;
        spawn_effect(&mut effects, context.clone(), Effect::Refresh(app.log_stream()));
      },
      event = events.next() => {
        match event.transpose()? {
          Some(event) => {
            if let Some(action) = handle_event(event, app) {
              if matches!(action, UiAction::Refresh) {
                refresh_in_flight = true;
              }
              spawn_effect(&mut effects, context.clone(), Effect::from(action, app.log_stream()));
            }
          }
          None => app.quit(),
        }
      },
      result = effects.join_next(), if !effects.is_empty() => {
        match result.expect("guarded effect task")? {
          EffectResult::Refresh(outcome) => {
            refresh_in_flight = false;
            app.apply_refresh(outcome);
          }
          EffectResult::Start(result) => {
            if app.complete_daemon_start(result) && !refresh_in_flight {
              refresh_in_flight = true;
              spawn_effect(&mut effects, context.clone(), Effect::Refresh(app.log_stream()));
            }
          }
          EffectResult::Mutation(result) => {
            if app.complete_mutation(result) && !refresh_in_flight {
              refresh_in_flight = true;
              spawn_effect(&mut effects, context.clone(), Effect::Refresh(app.log_stream()));
            }
          }
          EffectResult::Lifecycle(result) => {
            if app.complete_lifecycle(result) && !refresh_in_flight {
              refresh_in_flight = true;
              spawn_effect(&mut effects, context.clone(), Effect::Refresh(app.log_stream()));
            }
          }
        }
      }
    }
  }

  effects.shutdown().await;
  Ok(())
}

struct TerminalRestoreGuard;

impl Drop for TerminalRestoreGuard {
  fn drop(&mut self) {
    let _ = ratatui::try_restore();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use ratatui::Terminal;
  use ratatui::backend::TestBackend;

  #[test]
  fn initial_screen_fits_the_minimum_supported_viewport() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new();

    terminal.draw(|frame| render(frame, &mut app)).unwrap();

    let screen = format!("{}", terminal.backend());
    assert!(screen.contains("Cadder v"));
    assert!(screen.contains("Project / domain"));
    assert!(screen.contains("Connecting to Cadder"));
  }

  #[test]
  fn renderer_covers_offline_logs_confirmation_and_tiny_terminals() {
    let mut app = App::new();
    app.apply_refresh(crate::app::RefreshOutcome::Unavailable {
      connection: crate::app::ConnectionStatus::Offline,
      message: "Cadder is not running.".to_string(),
      guidance: Some("Press Enter to start it.".to_string()),
    });

    let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
    terminal.draw(|frame| render(frame, &mut app)).unwrap();
    assert!(format!("{}", terminal.backend()).contains("Press Enter to start it"));

    app.toggle_logs();
    terminal.draw(|frame| render(frame, &mut app)).unwrap();
    assert!(format!("{}", terminal.backend()).contains("Logs"));

    let mut connected = App::new();
    connected.apply_refresh(crate::app::RefreshOutcome::Connected {
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
    connected.prepare_lifecycle(crate::app::LifecycleAction::Restart);
    terminal
      .draw(|frame| render(frame, &mut connected))
      .unwrap();
    assert!(format!("{}", terminal.backend()).contains("Restart Cadder"));

    let mut tiny = Terminal::new(TestBackend::new(1, 1)).unwrap();
    tiny.draw(|frame| render(frame, &mut App::new())).unwrap();
  }
}
