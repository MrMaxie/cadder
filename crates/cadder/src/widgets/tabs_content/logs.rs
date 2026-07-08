use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, StatefulWidget, TableState, Widget};
use tui_term::vt100::Screen;
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::widgets::theme::THEME;

pub struct LogsTab<'a> {
  screen: Option<&'a Screen>,
}

impl<'a> LogsTab<'a> {
  pub const fn new(screen: Option<&'a Screen>) -> Self {
    Self { screen }
  }
}

impl StatefulWidget for LogsTab<'_> {
  type State = TableState;

  fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
    let block = Block::new()
      .borders(Borders::ALL)
      .border_style(THEME.table_border())
      .style(THEME.table());

    if let Some(screen) = self.screen {
      PseudoTerminal::new(screen)
        .block(block)
        .cursor(Cursor::default().visibility(false))
        .style(THEME.text())
        .render(area, buf);
    } else {
      block.render(area, buf);
    }
  }
}
