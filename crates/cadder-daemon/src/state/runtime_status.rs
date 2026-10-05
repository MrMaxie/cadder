use super::*;

impl DaemonState {
  pub async fn query_state(&self) -> QueryStateResponse {
    QueryStateResponse {
      request_id: String::new(),
      accepted: true,
      message: "State snapshot returned.".to_string(),
      snapshot: Some(self.snapshot().await),
    }
  }

  pub async fn snapshot(&self) -> GuiStateSnapshot {
    let registrations = {
      let inner = self.inner.lock().await;
      inner.registrations.values().cloned().collect::<Vec<_>>()
    };
    self.snapshot_from_parts(registrations).await
  }

  async fn snapshot_from_parts(
    &self,
    registrations: Vec<EntrypointRegistration>,
  ) -> GuiStateSnapshot {
    let (config, runtime) = {
      let coordinator = self.coordinator.lock().await;
      (coordinator.current_state(), coordinator.runtime())
    };
    GuiStateSnapshot {
      captured_at_utc: Utc::now(),
      registrations,
      runtime: runtime.inspect().await,
      config,
      storage: Some(self.storage_state()),
    }
  }
}
