use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{
  Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
  Widget,
};

use crate::widgets::theme::THEME;

pub struct LogsTab<'a> {
  lines: &'a [String],
  scroll: usize,
}

impl<'a> LogsTab<'a> {
  pub const fn new(lines: &'a [String], scroll: usize) -> Self {
    Self { lines, scroll }
  }
}

impl Widget for LogsTab<'_> {
  fn render(self, area: Rect, buf: &mut Buffer) {
    let block = Block::new()
      .borders(Borders::ALL)
      .border_style(THEME.table_border())
      .style(THEME.table());
    let inner = block.inner(area);
    let text = self
      .lines
      .iter()
      .map(|line| Line::raw(line.as_str()))
      .collect::<Vec<_>>();

    Paragraph::new(text)
      .block(block)
      .style(THEME.text())
      .scroll((u16::try_from(self.scroll).unwrap_or(u16::MAX), 0))
      .render(area, buf);

    if self.lines.len() > usize::from(inner.height) {
      let mut state = ScrollbarState::new(self.lines.len()).position(self.scroll);
      StatefulWidget::render(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        inner,
        buf,
        &mut state,
      );
    }
  }
}
