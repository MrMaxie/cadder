use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::widgets::theme::THEME;

pub const MAIN_SHORTCUTS: [Shortcut; 7] = [
  Shortcut::new("Up/Down", "select"),
  Shortcut::new("Space", "enable/disable"),
  Shortcut::new("l", "logs"),
  Shortcut::new("r", "refresh"),
  Shortcut::new("x", "stop"),
  Shortcut::new("R", "restart"),
  Shortcut::new("q", "quit"),
];

pub const LOG_SHORTCUTS: [Shortcut; 6] = [
  Shortcut::new("Up/Down", "select"),
  Shortcut::new("PgUp/PgDn", "scroll logs"),
  Shortcut::new("l", "close logs"),
  Shortcut::new("r", "refresh"),
  Shortcut::new("Space", "enable/disable"),
  Shortcut::new("q", "quit"),
];

pub const OFFLINE_SHORTCUTS: [Shortcut; 3] = [
  Shortcut::new("Enter", "start cadderd"),
  Shortcut::new("r", "retry"),
  Shortcut::new("q", "quit"),
];

pub const PENDING_SHORTCUTS: [Shortcut; 1] = [Shortcut::new("q", "quit")];

pub const CONFIRMATION_SHORTCUTS: [Shortcut; 3] = [
  Shortcut::new("Enter", "confirm"),
  Shortcut::new("Esc", "cancel"),
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
    let shortcut_spans = self
      .shortcuts
      .iter()
      .flat_map(|shortcut| {
        [
          Span::styled(shortcut.key, THEME.shortcut_key()),
          Span::styled(
            format!(" {}  ", shortcut.description),
            THEME.shortcut_description(),
          ),
        ]
      })
      .collect::<Vec<_>>();
    Paragraph::new(Line::from(shortcut_spans))
      .style(THEME.footer())
      .render(area, buf);
  }
}
