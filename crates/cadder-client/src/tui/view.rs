use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::widgets::StatefulWidget;

use crate::app::{App, Tab};
use crate::widgets::{
  AppTabs, DetailOverlay, DimBackground, DomainsTab, HeaderBar, LOG_SHORTCUTS, LogsTab,
  MAIN_SHORTCUTS, OFFLINE_SHORTCUTS, STARTING_SHORTCUTS, SettingsTab, ShortcutsBar, StatusTab,
};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub(super) fn render(frame: &mut Frame<'_>, app: &mut App) {
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
    crate::app::DETAIL_SHORTCUTS.as_slice()
  } else if app.is_starting_daemon() {
    STARTING_SHORTCUTS.as_slice()
  } else if app.can_start_daemon() {
    OFFLINE_SHORTCUTS.as_slice()
  } else if app.active_tab() == Tab::Logs {
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
  frame.render_widget(
    AppTabs::new(app.active_tab().index(), app.tab_labels()),
    tabs_area,
  );
  app.set_logs_viewport(logs_viewport.height);

  match app.active_tab() {
    Tab::Domains => {
      let rows = app.domain_rows();
      StatefulWidget::render(
        DomainsTab::new(rows),
        content_area,
        frame.buffer_mut(),
        app.active_table_state_mut(),
      );
    }
    Tab::Status if app.shows_status_screen() => frame.render_widget(
      StatusTab::new(
        app.status_message(),
        app.connection_guidance(),
        app.is_starting_daemon(),
        app.can_start_daemon(),
      ),
      content_area,
    ),
    Tab::Status => {
      let rows = app.settings_rows();
      StatefulWidget::render(
        SettingsTab::new(rows),
        content_area,
        frame.buffer_mut(),
        app.active_table_state_mut(),
      );
    }
    Tab::Logs => frame.render_widget(
      LogsTab::new(app.log_lines(), app.log_scroll()),
      content_area,
    ),
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
