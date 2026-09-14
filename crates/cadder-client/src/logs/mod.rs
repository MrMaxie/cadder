use cadder_api::LogsView;

pub(crate) const MAX_LOG_LINES: usize = 1_000;

pub struct LogStore {
  lines: Vec<String>,
  scrollback: usize,
  viewport_rows: usize,
}

impl LogStore {
  pub fn new() -> Self {
    Self {
      lines: vec!["Logs will appear here when Cadder is running.".to_string()],
      scrollback: 0,
      viewport_rows: 1,
    }
  }

  pub fn replace(&mut self, logs: LogsView) {
    let mut lines = Vec::with_capacity(logs.entries.len());
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
    if lines.len() > MAX_LOG_LINES {
      lines.drain(..lines.len() - MAX_LOG_LINES);
    }
    self.lines = lines;
    self.scrollback = 0;
  }

  pub fn set_notice(&mut self, message: impl Into<String>) {
    self.lines = message.into().lines().map(ToString::to_string).collect();
    if self.lines.is_empty() {
      self.lines.push(String::new());
    }
    self.scrollback = 0;
  }

  pub fn set_viewport(&mut self, rows: u16) {
    self.viewport_rows = usize::from(rows.max(1));
    self.scrollback = self.scrollback.min(self.available_scrollback());
  }

  pub fn lines(&self) -> &[String] {
    &self.lines
  }

  pub fn scroll(&self) -> usize {
    self
      .lines
      .len()
      .saturating_sub(self.viewport_rows)
      .saturating_sub(self.scrollback)
  }

  pub fn scroll_up(&mut self, amount: usize) {
    self.scrollback = self
      .scrollback
      .saturating_add(amount)
      .min(self.available_scrollback());
  }

  pub fn scroll_down(&mut self, amount: usize) {
    self.scrollback = self.scrollback.saturating_sub(amount);
  }

  fn available_scrollback(&self) -> usize {
    self.lines.len().saturating_sub(self.viewport_rows)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn scrolling_is_bounded_by_the_dynamic_content_length() {
    let mut logs = LogStore::new();
    logs.set_notice("one\ntwo\nthree\nfour");
    logs.set_viewport(2);

    assert_eq!(logs.scroll(), 2);
    logs.scroll_up(usize::MAX);
    assert_eq!(logs.scroll(), 0);
    logs.scroll_down(1);
    assert_eq!(logs.scroll(), 1);
  }

  #[test]
  fn notices_preserve_any_number_of_lines() {
    let mut logs = LogStore::new();
    logs.set_notice("one\ntwo\nthree");

    assert_eq!(logs.lines(), ["one", "two", "three"]);
  }
}
