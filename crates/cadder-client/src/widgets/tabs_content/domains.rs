use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{StatefulWidget, TableState};

use crate::data::DomainTableRow;
use crate::widgets::tabs_content::{TableBody, TableBodyRows};
use crate::widgets::theme::THEME;

pub struct DomainsTab {
  rows: Vec<DomainTableRow>,
}

impl DomainsTab {
  pub const fn new(rows: Vec<DomainTableRow>) -> Self {
    Self { rows }
  }
}

impl StatefulWidget for DomainsTab {
  type State = TableState;

  fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
    TableBody::new(THEME.domains_accent(), TableBodyRows::Domains(self.rows))
      .render(area, buf, state);
  }
}
