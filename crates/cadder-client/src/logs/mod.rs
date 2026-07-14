use cadder_api::LogsView;
use tui_term::vt100::{Parser, Screen};

pub(crate) const MAX_LOG_LINES: usize = 1_000;

pub struct LogStore {
  screen: Option<Screen>,
  content: String,
  scrollback: usize,
  follow_tail: bool,
  viewport: Option<TerminalSize>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct TerminalSize {
  rows: u16,
  cols: u16,
}

impl TerminalSize {
  fn new(rows: u16, cols: u16) -> Self {
    Self {
      rows: rows.max(1),
      cols: cols.max(1),
    }
  }
}

impl LogStore {
  pub fn new() -> Self {
    Self {
      screen: None,
      content: "Logs will appear here when Cadder is running.".to_string(),
      scrollback: 0,
      follow_tail: true,
      viewport: None,
    }
  }

  pub fn replace(&mut self, logs: LogsView) {
    let mut lines = Vec::new();
    if logs.has_gap || logs.truncated_by_retention {
      lines.push("[gap] Earlier log entries are no longer available.".to_string());
    }
    lines.extend(logs.entries.into_iter().map(|entry| {
      format!(
        "{} [{:?}] {}",
        entry.timestamp_utc.to_rfc3339(),
        entry.severity,
        entry.raw_message
      )
    }));
    if lines.is_empty() {
      lines.push("No log entries available.".to_string());
    }
    self.content = lines.join("\r\n");
    self.rebuild_screen();
  }

  pub fn set_notice(&mut self, message: impl Into<String>) {
    self.content = message.into();
    self.rebuild_screen();
  }

  pub fn set_viewport(&mut self, rows: u16, cols: u16) {
    if rows < 2 {
      // vt100 panics while scrolling a one-row grid after a wrapped line.
      self.viewport = None;
      self.screen = None;
      self.scrollback = 0;
      return;
    }

    let viewport = TerminalSize::new(rows, cols);
    if self.viewport == Some(viewport) {
      return;
    }
    self.viewport = Some(viewport);
    self.rebuild_screen();
  }

  pub fn screen(&self) -> Option<Screen> {
    let mut screen = self.screen.clone()?;
    if !self.follow_tail {
      screen.set_scrollback(self.scrollback.min(available_scrollback(&screen)));
    }
    Some(screen)
  }

  pub fn scroll_up(&mut self, amount: usize) {
    self.scrollback = self
      .scrollback
      .saturating_add(amount)
      .min(self.current_available_scrollback());
    self.follow_tail = self.scrollback == 0;
  }

  pub fn scroll_down(&mut self, amount: usize) {
    self.scrollback = self.scrollback.saturating_sub(amount);
    self.follow_tail = self.scrollback == 0;
  }

  fn rebuild_screen(&mut self) {
    let Some(viewport) = self.viewport else {
      self.screen = None;
      self.scrollback = 0;
      return;
    };
    let mut parser = Parser::new(viewport.rows, viewport.cols, MAX_LOG_LINES);
    parser.process(self.content.as_bytes());
    self.screen = Some(parser.screen().clone());
    self.scrollback = self.scrollback.min(self.current_available_scrollback());
  }

  fn current_available_scrollback(&self) -> usize {
    self.screen.as_ref().map(available_scrollback).unwrap_or(0)
  }
}

fn available_scrollback(screen: &Screen) -> usize {
  let mut screen = screen.clone();
  screen.set_scrollback(MAX_LOG_LINES);
  screen.scrollback()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn multiline_content_waits_for_the_first_real_viewport() {
    let mut logs = LogStore::new();

    logs.set_notice("first line\r\nsecond line\r\nthird line");

    assert!(logs.screen().is_none());
    logs.set_viewport(10, 80);
    assert!(logs.screen().is_some());
  }

  #[test]
  fn one_row_viewport_skips_terminal_emulation() {
    let mut logs = LogStore::new();

    logs.set_notice("x".repeat(160));
    logs.set_viewport(1, 80);
    assert!(logs.screen().is_none());

    logs.set_viewport(2, 80);
    assert!(logs.screen().is_some());
  }
}
