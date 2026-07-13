use super::*;

impl DaemonState {
  pub(super) async fn apply_registrations(
    &self,
    registrations: Vec<EntrypointRegistration>,
  ) -> ConfigState {
    self
      .apply_registrations_inner(registrations, None)
      .await
      .expect("unfenced configuration application cannot be rejected")
  }

  pub(super) async fn apply_registrations_fenced(
    &self,
    registrations: Vec<EntrypointRegistration>,
    fence: &OperationFence,
  ) -> Result<ConfigState, CommitRejection> {
    self
      .apply_registrations_inner(registrations, Some(fence))
      .await
  }

  async fn apply_registrations_inner(
    &self,
    registrations: Vec<EntrypointRegistration>,
    fence: Option<&OperationFence>,
  ) -> Result<ConfigState, CommitRejection> {
    let (action, runtime) = {
      let mut coordinator = self.coordinator.lock().await;
      commit_if_fenced(fence, || {
        (
          coordinator.begin_apply(&registrations),
          coordinator.runtime(),
        )
      })?
    };
    match action {
      CaddyApplyAction::Current(state) => Ok(state),
      CaddyApplyAction::Stop { attempted } => {
        if let Err(error) = runtime.stop().await {
          commit_if_fenced(fence, || {
            self.logs.append(
              LogStreamIdentity::runtime_control(),
              LogSeverity::Error,
              error.to_string(),
              LogAttributionKind::RuntimeControl,
              Some("idle-stop".to_string()),
            );
          })?;
        }
        let mut coordinator = self.coordinator.lock().await;
        commit_if_fenced(fence, || coordinator.finish_idle(attempted))
      }
      CaddyApplyAction::Apply {
        attempted,
        rendered,
        hash,
        source_config_paths,
      } => {
        let result = runtime.apply_config(&rendered, &self.logs).await;
        let mut coordinator = self.coordinator.lock().await;
        commit_if_fenced(fence, || {
          coordinator.finish_runtime_apply(attempted, hash, source_config_paths, result)
        })
      }
    }
  }
}

fn commit_if_fenced<T>(
  fence: Option<&OperationFence>,
  commit: impl FnOnce() -> T,
) -> Result<T, CommitRejection> {
  match fence {
    Some(fence) => fence.commit(commit),
    None => Ok(commit()),
  }
}
