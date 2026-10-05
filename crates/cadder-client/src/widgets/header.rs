use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

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

    let status = Line::from(vec![
      Span::styled(
        status_marker(self.runtime_status.daemon_is_running()),
        status_state(self.runtime_status.daemon_is_running()),
      ),
      Span::styled(" cadderd", THEME.status_name()),
      Span::styled(" · ", THEME.status_separator()),
      Span::styled(
        status_marker(self.runtime_status.caddy_is_running()),
        status_state(self.runtime_status.caddy_is_running()),
      ),
      Span::styled(" Caddy", THEME.status_name()),
      Span::raw(" "),
    ]);
    let status_width = u16::try_from(status.width()).unwrap_or(u16::MAX);
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
      Paragraph::new(status)
        .alignment(Alignment::Right)
        .style(THEME.header())
        .render(Rect::new(status_x, area.y, status_width, 1), buf);
    }
  }
}

const fn status_marker(running: bool) -> &'static str {
  if running { "●" } else { "○" }
}

fn status_state(running: bool) -> ratatui::style::Style {
  if running {
    THEME.status_running()
  } else {
    THEME.status_stopped()
  }
}
