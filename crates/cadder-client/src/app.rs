mod lifecycle;
mod navigation;

use cadder_ipc::RuntimeStatus as ProtocolRuntimeStatus;
use ratatui::widgets::TableState;

use crate::data::DataModel;

pub struct App {
  data: DataModel,
  table_state: TableState,
  runtime_status: RuntimeStatus,
  connection_message: String,
  connection_guidance: Option<String>,
  confirmation: Option<LifecycleAction>,
  notice: Option<String>,
  pending_message: Option<&'static str>,
  pending_frame: usize,
  last_start_failed: bool,
  should_quit: bool,
}

pub enum RefreshOutcome {
  Connected {
    snapshot: Box<cadder_ipc::GuiStateSnapshot>,
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
      table_state: TableState::new(),
      runtime_status: RuntimeStatus {
        connection: ConnectionStatus::Connecting,
        caddy_status: ProtocolRuntimeStatus::Unknown,
      },
      connection_message: "Connecting to Cadder...".to_string(),
      connection_guidance: None,
      confirmation: None,
      notice: None,
      pending_message: None,
      pending_frame: 0,
      last_start_failed: false,
      should_quit: false,
    };
    app.ensure_selected_row();
    app
  }
}

impl RuntimeStatus {
  pub const fn daemon_is_running(self) -> bool {
    matches!(self.connection, ConnectionStatus::Connected)
  }

  pub const fn caddy_is_running(self) -> bool {
    self.daemon_is_running() && matches!(self.caddy_status, ProtocolRuntimeStatus::Running)
  }

  pub const fn daemon_is_offline(self) -> bool {
    matches!(self.connection, ConnectionStatus::Offline)
  }
}

impl ConnectionStatus {
  const fn can_start_daemon(self) -> bool {
    matches!(self, Self::Offline)
  }
}

#[cfg(test)]
mod tests;
