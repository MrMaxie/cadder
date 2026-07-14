mod app;
mod data;
mod logs;
mod widgets;

use std::io;
use std::process::ExitCode;
use std::time::Duration;

use app::App;
use cadder_daemon::DaemonLaunchOptions;
use cadder_operator::OperatorContext;
use clap::{Parser, Subcommand, error::ErrorKind};
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::widgets::StatefulWidget;
use ratatui::{DefaultTerminal, Frame};
use widgets::{
  AppTabs, DetailOverlay, DimBackground, DomainsTab, HeaderBar, LOG_SHORTCUTS, LogsTab,
  MAIN_SHORTCUTS, OFFLINE_SHORTCUTS, STARTING_SHORTCUTS, SettingsTab, ShortcutsBar, StatusTab,
};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Parser)]
#[command(
  name = "cadder",
  version,
  about = "Cadder operator",
  arg_required_else_help = true
)]
struct Cli {
  #[command(subcommand)]
  command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
  /// Open the full-screen operator.
  Tui,
}

fn main() -> ExitCode {
  if let Err(error) = color_eyre::install() {
    eprintln!("Could not initialize Cadder error reporting: {error}");
    return ExitCode::from(9);
  }

  match Cli::try_parse() {
    Ok(Cli {
      command: Command::Tui,
    }) => match run_tui() {
      Ok(()) => ExitCode::SUCCESS,
      Err(error) => {
        eprintln!("Could not run the Cadder TUI: {error}");
        ExitCode::from(9)
      }
    },
    Err(error) => {
      let exit_code = cli_error_exit_code(&error);
      let _ = error.print();
      exit_code
    }
  }
}

fn cli_error_exit_code(error: &clap::Error) -> ExitCode {
  if error.kind() == ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand {
    ExitCode::SUCCESS
  } else {
    ExitCode::from(error.exit_code() as u8)
  }
}

fn run_tui() -> Result<()> {
  let context = OperatorContext::new("tui", None, DaemonLaunchOptions::default())?;
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()?;
  let mut app = App::new(context);
  runtime.block_on(app.refresh());

  let mut terminal = ratatui::try_init()?;
  let _terminal_guard = TerminalRestoreGuard;
  run(&runtime, &mut terminal, &mut app)
}

fn run(
  runtime: &tokio::runtime::Runtime,
  terminal: &mut DefaultTerminal,
  app: &mut App,
) -> Result<()> {
  let mut daemon_start: Option<tokio::task::JoinHandle<std::result::Result<(), OperatorError>>> =
    None;

  while !app.should_quit() {
    runtime.block_on(app.refresh_if_due());
    runtime.block_on(tokio::task::yield_now());
    if daemon_start.as_ref().is_some_and(|task| task.is_finished())
      && let Some(task) = daemon_start.take()
    {
      let result = runtime.block_on(task)?;
      runtime.block_on(app.complete_daemon_start(result));
    }
    terminal.draw(|frame| render(frame, app))?;

    if event::poll(Duration::from_millis(16))? {
      match handle_event(event::read()?, app)? {
        Some(UiAction::Refresh) => runtime.block_on(app.refresh()),
        Some(UiAction::StartDaemon) => {
          let context = app.daemon_start_context();
          daemon_start =
            Some(runtime.spawn(async move { context.ensure_daemon_running("tui").await }));
        }
        Some(UiAction::Mutate(target)) => {
          terminal.draw(|frame| render(frame, app))?;
          runtime.block_on(app.complete_mutation(target));
        }
        None => {}
      }
    }
  }

  Ok(())
}

