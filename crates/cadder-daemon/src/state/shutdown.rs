use super::*;

impl DaemonState {
  pub async fn shutdown(&self) -> BasicResponse {
    let _operation = self.config_operation.lock().await;
    let runtime = {
      let coordinator = self.coordinator.lock().await;
      coordinator.runtime()
    };
    let result = runtime.stop().await;
    if result.is_ok() {
      self.store.record_history(
        HistoryKind::Runtime,
        "Daemon shutdown requested.",
        None,
        None,
        &serde_json::json!({ "accepted": true }),
      );
      self.shutdown_signal.request();
    }
    BasicResponse {
      request_id: "shutdown".to_string(),
      accepted: result.is_ok(),
      message: result
        .map(|_| "Daemon shutdown requested.".to_string())
        .unwrap_or_else(|error| error.to_string()),
    }
  }
}
