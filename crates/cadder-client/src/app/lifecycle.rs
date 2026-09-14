use crate::data::MutationTarget;

use super::*;

impl App {
  pub fn apply_refresh(&mut self, outcome: RefreshOutcome) {
    match outcome {
      RefreshOutcome::Connected { snapshot, logs } => {
        self.last_start_failed = false;
        self.runtime_status = RuntimeStatus {
          connection: ConnectionStatus::Connected,
          caddy_status: snapshot.runtime.status,
        };
        self.connection_message = "Connected".to_string();
        self.connection_guidance = None;
        self.data.replace_snapshot(*snapshot);
        self.ensure_selected_row();
        match logs {
          Ok(logs) => self.logs.replace(logs),
          Err(error) => self.logs.set_notice(error.message),
        }
      }
      RefreshOutcome::Unavailable {
        connection,
        message,
        guidance,
      } => {
        if self.last_start_failed {
          return;
        }
        self.runtime_status = RuntimeStatus {
          connection,
          caddy_status: ProtocolRuntimeStatus::Unknown,
        };
        self.connection_message = message;
        self.connection_guidance = guidance;
        self.data.clear_snapshot();
        self.ensure_selected_row();
      }
    }
  }

  pub fn complete_mutation(&mut self, result: Result<(), ActionFailure>) -> bool {
    self.finish_action(result)
  }

  pub fn prepare_start_daemon(&mut self) -> bool {
    if !self.can_start_daemon() {
      return false;
    }
    self.last_start_failed = false;
    self.begin_action("Starting Cadder...");
    true
  }

  pub fn prepare_lifecycle(&mut self, action: LifecycleAction) {
    if self.is_pending() || self.runtime_status.connection != ConnectionStatus::Connected {
      return;
    }
    self.confirmation = Some(action);
  }

  pub fn cancel_confirmation(&mut self) {
    self.confirmation = None;
  }

  pub fn confirm_lifecycle(&mut self) -> Option<LifecycleAction> {
    let action = self.confirmation.take()?;
    let message = match action {
      LifecycleAction::Stop => "Stopping Cadder...",
      LifecycleAction::Restart => "Restarting Cadder...",
    };
    self.begin_action(message);
    Some(action)
  }

  pub const fn confirmation(&self) -> Option<LifecycleAction> {
    self.confirmation
  }

  pub const fn can_start_daemon(&self) -> bool {
    !self.is_pending() && self.runtime_status.connection.can_start_daemon()
  }

  pub const fn is_pending(&self) -> bool {
    self.pending_message.is_some()
  }

  pub fn complete_daemon_start(&mut self, result: Result<(), ActionFailure>) -> bool {
    if let Err(error) = &result {
      self.last_start_failed = true;
      self.connection_message.clone_from(&error.message);
      self.connection_guidance.clone_from(&error.guidance);
    }
    self.finish_action(result)
  }

  pub fn complete_lifecycle(&mut self, result: Result<(), ActionFailure>) -> bool {
    self.finish_action(result)
  }

  pub fn notice(&self) -> Option<String> {
    if let Some(action) = self.confirmation {
      let prompt = match action {
        LifecycleAction::Stop => "Stop Cadder and its owned Caddy process?",
        LifecycleAction::Restart => "Restart Cadder and its owned Caddy process?",
      };
      return Some(format!("{prompt}  Enter confirm  Esc cancel"));
    }
    if let Some(message) = self.pending_message {
      return Some(message.to_string());
    }
    if let Some(notice) = &self.notice {
      return Some(notice.clone());
    }
    if self.runtime_status.connection != ConnectionStatus::Connected {
      return Some(self.connection_guidance.as_deref().map_or_else(
        || self.connection_message.clone(),
        |guidance| format!("{}  {guidance}", self.connection_message),
      ));
    }
    None
  }

  pub fn prepare_toggle_current(&mut self) -> Option<MutationTarget> {
    if self.is_pending() || self.runtime_status.connection != ConnectionStatus::Connected {
      return None;
    }
    let entity = self.selected_entity()?;
    let target = self.data.mutation_target(&entity)?;
    self.begin_action("Applying route change...");
    Some(target)
  }

  fn begin_action(&mut self, message: &'static str) {
    self.notice = None;
    self.pending_message = Some(message);
  }

  fn finish_action(&mut self, result: Result<(), ActionFailure>) -> bool {
    self.pending_message = None;
    match result {
      Ok(()) => {
        self.notice = None;
        true
      }
      Err(error) => {
        self.notice = Some(error.guidance.map_or_else(
          || error.message.clone(),
          |guidance| format!("{}  {guidance}", error.message),
        ));
        false
      }
    }
  }
}
