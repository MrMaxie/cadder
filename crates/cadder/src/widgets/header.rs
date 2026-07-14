use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::app::RuntimeStatus;
use crate::widgets::theme::THEME;

const STATUS_WIDTH: u16 = 42;

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

    let title_width = if area.width > STATUS_WIDTH + 1 {
      area.width - STATUS_WIDTH - 1
    } else {
      area.width
    };
    let title = format!(" Cadder v{}", self.version);
    buf.set_stringn(
      area.x,
      area.y,
      title,
      title_width as usize,
      THEME.header_title(),
    );

    if area.width >= STATUS_WIDTH {
      let status_x = area.x + area.width - STATUS_WIDTH;
      render_service_status(
        buf,
        status_x,
        area.y,
        "cadderd",
        self.runtime_status.connection_label(),
        self.runtime_status.is_connected(),
      );
      buf.set_string(status_x + 20, area.y, " • ", THEME.service_separator());
      render_service_status(
        buf,
        status_x + 23,
        area.y,
        "caddy",
        self.runtime_status.caddy_label(),
        self.runtime_status.caddy_label() == "running",
      );
    }
  }
}

fn render_service_status(
  buf: &mut Buffer,
  x: u16,
  y: u16,
  label: &str,
  status: &str,
  online: bool,
) {
  let style = if online {
    THEME.service_online()
  } else {
    THEME.service_offline()
  };

  buf.set_string(x, y, format!("{label}: {status}"), style);
}
