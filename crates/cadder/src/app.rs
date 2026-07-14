use std::time::{Duration, Instant};

use cadder_operator::{
  ConnectionStateView, DomainSelector, OperatorContext, OperatorError, unavailable_status,
};
use cadder_protocol::{LogStreamIdentity, RuntimeStatus as ProtocolRuntimeStatus};
use ratatui::widgets::TableState;
use tui_term::vt100::Screen;

use crate::data::{DataModel, DomainTableRow, EntityId, MutationTarget, SettingsTableRow};
use crate::logs::LogStore;

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

pub const DETAIL_SHORTCUTS: [crate::widgets::Shortcut; 4] = [
  crate::widgets::Shortcut::new("Arrows", "prev/next item"),
  crate::widgets::Shortcut::new("PgUp/PgDn", "scroll details"),
  crate::widgets::Shortcut::new("Esc", "close details"),
  crate::widgets::Shortcut::new("Ctrl+C", "quit"),
];

pub struct App {
  context: OperatorContext,
  data: DataModel,
  logs: LogStore,
  log_stream: LogStreamIdentity,
  table_states: Vec<TableState>,
  runtime_status: RuntimeStatus,
  connection_message: String,
  connection_guidance: Option<String>,
  active_tab: usize,
  details: Option<DetailsState>,
  pending: bool,
  should_quit: bool,
  last_refresh: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
  Connecting,
  Connected,
  Offline,
  Error,
}

#[derive(Clone, Copy)]
pub struct RuntimeStatus {
  connection: ConnectionStatus,
  caddy_status: ProtocolRuntimeStatus,
}

pub struct DetailsState {
  title: String,
  lines: Vec<String>,
  scroll: usize,
}

impl App {
  pub fn new(context: OperatorContext) -> Self {
    let data = DataModel::default();
    let mut app = Self {
      context,
      data,
      logs: LogStore::new(),
      log_stream: LogStreamIdentity::runtime_control(),
      table_states: vec![
        TableState::new(),
        TableState::new().with_selected(0),
        TableState::new(),
      ],
      runtime_status: RuntimeStatus {
        connection: ConnectionStatus::Connecting,
        caddy_status: ProtocolRuntimeStatus::Unknown,
      },
      connection_message: "Connecting to cadderd...".to_string(),
      connection_guidance: None,
      active_tab: 0,
      details: None,
      pending: false,
      should_quit: false,
      last_refresh: Instant::now() - REFRESH_INTERVAL,
    };
    app.ensure_selected_row();
    app
  }

  pub async fn refresh_if_due(&mut self) {
    if self.last_refresh.elapsed() >= REFRESH_INTERVAL && !self.pending {
      self.refresh().await;
    }
  }

  pub async fn refresh(&mut self) {
    self.last_refresh = Instant::now();
    match self.context.query_state_response().await {
      Ok(response) => {
        let Some(snapshot) = response.snapshot else {
          self.set_connection_error("Cadder daemon returned no state snapshot.", None);
          return;
        };
        self.runtime_status = RuntimeStatus {
          connection: ConnectionStatus::Connected,
          caddy_status: snapshot.runtime.status,
        };
        self.connection_message = "Connected".to_string();
        self.connection_guidance = None;
        self.data.replace_snapshot(snapshot);
        self.ensure_selected_row();
        self.refresh_logs().await;
      }
      Err(error) => {
        let status = unavailable_status(&self.context, &error);
        self.runtime_status = RuntimeStatus {
          connection: match status.connection_state {
            ConnectionStateView::NotRunning => ConnectionStatus::Offline,
            ConnectionStateView::ConnectionFailed => ConnectionStatus::Error,
            ConnectionStateView::Connected => ConnectionStatus::Connected,
          },
          caddy_status: ProtocolRuntimeStatus::Unknown,
        };
        self.connection_message = status.message;
        self.connection_guidance = status.guidance;
        self.data.clear_snapshot();
        self.ensure_selected_row();
        self.logs.set_notice(
          self
            .connection_guidance
            .clone()
            .unwrap_or_else(|| "Press r to retry the connection.".to_string()),
        );
      }
    }
  }

