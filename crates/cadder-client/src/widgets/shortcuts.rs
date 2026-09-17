use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::widgets::theme::THEME;

pub const MAIN_SHORTCUTS: [Shortcut; 6] = [
  Shortcut::new("↑ ↓", "select"),
  Shortcut::new("enter", "enable/disable"),
  Shortcut::new("r", "refresh"),
  Shortcut::new("x", "stop"),
  Shortcut::new("shift+r", "restart"),
  Shortcut::new("q", "quit"),
];

pub const OFFLINE_SHORTCUTS: [Shortcut; 3] = [
  Shortcut::new("enter", "start cadderd"),
  Shortcut::new("r", "retry"),
  Shortcut::new("q", "quit"),
];

pub const PENDING_SHORTCUTS: [Shortcut; 1] = [Shortcut::new("q", "quit")];

pub const CONFIRMATION_SHORTCUTS: [Shortcut; 3] = [
  Shortcut::new("enter", "confirm"),
  Shortcut::new("esc", "cancel"),
  Shortcut::new("q", "quit"),
];

#[derive(Debug, Clone, Copy)]
pub struct Shortcut {
  key: &'static str,
  description: &'static str,
}

pub struct ShortcutsBar<'a> {
  shortcuts: &'a [Shortcut],
}

impl Shortcut {
  pub const fn new(key: &'static str, description: &'static str) -> Self {
    Self { key, description }
  }
}

impl<'a> ShortcutsBar<'a> {
  pub const fn new(shortcuts: &'a [Shortcut]) -> Self {
    Self { shortcuts }
  }
}

impl Widget for ShortcutsBar<'_> {
  fn render(self, area: Rect, buf: &mut Buffer) {
    let mut shortcut_spans = Vec::with_capacity(self.shortcuts.len().saturating_mul(3));
    for (index, shortcut) in self.shortcuts.iter().enumerate() {
      if index > 0 {
        shortcut_spans.push(Span::styled(" · ", THEME.shortcut_separator()));
      }
      shortcut_spans.push(Span::styled(shortcut.key, THEME.shortcut_key()));
      shortcut_spans.push(Span::styled(
        format!(" {}", shortcut.description),
        THEME.shortcut_description(),
      ));
    }
    Paragraph::new(Line::from(shortcut_spans))
      .style(THEME.footer())
      .render(area, buf);
  }
}
