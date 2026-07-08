use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{StatefulWidget, TableState};

use crate::data::SettingsTableRow;
use crate::widgets::tabs_content::{TableBody, TableBodyRows};
use crate::widgets::theme::THEME;

pub struct SettingsTab {
  rows: Vec<SettingsTableRow>,
}

impl SettingsTab {
  pub const fn new(rows: Vec<SettingsTableRow>) -> Self {
    Self { rows }
  }
}

impl StatefulWidget for SettingsTab {
  type State = TableState;

  fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
    TableBody::new(THEME.settings_accent(), TableBodyRows::Settings(self.rows))
      .render(area, buf, state);
  }
}
