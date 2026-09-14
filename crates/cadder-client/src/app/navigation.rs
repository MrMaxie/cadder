use crate::data::{DomainTableRow, EntityId, SettingsTableRow};

use super::*;

impl App {
  pub fn active_table_state_mut(&mut self) -> &mut TableState {
    &mut self.table_states[self.active_tab.index()]
  }

  pub const fn active_tab(&self) -> Tab {
    self.active_tab
  }

  pub const fn runtime_status(&self) -> RuntimeStatus {
    self.runtime_status
  }

  pub fn tab_labels(&self) -> [String; 3] {
    [
      "Domains".to_string(),
      "Status".to_string(),
      "Logs".to_string(),
    ]
  }

  pub fn domain_rows(&self) -> Vec<DomainTableRow> {
    self.data.domain_rows()
  }

  pub fn settings_rows(&self) -> Vec<SettingsTableRow> {
    self
      .data
      .status_rows(self.runtime_status.connection_label())
  }

  pub fn log_lines(&self) -> &[String] {
    self.logs.lines()
  }

  pub fn log_scroll(&self) -> usize {
    self.logs.scroll()
  }

  pub fn set_logs_viewport(&mut self, rows: u16) {
    self.logs.set_viewport(rows);
  }

  pub const fn details(&self) -> Option<&DetailsState> {
    self.details.as_ref()
  }

  pub const fn should_quit(&self) -> bool {
    self.should_quit
  }

  pub fn quit(&mut self) {
    self.should_quit = true;
  }

  pub fn next_tab(&mut self) {
    self.switch_tab(self.active_tab.next());
  }

  pub fn previous_tab(&mut self) {
    self.switch_tab(self.active_tab.previous());
  }

  #[cfg(test)]
  pub fn set_active_tab(&mut self, index: usize) {
    self.switch_tab(Tab::from_index(index.min(Tab::COUNT - 1)));
  }

  fn switch_tab(&mut self, tab: Tab) {
    if self.active_tab == Tab::Domains
      && let Some(entity) = self.selected_entity()
      && let Some(stream) = self.data.log_stream(&entity)
    {
      self.log_stream = stream;
    }
    self.active_tab = tab;
    self.ensure_selected_row();
  }

  pub fn select_next(&mut self) {
    let len = self.active_row_len();
    if len == 0 {
      self.active_table_state_mut().select(None);
      return;
    }
    let current = self.selected_index().unwrap_or(0);
    self
      .active_table_state_mut()
      .select(Some((current + 1).min(len - 1)));
  }

  pub fn select_previous(&mut self) {
    let len = self.active_row_len();
    if len == 0 {
      self.active_table_state_mut().select(None);
      return;
    }
    let current = self.selected_index().unwrap_or(0);
    self
      .active_table_state_mut()
      .select(Some(current.saturating_sub(1)));
  }

  pub fn scroll_logs_up(&mut self, amount: usize) {
    self.logs.scroll_up(amount);
  }

  pub fn scroll_logs_down(&mut self, amount: usize) {
    self.logs.scroll_down(amount);
  }

  pub fn open_details(&mut self) {
    let Some(entity) = self.selected_entity() else {
      return;
    };
    self.details = Some(DetailsState::new(
      self.data.title(&entity),
      self.data.describe(&entity),
    ));
  }

  pub fn close_details(&mut self) {
    if !self.pending {
      self.details = None;
      self.confirmation = None;
    }
  }

  pub fn previous_details_item(&mut self) {
    if self.pending || self.details.is_none() {
      return;
    }
    let previous = self.selected_index();
    self.select_previous();
    if self.selected_index() != previous {
      self.open_details();
    }
  }

  pub fn next_details_item(&mut self) {
    if self.pending || self.details.is_none() {
      return;
    }
    let previous = self.selected_index();
    self.select_next();
    if self.selected_index() != previous {
      self.open_details();
    }
  }

  pub fn page_details_up(&mut self) {
    if let Some(details) = &mut self.details {
      details.scroll = details.scroll.saturating_sub(8);
    }
  }

  pub fn page_details_down(&mut self) {
    if let Some(details) = &mut self.details {
      details.scroll = (details.scroll + 8).min(details.line_count().saturating_sub(1));
    }
  }

  pub fn active_row_len(&self) -> usize {
    match self.active_tab {
      Tab::Domains => self.data.domain_rows().len(),
      Tab::Status if self.shows_status_screen() => 0,
      Tab::Status => self
        .data
        .status_rows(self.runtime_status.connection_label())
        .len(),
      Tab::Logs => 0,
    }
  }

  fn selected_index(&self) -> Option<usize> {
    let len = self.active_row_len();
    (len > 0).then(|| {
      self.table_states[self.active_tab.index()]
        .selected()
        .unwrap_or(0)
        .min(len - 1)
    })
  }

  pub(super) fn selected_entity(&self) -> Option<EntityId> {
    let index = self.selected_index()?;
    match self.active_tab {
      Tab::Domains => self
        .data
        .domain_rows()
        .get(index)
        .map(DomainTableRow::entity),
      Tab::Status => self
        .data
        .status_rows(self.runtime_status.connection_label())
        .get(index)
        .map(SettingsTableRow::entity),
      Tab::Logs => None,
    }
  }

  pub(super) fn ensure_selected_row(&mut self) {
    let len = self.active_row_len();
    if len == 0 {
      self.active_table_state_mut().select(None);
      return;
    }
    let selected = self.table_states[self.active_tab.index()]
      .selected()
      .unwrap_or(0)
      .min(len - 1);
    self.active_table_state_mut().select(Some(selected));
  }

  pub fn log_stream(&self) -> LogStreamIdentity {
    self.log_stream.clone()
  }
}
