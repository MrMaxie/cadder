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
        self.logs.set_notice(format!(
          "{}\r\n\r\n{}",
          self.connection_message,
          self.connection_guidance.as_deref().unwrap_or_default()
        ));
      }
    }
  }

  pub fn complete_mutation(&mut self, result: Result<(), ActionFailure>) -> bool {
    self.pending = false;
    match result {
      Ok(_) => {
        self.details = None;
        true
      }
      Err(error) => {
        self.show_action_failure(error);
        false
      }
    }
  }

  pub fn prepare_start_daemon(&mut self) -> bool {
    if !self.can_start_daemon() {
      return false;
    }
    self.last_start_failed = false;
    self.pending = true;
    true
  }

  pub fn prepare_lifecycle(&mut self, action: LifecycleAction) {
    if self.pending || self.runtime_status.connection != ConnectionStatus::Connected {
      return;
    }
    let (title, prompt) = match action {
      LifecycleAction::Stop => (" Confirm stop ", "Stop Cadder and its owned Caddy process?"),
      LifecycleAction::Restart => (
        " Confirm restart ",
        "Restart Cadder and its owned Caddy process?",
      ),
    };
    self.confirmation = Some(action);
    self.details = Some(DetailsState::new(
      title.to_string(),
      vec![
        prompt.to_string(),
        "Press Enter to confirm or Esc to cancel.".to_string(),
      ],
    ));
  }

  pub fn confirm_lifecycle(&mut self) -> Option<LifecycleAction> {
    let action = self.confirmation.take()?;
    self.pending = true;
    self.details = Some(DetailsState::new(
      " Applying action ".to_string(),
      vec!["Waiting for Cadder to complete the lifecycle action...".to_string()],
    ));
    Some(action)
  }

  pub fn prepare_start_daemon_from_status(&mut self) -> bool {
    self.active_tab == Tab::Status && self.prepare_start_daemon()
  }

  pub const fn can_start_daemon(&self) -> bool {
    !self.pending && self.runtime_status.connection.can_start_daemon()
  }

  pub const fn is_starting_daemon(&self) -> bool {
    self.pending && self.runtime_status.connection.can_start_daemon()
  }

  pub const fn is_pending(&self) -> bool {
    self.pending
  }

  pub fn shows_status_screen(&self) -> bool {
    self.runtime_status.connection != ConnectionStatus::Connected
  }

  pub fn complete_daemon_start(&mut self, result: Result<(), ActionFailure>) -> bool {
    self.pending = false;
    match result {
      Ok(()) => {
        self.details = None;
        true
      }
      Err(error) => {
        self.last_start_failed = true;
        self.connection_message = error.message;
        self.connection_guidance = error.guidance;
        let notice = self.connection_guidance.as_deref().map_or_else(
          || self.connection_message.clone(),
          |guidance| format!("{}\r\n\r\n{guidance}", self.connection_message),
        );
        self.logs.set_notice(notice);
        false
      }
    }
  }

  pub fn complete_lifecycle(&mut self, result: Result<(), ActionFailure>) -> bool {
    self.pending = false;
    match result {
      Ok(()) => {
        self.details = None;
        true
      }
      Err(error) => {
        self.show_action_failure(error);
        false
      }
    }
  }

  pub fn status_message(&self) -> &str {
    &self.connection_message
  }

  pub fn connection_guidance(&self) -> Option<&str> {
    self.connection_guidance.as_deref()
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

  fn show_action_failure(&mut self, error: ActionFailure) {
    let mut lines = vec![error.message];
    if let Some(guidance) = error.guidance {
      lines.push(guidance);
    }
    self.details = Some(DetailsState::new(" Action failed ".to_string(), lines));
  }
}
