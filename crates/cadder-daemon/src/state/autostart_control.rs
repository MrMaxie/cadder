use super::*;

impl DaemonState {
  pub async fn query_autostart(&self, request_id: String) -> QueryAutostartResponse {
    let view = self.autostart.query();
    QueryAutostartResponse {
      request_id,
      accepted: !matches!(view.status, cadder_protocol::AutostartStatus::Unsupported),
      message: "Autostart status returned.".to_string(),
      mode: view.mode,
      status: view.status,
      target: view.target,
      diagnostics: view.diagnostics,
    }
  }

  pub async fn set_autostart(&self, request: SetAutostartRequest) -> SetAutostartResponse {
    let view = self.autostart.set(request.mode);
    self.store.record_history(
      HistoryKind::Autostart,
      format!("Set autostart mode to {:?}.", request.mode),
      None,
      None,
      &serde_json::json!({
        "mode": request.mode,
        "status": view.status,
        "target": view.target
      }),
    );
    SetAutostartResponse {
      request_id: request.request_id,
      accepted: matches!(
        view.status,
        cadder_protocol::AutostartStatus::Disabled | cadder_protocol::AutostartStatus::Enabled
      ),
      message: "Autostart mode updated.".to_string(),
      mode: view.mode,
      status: view.status,
      target: view.target,
      diagnostics: view.diagnostics,
    }
  }
}
