use super::*;

impl DaemonState {
  pub(super) async fn apply_registrations(
    &self,
    registrations: Vec<EntrypointRegistration>,
  ) -> ConfigState {
    let (action, runtime) = {
      let mut coordinator = self.coordinator.lock().await;
      (
        coordinator.begin_apply(&registrations),
        coordinator.runtime(),
      )
    };
    match action {
      CaddyApplyAction::Current(state) => state,
      CaddyApplyAction::Stop { attempted } => {
        if let Err(error) = runtime.stop().await {
          self.logs.append(
            LogStreamIdentity::runtime_control(),
            LogSeverity::Error,
            error.to_string(),
            LogAttributionKind::RuntimeControl,
            Some("idle-stop".to_string()),
          );
        }
        let mut coordinator = self.coordinator.lock().await;
        coordinator.finish_idle(attempted)
      }
      CaddyApplyAction::Apply {
        attempted,
        rendered,
        hash,
        source_config_paths,
      } => {
        let result = runtime.apply_config(&rendered, &self.logs).await;
        let mut coordinator = self.coordinator.lock().await;
        coordinator.finish_runtime_apply(attempted, hash, source_config_paths, result)
      }
    }
  }
}
