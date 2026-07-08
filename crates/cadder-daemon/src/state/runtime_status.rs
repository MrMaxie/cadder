use super::*;

impl DaemonState {
  pub async fn query_state(&self, request_id: String) -> QueryStateResponse {
    QueryStateResponse {
      request_id,
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

  pub(super) async fn publish_change(
    &self,
    kind: StateChangeKind,
    registration_id: Option<String>,
  ) {
    let _publish = self.publish_operation.lock().await;
    let (sequence, registrations) = {
      let mut inner = self.inner.lock().await;
      inner.sequence += 1;
      (
        inner.sequence,
        inner.registrations.values().cloned().collect::<Vec<_>>(),
      )
    };
    let event = StateChangedEvent {
      request_id: "state-change".to_string(),
      sequence_number: sequence,
      change_kind: kind,
      snapshot: self.snapshot_from_parts(registrations).await,
      registration_id,
    };
    let _ = self.events.send(event);
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
      storage: Some(self.store.state()),
    }
  }
}
