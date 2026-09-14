mod lifecycle;
mod navigation;

use cadder_api::LogsView;
use cadder_ipc::{LogStreamIdentity, RuntimeStatus as ProtocolRuntimeStatus};
use ratatui::widgets::TableState;

use crate::data::DataModel;
use crate::logs::LogStore;

pub struct App {
  data: DataModel,
  logs: LogStore,
  log_stream: LogStreamIdentity,
  table_state: TableState,
  runtime_status: RuntimeStatus,
  connection_message: String,
  connection_guidance: Option<String>,
  logs_open: bool,
  confirmation: Option<LifecycleAction>,
  notice: Option<String>,
  pending_message: Option<&'static str>,
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
pub enum LifecycleAction {
  Stop,
  Restart,
}

#[derive(Clone, Copy)]
pub struct RuntimeStatus {
  connection: ConnectionStatus,
  caddy_status: ProtocolRuntimeStatus,
}

impl App {
  pub fn new() -> Self {
    let mut app = Self {
      data: DataModel::default(),
      logs: LogStore::new(),
      log_stream: LogStreamIdentity::runtime_control(),
      table_state: TableState::new(),
      runtime_status: RuntimeStatus {
        connection: ConnectionStatus::Connecting,
        caddy_status: ProtocolRuntimeStatus::Unknown,
      },
      connection_message: "Connecting to Cadder...".to_string(),
      connection_guidance: None,
      logs_open: false,
      confirmation: None,
      notice: None,
      pending_message: None,
      last_start_failed: false,
      should_quit: false,
    };
    app.ensure_selected_row();
    app
  }
}

impl RuntimeStatus {
  pub const fn service_label(self) -> &'static str {
    if matches!(self.connection, ConnectionStatus::Connected) {
      "Caddy"
    } else {
      "Cadder"
    }
  }

  pub const fn state_label(self) -> &'static str {
    match self.connection {
      ConnectionStatus::Connecting => "connecting",
      ConnectionStatus::Offline => "offline",
      ConnectionStatus::Error => "connection error",
      ConnectionStatus::Connected => match self.caddy_status {
        ProtocolRuntimeStatus::Unknown => "unknown",
        ProtocolRuntimeStatus::NotResolved => "not resolved",
        ProtocolRuntimeStatus::Resolved => "resolved",
        ProtocolRuntimeStatus::Running => "running",
        ProtocolRuntimeStatus::Unhealthy => "unhealthy",
        ProtocolRuntimeStatus::Idle => "idle",
      },
    }
  }

  pub const fn is_healthy(self) -> bool {
    matches!(self.connection, ConnectionStatus::Connected)
      && matches!(self.caddy_status, ProtocolRuntimeStatus::Running)
  }
}

impl ConnectionStatus {
  const fn can_start_daemon(self) -> bool {
    matches!(self, Self::Offline)
  }
}

#[cfg(test)]
mod tests;
