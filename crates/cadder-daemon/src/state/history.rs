use super::*;

impl DaemonState {
  pub async fn query_history(
    &self,
    request: cadder_protocol::QueryHistoryRequest,
  ) -> QueryHistoryResponse {
    QueryHistoryResponse {
      request_id: request.request_id,
      accepted: true,
      message: "Runtime history returned.".to_string(),
      records: self
        .store
        .query_history(request.kind, request.limit.unwrap_or(100))
        .await,
      storage: Some(self.store.state()),
    }
  }
}
