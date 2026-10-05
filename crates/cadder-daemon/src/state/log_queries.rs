use super::*;

impl DaemonState {
  pub async fn query_logs(&self, request: cadder_ipc::QueryLogsPayload) -> QueryLogsResponse {
    let active = self.stream_is_active(&request.stream).await;
    let result = self
      .logs
      .query(
        LogQuery {
          stream: request.stream.clone(),
          limit: request.limit.unwrap_or(100).clamp(1, 200),
        },
        active,
      )
      .await;

    QueryLogsResponse {
      request_id: String::new(),
      accepted: true,
      message: "Caddy logs returned.".to_string(),
      stream: request.stream,
      stream_status: result.status,
      entries: result.entries,
    }
  }

  async fn stream_is_active(&self, stream: &LogStreamIdentity) -> bool {
    let inner = self.inner.lock().await;
    if stream.stream_id == "runtime" || stream.stream_id == "runtime-control" {
      return true;
    }
    inner.registrations.values().any(|registration| {
      (registration.log_stream == *stream && registration.activation_state.is_enabled())
        || registration
          .registered_domains
          .iter()
          .any(|domain| domain.log_stream == *stream && domain.activation_state.is_enabled())
    })
  }
}
