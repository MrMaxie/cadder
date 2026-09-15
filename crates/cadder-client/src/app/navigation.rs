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
  }

  pub fn select_previous(&mut self) {
    let len = self.domain_rows().len();
    if len == 0 {
      self.table_state.select(None);
      return;
    }
    let current = self.selected_index().unwrap_or(0);
    self.table_state.select(Some(current.saturating_sub(1)));
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
  }
}
