mod app;
mod data;
mod logs;
mod widgets;

use std::io;
use std::time::Duration;

use app::App;
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::widgets::StatefulWidget;
use ratatui::{DefaultTerminal, Frame};
use widgets::{
  AppTabs, DetailOverlay, DimBackground, DomainsTab, HeaderBar, LOG_SHORTCUTS, LogsTab,
  MAIN_SHORTCUTS, SettingsTab, ShortcutsBar,
};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> Result<()> {
  color_eyre::install()?;
  let mut app = App::load()?;

  let mut terminal = ratatui::init();
  let result = run(&mut terminal, &mut app);
  ratatui::restore();
  result?;

  Ok(())
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
  while !app.should_quit() {
    app.poll_logs();
    terminal.draw(|frame| render(frame, app))?;

    if event::poll(Duration::from_millis(16))? {
      handle_event(event::read()?, app)?;
    }
  }

  Ok(())
}

fn handle_event(event: Event, app: &mut App) -> io::Result<()> {
  match event {
    Event::Key(key) if key.kind == KeyEventKind::Press => {
      handle_key(key.code, key.modifiers, app);
    }
    _ => {}
  }

  Ok(())
}

fn handle_key(code: KeyCode, modifiers: KeyModifiers, app: &mut App) {
  if is_force_quit_key(code, modifiers) {
    app.quit();
    return;
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
    return;
  }

  match code {
    KeyCode::Esc => app.quit(),
    KeyCode::Left => app.previous_tab(),
    KeyCode::Right => app.next_tab(),
    KeyCode::Up if app.active_tab() == 2 => app.scroll_logs_up(1),
    KeyCode::Down if app.active_tab() == 2 => app.scroll_logs_down(1),
    KeyCode::PageUp if app.active_tab() == 2 => app.scroll_logs_up(10),
    KeyCode::PageDown if app.active_tab() == 2 => app.scroll_logs_down(10),
    KeyCode::Up => app.select_previous(),
    KeyCode::Down => app.select_next(),
    KeyCode::Char(' ') => app.toggle_current(),
    KeyCode::Enter => app.open_details(),
    _ => {}
  }
}

fn is_force_quit_key(code: KeyCode, modifiers: KeyModifiers) -> bool {
  modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c' | 'C' | 'x' | 'X'))
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
