use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{
  Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
  Widget,
};

use crate::widgets::theme::THEME;

pub struct LogsPanel<'a> {
  title: &'a str,
  lines: &'a [String],
  scroll: usize,
}

impl<'a> LogsPanel<'a> {
  pub const fn new(title: &'a str, lines: &'a [String], scroll: usize) -> Self {
    Self {
      title,
      lines,
      scroll,
    }
  }
}

impl Widget for LogsPanel<'_> {
  fn render(self, area: Rect, buf: &mut Buffer) {
    let block = Block::new()
      .title(Line::from(format!(" {} ", self.title)).style(THEME.panel_title()))
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