fn handle_event(event: Event, app: &mut App) -> io::Result<Option<UiAction>> {
  match event {
    Event::Key(key) if key.kind == KeyEventKind::Press => {
      Ok(handle_key(key.code, key.modifiers, app))
    }
    _ => Ok(None),
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
    KeyCode::Up if app.active_tab() == 2 => app.scroll_logs_up(1),
    KeyCode::Down if app.active_tab() == 2 => app.scroll_logs_down(1),
    KeyCode::PageUp if app.active_tab() == 2 => app.scroll_logs_up(10),
    KeyCode::PageDown if app.active_tab() == 2 => app.scroll_logs_down(10),
    KeyCode::Up => app.select_previous(),
    KeyCode::Down => app.select_next(),
    KeyCode::Enter | KeyCode::Char(' ') if app.prepare_start_daemon_from_status() => {
      return Some(UiAction::StartDaemon);
    }
    KeyCode::Char(' ') => return app.prepare_toggle_current().map(UiAction::Mutate),
    KeyCode::Char('r' | 'R') => return Some(UiAction::Refresh),
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

enum UiAction {
  Refresh,
  StartDaemon,
  Mutate(crate::data::MutationTarget),
}

struct TerminalRestoreGuard;

impl Drop for TerminalRestoreGuard {
  fn drop(&mut self) {
    let _ = ratatui::try_restore();
  }
}

fn render(frame: &mut Frame<'_>, app: &mut App) {
  let root = frame.area();
  let (main_area, footer_area) = root_areas(root);

  render_main(frame, main_area, app);

  if app.details().is_some() {
    frame.render_widget(DimBackground, main_area);
  }
  if let Some(details) = app.details() {
    frame.render_widget(
      DetailOverlay::new(details.title(), details.lines(), details.scroll()),
      main_area,
    );
  }

  let shortcuts = if app.details().is_some() {
    app::DETAIL_SHORTCUTS.as_slice()
  } else if app.is_starting_daemon() {
    STARTING_SHORTCUTS.as_slice()
  } else if app.can_start_daemon() {
    OFFLINE_SHORTCUTS.as_slice()
  } else if app.active_tab() == 2 {
    LOG_SHORTCUTS.as_slice()
  } else {
    MAIN_SHORTCUTS.as_slice()
  };
  frame.render_widget(ShortcutsBar::new(shortcuts), footer_area);
}

fn render_main(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
  if area.height == 0 {
    return;
  }

  let (header_area, tabs_area, content_area) = main_areas(area);
  let logs_viewport = content_area.inner(Margin {
    vertical: 1,
    horizontal: 1,
  });

  frame.render_widget(
    HeaderBar::new(APP_VERSION, app.runtime_status()),
    header_area,
  );
  frame.render_widget(AppTabs::new(app.active_tab(), app.tab_labels()), tabs_area);
  app.set_logs_viewport(logs_viewport.height, logs_viewport.width);

  let active_tab = app.active_tab();
  match active_tab {
    0 => {
      let rows = app.domain_rows();
      StatefulWidget::render(
        DomainsTab::new(rows),
        content_area,
        frame.buffer_mut(),
        app.active_table_state_mut(),
      );
    }
    1 => {
      if app.shows_status_screen() {
        frame.render_widget(
          StatusTab::new(
            app.status_message(),
            app.connection_guidance(),
            app.is_starting_daemon(),
            app.can_start_daemon(),
          ),
          content_area,
        );
      } else {
        let rows = app.settings_rows();
        StatefulWidget::render(
          SettingsTab::new(rows),
          content_area,
          frame.buffer_mut(),
          app.active_table_state_mut(),
        );
      }
    }
    2 => {
      let screen = app.log_screen();
      StatefulWidget::render(
        LogsTab::new(screen.as_ref()),
        content_area,
        frame.buffer_mut(),
        app.active_table_state_mut(),
      );
    }
    _ => {}
  }
}

fn root_areas(root: Rect) -> (Rect, Rect) {
  let footer_height = 1.min(root.height);
  let main_area = Rect {
    x: root.x,
    y: root.y,
    width: root.width,
    height: root.height.saturating_sub(footer_height),
  };
  let footer_area = Rect {
    x: root.x,
    y: root.y + main_area.height,
    width: root.width,
    height: footer_height,
  };

  (main_area, footer_area)
}

fn main_areas(area: Rect) -> (Rect, Rect, Rect) {
  let [header_area, tabs_area, content_area] = area.layout(&Layout::vertical([
    Constraint::Length(1),
    Constraint::Length(1),
    Constraint::Fill(1),
  ]));

  (header_area, tabs_area, content_area)
}

#[cfg(test)]
mod tests {
  use super::*;
  use clap::CommandFactory;

  #[test]
  fn bare_invocation_prints_help_instead_of_starting_the_tui() {
    let error = Cli::try_parse_from(["cadder"]).expect_err("bare invocation should display help");

    assert_eq!(
      error.kind(),
      ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
    assert_eq!(cli_error_exit_code(&error), ExitCode::SUCCESS);
  }

  #[test]
  fn tui_is_the_only_operator_command() {
    let cli = Cli::try_parse_from(["cadder", "tui"]).expect("TUI should parse");

    assert!(matches!(cli.command, Command::Tui));
  }

  #[test]
  fn removed_runtime_selection_options_are_rejected() {
    let error = Cli::try_parse_from(["cadder", "--runtime-dir", "runtime-test", "tui"])
      .expect_err("runtime selection options should be rejected");

    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    assert_eq!(error.exit_code(), 2);
  }

  #[test]
  fn unknown_command_is_rejected_without_starting_the_tui() {
    let error =
      Cli::try_parse_from(["cadder", "web"]).expect_err("unknown commands should be rejected");

    assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
    assert_eq!(error.exit_code(), 2);
  }

  #[test]
  fn root_help_is_generated_from_the_cli_definition() {
    let help = Cli::command().render_help().to_string();

    assert!(help.contains("Cadder operator"));
    assert!(!help.contains("--runtime-dir"));
    assert!(!help.contains("--profile"));
    assert!(help.contains("tui"));
  }
}
