use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::widgets::theme::THEME;

pub struct StatusTab<'a> {
  message: &'a str,
  guidance: Option<&'a str>,
  starting: bool,
  can_start: bool,
}

impl<'a> StatusTab<'a> {
  pub const fn new(
    message: &'a str,
    guidance: Option<&'a str>,
    starting: bool,
    can_start: bool,
  ) -> Self {
    Self {
      message,
      guidance,
      starting,
      can_start,
    }
  }
}

impl Widget for StatusTab<'_> {
  fn render(self, area: Rect, buf: &mut Buffer) {
    let block = Block::new()
      .borders(Borders::ALL)
      .border_style(THEME.table_border())
      .style(THEME.table());
    let lines = if self.starting {
      vec![
        Line::from(Span::styled("Starting cadderd...", THEME.text())),
        Line::from(""),
        Line::from(Span::styled("[ Starting... ]", THEME.accent_text())),
      ]
    } else {
      let mut lines = vec![Line::from(Span::styled(self.message, THEME.text()))];
      if let Some(guidance) = self.guidance {
        lines.push(Line::from(Span::styled(
          guidance,
          THEME.shortcut_description(),
        )));
      }
      if self.can_start {
        lines.extend([
          Line::from(""),
          Line::from(Span::styled("[ Start cadderd ]", THEME.accent_text())),
          Line::from(Span::styled(
            "Press Enter to start.",
            THEME.shortcut_description(),
          )),
        ]);
      }
      lines
    };

    Paragraph::new(lines).block(block).render(area, buf);
  }
}