  pub async fn complete_mutation(&mut self, target: MutationTarget) {
    let result = match &target.entity {
      EntityId::Entrypoint(registration_id) => {
        self
          .context
          .set_entrypoint_enabled("tui", registration_id.clone(), target.enabled)
          .await
      }
      EntityId::Domain {
        registration_id,
        canonical_domain,
      } => {
        self
          .context
          .set_domain_enabled(
            "tui",
            &DomainSelector {
              domain: canonical_domain.clone(),
              registration: Some(registration_id.clone()),
            },
            target.enabled,
          )
          .await
      }
      EntityId::Status(_) => return,
    };

    self.pending = false;
    match result {
      Ok(_) => {
        self.details = None;
        self.refresh().await;
      }
      Err(error) => self.show_operator_error(error),
    }
  }

  pub fn prepare_start_daemon(&mut self) -> bool {
    if !self.can_start_daemon() {
      return false;
    }
    self.pending = true;
    self.details = Some(DetailsState::new(
      " Starting cadderd ".to_string(),
      vec!["Waiting for the selected daemon to become ready...".to_string()],
    ));
    true
  }

  pub const fn can_start_daemon(&self) -> bool {
    !self.pending && self.runtime_status.connection.can_start_daemon()
  }

  pub async fn start_daemon(&mut self) {
    let result = self.context.ensure_daemon_running("tui").await;
    self.pending = false;
    match result {
      Ok(()) => {
        self.details = None;
        self.refresh().await;
      }
      Err(error) => self.show_operator_error(error),
    }
  }

  pub fn prepare_toggle_current(&mut self) -> Option<MutationTarget> {
    if self.pending {
      return None;
    }
    if self.runtime_status.connection != ConnectionStatus::Connected {
      let (title, lines) = connection_recovery_details(
        self.runtime_status.connection,
        &self.connection_message,
        self.connection_guidance.as_deref(),
      );
      self.details = Some(DetailsState::new(title, lines));
      return None;
    }
    let entity = self.selected_entity()?;
    let target = self.data.mutation_target(&entity)?;
    self.pending = true;
    self.details = Some(DetailsState::new(
      " Applying change ".to_string(),
      vec!["Waiting for the daemon to confirm the requested state...".to_string()],
    ));
    Some(target)
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
      "Status".to_string(),
      format!("Logs ({})", self.logs.stream_label()),
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

  pub fn log_screen(&self) -> Option<Screen> {
    self.logs.screen()
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
    if self.active_tab == 0
      && let Some(entity) = self.selected_entity()
      && let Some(stream) = self.data.log_stream(&entity)
    {
      self.log_stream = stream;
    }
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
      0 => self.data.domain_rows().len(),
      1 => self
        .data
        .status_rows(self.runtime_status.connection_label())
        .len(),
      _ => 0,
    }
  }

  fn selected_index(&self) -> Option<usize> {
    let len = self.active_row_len();
    (len > 0).then(|| {
      self.table_states[self.active_tab]
        .selected()
        .unwrap_or(0)
        .min(len - 1)
    })
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
        .status_rows(self.runtime_status.connection_label())
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
    let selected = self.table_states[self.active_tab]
      .selected()
      .unwrap_or(0)
      .min(len - 1);
    self.active_table_state_mut().select(Some(selected));
  }

  async fn refresh_logs(&mut self) {
    match self
      .context
      .query_logs(
        "tui",
        "query logs",
        self.log_stream.clone(),
        200,
        None,
        None,
      )
      .await
    {
      Ok(logs) => self.logs.replace(logs),
      Err(error) => self.logs.set_notice(error.message),
    }
  }

