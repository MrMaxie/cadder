use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{StatefulWidget, TableState};

use crate::data::DomainTableRow;
use crate::widgets::tabs_content::TableBody;

pub struct RoutesTable {
  rows: Vec<DomainTableRow>,
}

impl RoutesTable {
  pub const fn new(rows: Vec<DomainTableRow>) -> Self {
    Self { rows }
  }
}

impl StatefulWidget for RoutesTable {
  type State = TableState;

  fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
    TableBody::new(self.rows).render(area, buf, state);
  }
}
