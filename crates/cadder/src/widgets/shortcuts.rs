use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::widgets::theme::THEME;

pub const MAIN_SHORTCUTS: [Shortcut; 6] = [
  Shortcut::new("Left/Right", "switch tab"),
  Shortcut::new("Up/Down", "select row"),
  Shortcut::new("Space", "toggle enabled"),
  Shortcut::new("Enter", "details"),
  Shortcut::new("Esc", "quit"),
  Shortcut::new("Ctrl+C/X", "quit"),
];

pub const LOG_SHORTCUTS: [Shortcut; 5] = [
  Shortcut::new("Left/Right", "switch tab"),
  Shortcut::new("Up/Down", "scroll logs"),
  Shortcut::new("PgUp/PgDn", "scroll logs"),
  Shortcut::new("Esc", "quit"),
  Shortcut::new("Ctrl+C/X", "quit"),
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
