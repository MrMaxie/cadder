use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::widgets::theme::THEME;

pub struct DimBackground;

impl Widget for DimBackground {
  fn render(self, area: Rect, buf: &mut Buffer) {
    buf.set_style(area, THEME.dim_background());
  }
}
