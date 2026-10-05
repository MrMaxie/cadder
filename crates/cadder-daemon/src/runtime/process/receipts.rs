use super::*;

#[derive(Debug)]
pub(crate) struct ProcessRuntimeApplyReceipt {
  pub(in crate::runtime) runtime: ProcessRuntime,
  pub(super) staged: StagedRuntimeConfig,
  pub(super) previous_config: Option<Vec<u8>>,
  pub(super) was_running: bool,
}

impl ProcessRuntimeApplyReceipt {
  pub(in crate::runtime) fn accept(&mut self) -> Result<()> {
    self.staged.promote()
  }

  pub(in crate::runtime) async fn rollback(self, logs: &CaddyLogStore) -> Result<()> {
    let Self {
      runtime,
      staged,
      previous_config,
      was_running,
    } = self;
    let rollback: Result<()> = async {
      if was_running {
        let previous = previous_config.as_deref().ok_or_else(|| {
          anyhow::anyhow!("running Caddy runtime did not have a previous effective config")
        })?;
        let mut rollback_config = StagedRuntimeConfig::stage(&runtime.paths, previous).await?;
        runtime.reload(rollback_config.path(), logs).await?;
        rollback_config.promote()
      } else {
        runtime.stop().await?;
        restore_effective_config(&runtime.paths, previous_config.as_deref()).await
      }
    }
    .await;
    drop(staged);
    if let Err(error) = rollback {
      let stop_error = runtime.stop().await.err();
      let restore_error = restore_effective_config(&runtime.paths, previous_config.as_deref())
        .await
        .err();
      return Err(error.context(format!(
        "runtime rollback entered fail-closed cleanup; stop error: {}; file restore error: {}",
        stop_error
          .as_ref()
          .map_or_else(|| "none".to_string(), |error| format!("{error:#}")),
        restore_error
          .as_ref()
          .map_or_else(|| "none".to_string(), |error| format!("{error:#}"))
      )));
    }
    Ok(())
  }
}

#[derive(Debug)]
pub(crate) struct ProcessRuntimeStopReceipt {
  pub(super) runtime: ProcessRuntime,
  pub(super) previous_config: Option<Vec<u8>>,
  pub(super) was_running: bool,
}

impl ProcessRuntimeStopReceipt {
  pub(in crate::runtime) fn accept(&mut self) -> Result<()> {
    remove_effective_config(&self.runtime.paths)
  }

  pub(in crate::runtime) async fn rollback(self, logs: &CaddyLogStore) -> Result<()> {
    let Self {
      runtime,
      previous_config,
      was_running,
    } = self;
    let rollback: Result<()> = async {
      if was_running {
        let previous = previous_config.as_deref().ok_or_else(|| {
          anyhow::anyhow!("running Caddy runtime did not have a previous effective config")
        })?;
        let mut rollback_config = StagedRuntimeConfig::stage(&runtime.paths, previous).await?;
        runtime.start(rollback_config.path(), logs).await?;
        rollback_config.promote()
      } else {
        restore_effective_config(&runtime.paths, previous_config.as_deref()).await
      }
    }
    .await;
    if let Err(error) = rollback {
      let stop_error = runtime.stop().await.err();
      let restore_error = restore_effective_config(&runtime.paths, previous_config.as_deref())
        .await
        .err();
      return Err(error.context(format!(
        "runtime stop rollback entered fail-closed cleanup; stop error: {}; file restore error: {}",
        stop_error
          .as_ref()
          .map_or_else(|| "none".to_string(), |error| format!("{error:#}")),
        restore_error
          .as_ref()
          .map_or_else(|| "none".to_string(), |error| format!("{error:#}"))
      )));
    }
    Ok(())
  }
}
