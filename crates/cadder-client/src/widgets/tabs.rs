use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::symbols;
use ratatui::widgets::{Tabs, Widget};

use crate::widgets::theme::THEME;

pub struct AppTabs {
  selected: usize,
  labels: [String; 3],
}

impl AppTabs {
  pub const fn new(selected: usize, labels: [String; 3]) -> Self {
    Self { selected, labels }
  }
}

impl Widget for AppTabs {
  fn render(self, area: Rect, buf: &mut Buffer) {
    let tabs = Tabs::new(self.labels)
      .select(self.selected)
      .divider(symbols::DOT)
      .padding("  ", "  ")
      .style(THEME.inactive_tab())
      .highlight_style(THEME.active_tab());

    tabs.render(area, buf);
  }
}
