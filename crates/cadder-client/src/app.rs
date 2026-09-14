mod lifecycle;
mod navigation;

#[cfg(test)]
use cadder_api::ConnectionStateView;
use cadder_api::LogsView;
use cadder_ipc::{LogStreamIdentity, RuntimeStatus as ProtocolRuntimeStatus};
use ratatui::widgets::TableState;

use crate::data::DataModel;
use crate::logs::LogStore;

pub const DETAIL_SHORTCUTS: [crate::widgets::Shortcut; 4] = [
  crate::widgets::Shortcut::new("Arrows", "prev/next item"),
  crate::widgets::Shortcut::new("PgUp/PgDn", "scroll details"),
  crate::widgets::Shortcut::new("Esc", "close details"),
  crate::widgets::Shortcut::new("Ctrl+C", "quit"),
];

pub struct App {
  data: DataModel,
  logs: LogStore,
  log_stream: LogStreamIdentity,
  table_states: [TableState; Tab::COUNT],
  runtime_status: RuntimeStatus,
  connection_message: String,
  connection_guidance: Option<String>,
  active_tab: Tab,
  details: Option<DetailsState>,
  confirmation: Option<LifecycleAction>,
  pending: bool,
  last_start_failed: bool,
  should_quit: bool,
}

pub enum RefreshOutcome {
  Connected {
    snapshot: Box<cadder_ipc::GuiStateSnapshot>,
    logs: Result<LogsView, ActionFailure>,
  },
  Unavailable {
    connection: ConnectionStatus,
    message: String,
    guidance: Option<String>,
  },
}

#[derive(Debug, Clone)]
pub struct ActionFailure {
  pub message: String,
  pub guidance: Option<String>,
}

impl From<cadder_api::OperatorError> for ActionFailure {
  fn from(error: cadder_api::OperatorError) -> Self {
    Self {
      message: error.message,
      guidance: error.guidance,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
  Connecting,
  Connected,
  Offline,
  Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
  Domains,
  Status,
  Logs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAction {
  Stop,
  Restart,
}

impl Tab {
  const COUNT: usize = 3;

  pub const fn index(self) -> usize {
    match self {
      Self::Domains => 0,
      Self::Status => 1,
      Self::Logs => 2,
    }
  }

  #[cfg(test)]
  const fn from_index(index: usize) -> Self {
    match index {
      1 => Self::Status,
      2 => Self::Logs,
      _ => Self::Domains,
    }
  }

  const fn next(self) -> Self {
    match self {
      Self::Domains => Self::Status,
      Self::Status => Self::Logs,
      Self::Logs => Self::Domains,
    }
  }

  const fn previous(self) -> Self {
    match self {
      Self::Domains => Self::Logs,
      Self::Status => Self::Domains,
      Self::Logs => Self::Status,
    }
  }
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
  pub fn new() -> Self {
    let data = DataModel::default();
    let mut app = Self {
      data,
      logs: LogStore::new(),
      log_stream: LogStreamIdentity::runtime_control(),
      table_states: [
        TableState::new(),
        TableState::new().with_selected(0),
        TableState::new(),
      ],
      runtime_status: RuntimeStatus {
        connection: ConnectionStatus::Connecting,
        caddy_status: ProtocolRuntimeStatus::Unknown,
      },
      connection_message: "Connecting to Cadder...".to_string(),
      connection_guidance: None,
      active_tab: Tab::Domains,
      details: None,
      confirmation: None,
      pending: false,
      last_start_failed: false,
      should_quit: false,
    };
    app.ensure_selected_row();
    app
  }
}

impl RuntimeStatus {
  pub const fn connection_label(self) -> &'static str {
    match self.connection {
      ConnectionStatus::Connecting => "connecting",
      ConnectionStatus::Connected => "connected",
      ConnectionStatus::Offline => "-",
      ConnectionStatus::Error => "error",
    }
  }

  pub fn caddy_label(self) -> &'static str {
    if self.connection != ConnectionStatus::Connected {
      return "-";
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
    ConnectionStatus::Offline => "Open Status and press Enter to start Cadder.",
    _ => guidance.unwrap_or("Press r to retry the connection."),
  };
  (
    title.to_string(),
    vec![message.to_string(), recovery.to_string()],
  )
}

#[cfg(test)]
fn unavailable_copy(connection_state: ConnectionStateView) -> (&'static str, &'static str) {
  match connection_state {
    ConnectionStateView::NotRunning => (
      "Cadder is not running.",
      "Open Status and press Enter to start it.",
    ),
    ConnectionStateView::ConnectionFailed => (
      "Could not connect to Cadder.",
      "Press r to retry the connection.",
    ),
    ConnectionStateView::Connected => {
      ("Cadder is unavailable.", "Press r to retry the connection.")
    }
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

#[cfg(test)]
mod tests;
