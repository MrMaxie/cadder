use ratatui::widgets::TableState;
use std::time::{SystemTime, UNIX_EPOCH};
use tui_term::vt100::Screen;

use crate::data::{DataModel, DomainTableRow, EntityId, SettingsTableRow};
use crate::logs::LogStore;

pub const DETAIL_SHORTCUTS: [crate::widgets::Shortcut; 4] = [
  crate::widgets::Shortcut::new("Arrows", "prev/next item"),
  crate::widgets::Shortcut::new("PgUp/PgDn", "scroll details"),
  crate::widgets::Shortcut::new("Esc", "close details"),
  crate::widgets::Shortcut::new("Ctrl+C/X", "quit"),
];

pub struct App {
  data: DataModel,
  logs: LogStore,
  table_states: Vec<TableState>,
  runtime_status: RuntimeStatus,
  active_tab: usize,
  details: Option<DetailsState>,
  should_quit: bool,
}

#[derive(Clone, Copy)]
pub struct RuntimeStatus {
  cadderd_running: bool,
  caddy_running: bool,
}

pub struct DetailsState {
  title: String,
  lines: Vec<String>,
  scroll: usize,
}

impl App {
  pub fn load() -> color_eyre::Result<Self> {
    let data = DataModel::load()?;
    let logs = LogStore::new();
    let table_states = vec![
      table_state_for_len(data.domain_rows().len()),
      table_state_for_len(data.settings_rows().len()),
      TableState::new(),
    ];

    Ok(Self {
      data,
      logs,
      table_states,
      runtime_status: pseudorandom_runtime_status(),
      active_tab: 0,
      details: None,
      should_quit: false,
    })
  }

  pub fn active_table_state_mut(&mut self) -> &mut TableState {
    &mut self.table_states[self.active_tab]
  }

  pub const fn active_tab(&self) -> usize {
    self.active_tab
  }

  pub const fn runtime_status(&self) -> RuntimeStatus {
    self.runtime_status
  }

  pub fn tab_labels(&self) -> [String; 3] {
    [
      "Domains".to_string(),
      "Settings".to_string(),
      "logs".to_string(),
    ]
  }

  pub fn domain_rows(&self) -> Vec<DomainTableRow> {
    self.data.domain_rows()
  }

  pub fn settings_rows(&self) -> Vec<SettingsTableRow> {
    self.data.settings_rows()
  }

  pub fn log_screen(&self) -> Option<Screen> {
    self.logs.screen()
  }

  pub fn poll_logs(&mut self) {
    self.logs.poll()
  }

  pub fn set_logs_viewport(&mut self, rows: u16, cols: u16) {
    self.logs.set_viewport(rows, cols);
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
    self.set_active_tab((self.active_tab + 1) % self.table_states.len());
  }

  pub fn previous_tab(&mut self) {
    self.set_active_tab((self.active_tab + self.table_states.len() - 1) % self.table_states.len());
  }

  pub fn set_active_tab(&mut self, index: usize) {
    self.active_tab = index.min(self.table_states.len().saturating_sub(1));
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

  pub fn toggle_current(&mut self) {
    let Some(entity) = self.selected_entity() else {
      return;
    };

    self.data.toggle(entity);
    self.ensure_selected_row();
  }

  pub fn open_details(&mut self) {
    let Some(entity) = self.selected_entity() else {
      return;
    };

    self.details = Some(DetailsState::new(
      self.data.title(entity),
      self.data.describe(entity),
    ));
  }

  pub fn close_details(&mut self) {
    self.details = None;
  }

  pub fn previous_details_item(&mut self) {
    if self.details.is_none() {
      return;
    }

    let previous = self.selected_index();
    self.select_previous();
    if self.selected_index() != previous {
      self.open_details();
    }
  }

  pub fn next_details_item(&mut self) {
    if self.details.is_none() {
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
      0 => self.data.domain_rows().len(),
      1 => self.data.settings_rows().len(),
      2 => 0,
      _ => 0,
    }
  }

  fn selected_index(&self) -> Option<usize> {
    let len = self.active_row_len();
    if len == 0 {
      None
    } else {
      self.table_states[self.active_tab]
        .selected()
        .map(|index| index.min(len - 1))
    }
  }

  fn selected_entity(&self) -> Option<EntityId> {
    let index = self.selected_index()?;
    match self.active_tab {
      0 => self
        .data
        .domain_rows()
        .get(index)
        .map(DomainTableRow::entity),
      1 => self
        .data
        .settings_rows()
        .get(index)
        .map(SettingsTableRow::entity),
      _ => None,
    }
  }

  fn ensure_selected_row(&mut self) {
    let len = self.active_row_len();
    if len == 0 {
      self.active_table_state_mut().select(None);
      return;
    }

    let selected = self
      .table_states
      .get(self.active_tab)
      .and_then(TableState::selected)
      .unwrap_or(0)
      .min(len - 1);
    self.active_table_state_mut().select(Some(selected));
  }
}

fn table_state_for_len(len: usize) -> TableState {
  if len == 0 {
    TableState::new()
  } else {
    TableState::new().with_selected(0)
  }
}

fn pseudorandom_runtime_status() -> RuntimeStatus {
  let seed = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_nanos())
    .unwrap_or(0);

  RuntimeStatus {
    cadderd_running: seed & 1 == 0,
    caddy_running: seed & 2 == 0,
  }
}

impl RuntimeStatus {
  pub const fn cadderd_running(self) -> bool {
    self.cadderd_running
  }

  pub const fn caddy_running(self) -> bool {
    self.caddy_running
  }
}

impl DetailsState {
  fn new(title: String, lines: Vec<String>) -> Self {
    Self {
      title,
      lines,
      scroll: 0,
    }
  }

  pub fn title(&self) -> &str {
    &self.title
  }

  pub fn lines(&self) -> &[String] {
    &self.lines
  }

  pub const fn scroll(&self) -> usize {
    self.scroll
  }

  pub fn line_count(&self) -> usize {
    self.lines.len()
  }
}
