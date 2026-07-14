mod app;
mod data;
mod logs;
mod widgets;

use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use app::App;
use cadder_daemon::{DaemonLaunchOptions, RuntimeProfile};
use cadder_operator::OperatorContext;
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::widgets::StatefulWidget;
use ratatui::{DefaultTerminal, Frame};
use widgets::{
  AppTabs, DetailOverlay, DimBackground, DomainsTab, HeaderBar, LOG_SHORTCUTS, LogsTab,
  MAIN_SHORTCUTS, OFFLINE_SHORTCUTS, SettingsTab, ShortcutsBar,
};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

const ROOT_HELP: &str = "Cadder operator\n\nUsage:\n  cadder\n  cadder [OPTIONS] tui\n\nCommands:\n  tui  Open the full-screen operator\n\nOptions:\n  --runtime-dir <PATH>  Select an explicit runtime directory\n  --profile <NAME>      Select the default or dev runtime profile\n  -h, --help            Print help\n";

fn main() -> ExitCode {
  if let Err(error) = color_eyre::install() {
    eprintln!("Could not initialize Cadder error reporting: {error}");
    return ExitCode::from(9);
  }

  match parse_args(std::env::args().skip(1)) {
    Ok(ParseResult::Help) => {
      print!("{ROOT_HELP}");
      ExitCode::SUCCESS
    }
    Ok(ParseResult::Tui(options)) => match run_tui(options) {
      Ok(()) => ExitCode::SUCCESS,
      Err(error) => {
        eprintln!("Could not run the Cadder TUI: {error}");
        ExitCode::from(9)
      }
    },
    Err(message) => {
      eprintln!("{message}\n\n{ROOT_HELP}");
      ExitCode::from(2)
    }
  }
}

fn run_tui(options: TuiOptions) -> Result<()> {
  let context = OperatorContext::new(
    "tui",
    options.runtime_dir,
    DaemonLaunchOptions {
      runtime_profile: options.profile,
      ..DaemonLaunchOptions::default()
    },
  )?;
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
  while !app.should_quit() {
    runtime.block_on(app.refresh_if_due());
    terminal.draw(|frame| render(frame, app))?;

    if event::poll(Duration::from_millis(16))? {
      match handle_event(event::read()?, app)? {
        Some(UiAction::Refresh) => runtime.block_on(app.refresh()),
        Some(UiAction::StartDaemon) => {
          terminal.draw(|frame| render(frame, app))?;
          runtime.block_on(app.start_daemon());
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

#[derive(Default)]
struct TuiOptions {
  runtime_dir: Option<PathBuf>,
  profile: Option<RuntimeProfile>,
}

enum ParseResult {
  Help,
  Tui(TuiOptions),
}

fn parse_args(args: impl Iterator<Item = String>) -> std::result::Result<ParseResult, String> {
  let args = args.collect::<Vec<_>>();
  if args.is_empty()
    || args
      .iter()
      .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
  {
    return Ok(ParseResult::Help);
  }

  let mut options = TuiOptions::default();
  let mut tui_requested = false;
  let mut index = 0;
  while index < args.len() {
    match args[index].as_str() {
      "tui" if !tui_requested => tui_requested = true,
      "--runtime-dir" => {
        index += 1;
        let value = args
          .get(index)
          .ok_or_else(|| "--runtime-dir requires a path.".to_string())?;
        options.runtime_dir = Some(PathBuf::from(value));
      }
      "--profile" => {
        index += 1;
        let value = args
          .get(index)
          .ok_or_else(|| "--profile requires `default` or `dev`.".to_string())?;
        options.profile = Some(RuntimeProfile::parse_cli(value)?);
      }
      unknown => return Err(format!("Unknown Cadder command or option: `{unknown}`.")),
    }
    index += 1;
  }

  tui_requested
    .then_some(ParseResult::Tui(options))
    .ok_or_else(|| "A command is required.".to_string())
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
      let rows = app.settings_rows();
      StatefulWidget::render(
        SettingsTab::new(rows),
        content_area,
        frame.buffer_mut(),
        app.active_table_state_mut(),
      );
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

  #[test]
  fn bare_invocation_prints_help_instead_of_starting_tui() {
    assert!(matches!(
      parse_args(std::iter::empty()),
      Ok(ParseResult::Help)
    ));
  }

  #[test]
  fn tui_accepts_runtime_options_before_and_after_command() {
    let result = parse_args(
      ["--profile", "dev", "tui", "--runtime-dir", "runtime-test"]
        .into_iter()
        .map(ToString::to_string),
    )
    .expect("TUI options should parse");
    let ParseResult::Tui(options) = result else {
      panic!("expected TUI invocation");
    };
    assert_eq!(options.profile, Some(RuntimeProfile::Dev));
    assert_eq!(options.runtime_dir, Some(PathBuf::from("runtime-test")));
  }

  #[test]
  fn unknown_command_is_rejected_without_starting_tui() {
    let Err(error) = parse_args(std::iter::once("web".to_string())) else {
      panic!("unknown command should be rejected");
    };
    assert!(error.contains("Unknown Cadder command"));
  }
}
