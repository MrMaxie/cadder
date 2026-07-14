use super::*;

impl DaemonState {
  pub async fn query_autostart(&self, request_id: String) -> QueryAutostartResponse {
    let view = self.autostart.query();
    QueryAutostartResponse {
      request_id,
      accepted: !matches!(view.status, cadder_ipc::AutostartStatus::Unsupported),
      message: "Autostart status returned.".to_string(),
      mode: view.mode,
      status: view.status,
      target: view.target,
      diagnostics: view.diagnostics,
    }
  }

  pub async fn set_autostart(&self, request: SetAutostartRequest) -> SetAutostartResponse {
    let request_id = request.request_id.clone();
    let mode = request.mode;
    let fence = match self.issue_operation_fence() {
      Ok(fence) => fence,
      Err(error) => return autostart_mutation_rejected(request_id, mode, error),
    };
    self
      .set_autostart_fenced(request, &fence)
      .await
      .unwrap_or_else(|error| autostart_mutation_rejected(request_id, mode, error))
  }

  pub(crate) async fn set_autostart_fenced(
    &self,
    request: SetAutostartRequest,
    fence: &OperationFence,
  ) -> Result<SetAutostartResponse, CommitRejection> {
    fence.commit_final(|| ())?;
    Ok(SetAutostartResponse {
      request_id: request.request_id,
      accepted: false,
      message: "Autostart changes are unavailable on this installation.".to_string(),
      mode: request.mode,
      status: cadder_ipc::AutostartStatus::Unsupported,
      target: None,
      diagnostics: vec![cadder_ipc::AutostartDiagnostic {
        code: "autostart-update-unavailable".to_string(),
        message: "Configure startup manually, or retry after upgrading Cadder.".to_string(),
      }],
    })
  }
}

fn autostart_mutation_rejected(
  request_id: String,
  mode: cadder_ipc::AutostartMode,
  error: CommitRejection,
) -> SetAutostartResponse {
  SetAutostartResponse {
    request_id,
    accepted: false,
    message: format!("Daemon mutation rejected: {error}."),
    mode,
    status: cadder_ipc::AutostartStatus::Unknown,
    target: None,
    diagnostics: vec![cadder_ipc::AutostartDiagnostic {
      code: "autostart-update-rejected".to_string(),
      message: "Cadder did not change the autostart configuration.".to_string(),
    }],
  }
}
