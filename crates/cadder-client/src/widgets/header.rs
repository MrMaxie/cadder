use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::app::RuntimeStatus;
use crate::widgets::theme::THEME;

pub struct HeaderBar {
  version: &'static str,
  runtime_status: RuntimeStatus,
}

impl HeaderBar {
  pub const fn new(version: &'static str, runtime_status: RuntimeStatus) -> Self {
    Self {
      version,
      runtime_status,
    }
  }
}

impl Widget for HeaderBar {
  fn render(self, area: Rect, buf: &mut Buffer) {
    if area.is_empty() {
      return;
    }

    buf.set_style(area, THEME.header());

    let marker = if self.runtime_status.is_healthy() {
      "●"
    } else {
      "○"
    };
    let status = format!(
      "{marker} {} {} ",
      self.runtime_status.service_label(),
      self.runtime_status.state_label()
    );
    let status_width = u16::try_from(status.chars().count()).unwrap_or(u16::MAX);
    let title_width = area.width.saturating_sub(status_width + 1).max(1);
    let title = format!(" Cadder v{}", self.version);
    buf.set_stringn(
      area.x,
      area.y,
      title,
      title_width as usize,
      THEME.header_title(),
    );

    if area.width > status_width + 1 {
      let status_x = area.x + area.width - status_width;
      let style = if self.runtime_status.is_healthy() {
        THEME.service_online()
      } else {
        THEME.service_offline()
      };
      buf.set_stringn(status_x, area.y, status, status_width.into(), style);
    }
  }
}
