use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::text::{Line, Text};
use ratatui::widgets::{
  Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
  StatefulWidget, Widget, Wrap,
};

use crate::widgets::theme::THEME;

pub struct DetailOverlay<'a> {
  title: &'a str,
  lines: &'a [String],
  scroll: usize,
}

impl<'a> DetailOverlay<'a> {
  pub const fn new(title: &'a str, lines: &'a [String], scroll: usize) -> Self {
    Self {
      title,
      lines,
      scroll,
    }
  }
}

impl Widget for DetailOverlay<'_> {
  fn render(self, area: Rect, buf: &mut Buffer) {
    let popup_area = centered_rect(area, 66, 44);
    Clear.render(popup_area, buf);

    let block = Block::new()
      .title(Line::from(self.title).style(THEME.overlay_title()))
      .borders(Borders::ALL)
      .border_style(THEME.overlay_border())
      .style(THEME.overlay());
    let inner = block.inner(popup_area);
    block.render(popup_area, buf);

    let text_area = inner.inner(Margin {
      vertical: 0,
      horizontal: 1,
    });
    let viewport_height = usize::from(text_area.height).max(1);
    let max_scroll = self.lines.len().saturating_sub(viewport_height);
    let scroll = self.scroll.min(max_scroll);

    Paragraph::new(Text::from(detail_lines(self.lines)))
      .wrap(Wrap { trim: false })
      .scroll((scroll.min(usize::from(u16::MAX)) as u16, 0))
      .style(THEME.text())
      .render(text_area, buf);

    if self.lines.len() > viewport_height {
      let mut scrollbar_state = ScrollbarState::new(self.lines.len())
        .viewport_content_length(viewport_height)
        .position(scroll);
      StatefulWidget::render(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        inner,
        buf,
        &mut scrollbar_state,
      );
    }
  }
}

fn centered_rect(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
  let [_, vertical, _] = area.layout(&Layout::vertical([
    Constraint::Percentage((100 - height_percent) / 2),
    Constraint::Percentage(height_percent),
    Constraint::Percentage((100 - height_percent) / 2),
  ]));
  let [_, horizontal, _] = vertical.layout(&Layout::horizontal([
    Constraint::Percentage((100 - width_percent) / 2),
    Constraint::Percentage(width_percent),
    Constraint::Percentage((100 - width_percent) / 2),
  ]));

  horizontal
}

fn detail_lines(lines: &[String]) -> Vec<Line<'_>> {
  lines
    .iter()
    .enumerate()
    .map(|(index, line)| {
      if index == 0 {
        Line::from(line.as_str()).style(THEME.accent_text())
      } else {
        Line::from(line.as_str()).style(THEME.text())
      }
    })
    .collect()
}
