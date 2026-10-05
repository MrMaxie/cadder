use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{StatefulWidget, TableState};

use crate::data::DomainTableRow;
use crate::widgets::tabs_content::TableBody;

pub struct RoutesTable {
  rows: Vec<DomainTableRow>,
  empty_message: &'static str,
}

impl RoutesTable {
  pub const fn new(rows: Vec<DomainTableRow>, empty_message: &'static str) -> Self {
    Self {
      rows,
      empty_message,
    }
  }
}

impl StatefulWidget for RoutesTable {
  type State = TableState;

  fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
    TableBody::new(self.rows, self.empty_message).render(area, buf, state);
  }
}
