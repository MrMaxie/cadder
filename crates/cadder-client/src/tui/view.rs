use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, StatefulWidget, Widget};

use crate::app::App;
use crate::widgets::theme::THEME;
use crate::widgets::{
  CONFIRMATION_SHORTCUTS, HeaderBar, LOG_SHORTCUTS, LogsPanel, MAIN_SHORTCUTS, OFFLINE_SHORTCUTS,
  PENDING_SHORTCUTS, RoutesTable, Shortcut, ShortcutsBar,
};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub(super) fn render(frame: &mut Frame<'_>, app: &mut App) {
  let root = frame.area();
  let (main_area, footer_area) = root_areas(root);
  render_main(frame, main_area, app);
  frame.render_widget(ShortcutsBar::new(shortcuts(app)), footer_area);
}

fn shortcuts(app: &App) -> &'static [Shortcut] {
  if app.confirmation().is_some() {
    &CONFIRMATION_SHORTCUTS
  } else if app.is_pending() {
    &PENDING_SHORTCUTS
  } else if app.can_start_daemon() {
    &OFFLINE_SHORTCUTS
  } else if app.logs_open() {
    &LOG_SHORTCUTS
  } else {
    &MAIN_SHORTCUTS
  }
}

fn render_main(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
  if area.height == 0 {
    return;
  }

  let notice = app.notice();
  let areas = main_areas(area, app.logs_open(), notice.is_some());
  frame.render_widget(
    HeaderBar::new(APP_VERSION, app.runtime_status()),
    areas.header,
  );

  let rows = app.domain_rows();
  StatefulWidget::render(
    RoutesTable::new(rows),
    areas.routes,
    frame.buffer_mut(),
    app.table_state_mut(),
  );

  if let Some(logs) = areas.logs {
    app.set_logs_viewport(logs.height.saturating_sub(2));
    frame.render_widget(
      LogsPanel::new(&app.log_title(), app.log_lines(), app.log_scroll()),
      logs,
    );
  }

  if let (Some(notice_area), Some(notice)) = (areas.notice, notice) {
    Paragraph::new(Line::from(notice))
      .style(THEME.notice())
      .render(notice_area, frame.buffer_mut());
  }
}

struct MainAreas {
  header: Rect,
  routes: Rect,
  logs: Option<Rect>,
  notice: Option<Rect>,
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

fn main_areas(area: Rect, logs_open: bool, has_notice: bool) -> MainAreas {
  let notice_height = u16::from(has_notice);
  let available_content = area.height.saturating_sub(1 + notice_height);
  let logs_height = if logs_open && available_content >= 8 {
    (available_content / 3).clamp(6, 12)
  } else {
    0
  };
  let [header, routes, logs, notice] = area.layout(&Layout::vertical([
    Constraint::Length(1),
    Constraint::Fill(1),
    Constraint::Length(logs_height),
    Constraint::Length(notice_height),
  ]));

  MainAreas {
    header,
    routes,
    logs: (logs_height > 0).then_some(logs),
    notice: has_notice.then_some(notice),
  }
}