  fn set_connection_error(&mut self, message: &str, guidance: Option<&str>) {
    self.runtime_status = RuntimeStatus {
      connection: ConnectionStatus::Error,
      caddy_status: ProtocolRuntimeStatus::Unknown,
    };
    self.connection_message = message.to_string();
    self.connection_guidance = guidance.map(ToString::to_string);
    self.data.clear_snapshot();
    self.ensure_selected_row();
    self.logs.set_notice(guidance.unwrap_or(message));
  }

  fn show_operator_error(&mut self, error: OperatorError) {
    let mut lines = vec![error.message];
    if let Some(guidance) = error.guidance {
      lines.push(guidance);
    }
    self.details = Some(DetailsState::new(" Action failed ".to_string(), lines));
  }
}

impl RuntimeStatus {
  pub const fn connection_label(self) -> &'static str {
    match self.connection {
      ConnectionStatus::Connecting => "connecting",
      ConnectionStatus::Connected => "connected",
      ConnectionStatus::Offline => "not running",
      ConnectionStatus::Error => "error",
    }
  }

  pub fn caddy_label(self) -> &'static str {
    if self.connection != ConnectionStatus::Connected {
      return "unknown";
    }
    match self.caddy_status {
      ProtocolRuntimeStatus::Unknown => "unknown",
      ProtocolRuntimeStatus::NotResolved => "not resolved",
      ProtocolRuntimeStatus::Resolved => "resolved",
      ProtocolRuntimeStatus::Running => "running",
      ProtocolRuntimeStatus::Unhealthy => "unhealthy",
      ProtocolRuntimeStatus::Idle => "idle",
    }
  }

  pub fn is_connected(self) -> bool {
    self.connection == ConnectionStatus::Connected
  }
}

impl ConnectionStatus {
  const fn can_start_daemon(self) -> bool {
    matches!(self, Self::Offline)
  }
}

fn connection_recovery_details(
  status: ConnectionStatus,
  message: &str,
  guidance: Option<&str>,
) -> (String, Vec<String>) {
  let title = match status {
    ConnectionStatus::Connecting => " Connecting ",
    ConnectionStatus::Offline => " Offline ",
    ConnectionStatus::Error => " Connection error ",
    ConnectionStatus::Connected => " Connected ",
  };
  let recovery = match status {
    ConnectionStatus::Offline => "Press s to start cadderd, or r to retry the connection.",
    _ => guidance.unwrap_or("Press r to retry the connection."),
  };
  (
    title.to_string(),
    vec![message.to_string(), recovery.to_string()],
  )
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn runtime_status_labels_preserve_authoritative_caddy_states() {
    let label = |caddy_status| {
      RuntimeStatus {
        connection: ConnectionStatus::Connected,
        caddy_status,
      }
      .caddy_label()
    };

    assert_eq!(label(ProtocolRuntimeStatus::Unknown), "unknown");
    assert_eq!(label(ProtocolRuntimeStatus::NotResolved), "not resolved");
    assert_eq!(label(ProtocolRuntimeStatus::Resolved), "resolved");
    assert_eq!(label(ProtocolRuntimeStatus::Running), "running");
    assert_eq!(label(ProtocolRuntimeStatus::Unhealthy), "unhealthy");
    assert_eq!(label(ProtocolRuntimeStatus::Idle), "idle");
  }

  #[test]
  fn connection_error_recovery_preserves_guidance_without_suggesting_daemon_start() {
    let (title, lines) = connection_recovery_details(
      ConnectionStatus::Error,
      "The protocol is incompatible.",
      Some("Upgrade the older Cadder component."),
    );

    assert_eq!(title, " Connection error ");
    assert_eq!(lines[1], "Upgrade the older Cadder component.");
    assert!(!lines.iter().any(|line| line.contains("Press s")));
  }

  #[test]
  fn offline_recovery_offers_explicit_daemon_start() {
    let (title, lines) =
      connection_recovery_details(ConnectionStatus::Offline, "cadderd is not running.", None);

    assert_eq!(title, " Offline ");
    assert!(lines[1].contains("Press s"));
    assert!(ConnectionStatus::Offline.can_start_daemon());
    assert!(!ConnectionStatus::Error.can_start_daemon());
  }
}
