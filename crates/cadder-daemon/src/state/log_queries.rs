use super::*;

impl DaemonState {
  pub async fn query_logs(&self, request: cadder_protocol::QueryLogsRequest) -> QueryLogsResponse {
    let active = self.stream_is_active(&request.stream).await;
    let result = self.logs.query(
      LogQuery {
        stream: request.stream.clone(),
        limit: request.limit.unwrap_or(100).clamp(1, 500),
        after_sequence: request
          .cursor
          .as_deref()
          .and_then(|cursor| cursor.strip_prefix("seq:"))
          .and_then(|sequence| sequence.parse::<u64>().ok()),
        minimum_severity: request.minimum_severity,
      },
      active,
    );

    QueryLogsResponse {
      request_id: request.request_id,
      accepted: true,
      message: "Caddy logs returned.".to_string(),
      stream: request.stream,
      stream_status: result.status,
      entries: result.entries,
      next_cursor: result.next_cursor,
      has_gap: result.has_gap,
      has_more_before: result.has_more_before,
      truncated_by_retention: result.truncated_by_retention,
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
