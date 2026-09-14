use crate::data::{DomainTableRow, EntityId};

use super::*;

impl App {
  pub fn table_state_mut(&mut self) -> &mut TableState {
    &mut self.table_state
  }

  pub const fn runtime_status(&self) -> RuntimeStatus {
    self.runtime_status
  }

  pub fn domain_rows(&self) -> Vec<DomainTableRow> {
    self.data.domain_rows()
  }

  pub fn log_lines(&self) -> &[String] {
    self.logs.lines()
  }

  pub fn log_scroll(&self) -> usize {
    self.logs.scroll()
  }

  pub fn log_title(&self) -> String {
    self
      .selected_entity()
      .map_or_else(|| "Logs".to_string(), |entity| self.data.log_title(&entity))
  }

  pub fn set_logs_viewport(&mut self, rows: u16) {
    self.logs.set_viewport(rows);
  }

  pub const fn logs_open(&self) -> bool {
    self.logs_open
  }

  pub fn toggle_logs(&mut self) -> bool {
    self.logs_open = !self.logs_open;
    if self.logs_open {
      self.sync_log_stream();
    }
    self.logs_open
  }

  pub const fn should_quit(&self) -> bool {
    self.should_quit
  }

  pub fn quit(&mut self) {
    self.should_quit = true;
  }

  pub fn select_next(&mut self) {
    let len = self.domain_rows().len();
    if len == 0 {
      self.table_state.select(None);
      return;
    }
    let current = self.selected_index().unwrap_or(0);
    self.table_state.select(Some((current + 1).min(len - 1)));
    self.sync_log_stream();
  }

  pub fn select_previous(&mut self) {
    let len = self.domain_rows().len();
    if len == 0 {
      self.table_state.select(None);
      return;
    }
    let current = self.selected_index().unwrap_or(0);
    self.table_state.select(Some(current.saturating_sub(1)));
    self.sync_log_stream();
  }

  pub fn scroll_logs_up(&mut self, amount: usize) {
    self.logs.scroll_up(amount);
  }

  pub fn scroll_logs_down(&mut self, amount: usize) {
    self.logs.scroll_down(amount);
  }

  fn selected_index(&self) -> Option<usize> {
    let len = self.domain_rows().len();
    (len > 0).then(|| self.table_state.selected().unwrap_or(0).min(len - 1))
  }

  pub(super) fn selected_entity(&self) -> Option<EntityId> {
    let index = self.selected_index()?;
    self
      .data
      .domain_rows()
      .get(index)
      .map(DomainTableRow::entity)
  }

  pub(super) fn ensure_selected_row(&mut self) {
    let len = self.domain_rows().len();
    if len == 0 {
      self.table_state.select(None);
      return;
    }
    let selected = self.table_state.selected().unwrap_or(0).min(len - 1);
    self.table_state.select(Some(selected));
    self.sync_log_stream();
  }

  fn sync_log_stream(&mut self) {
    if let Some(stream) = self
      .selected_entity()
      .and_then(|entity| self.data.log_stream(&entity))
    {
      self.log_stream = stream;
    }
  }

  pub fn log_stream(&self) -> LogStreamIdentity {
    self.log_stream.clone()
  }
}
