use super::*;

#[derive(Debug, Clone)]
pub struct MockCaddyRuntime {
  paths: RuntimePaths,
  state: Arc<StdMutex<MockCaddyRuntimeState>>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct MockCaddyRuntimeState {
  running: bool,
  applied_config_bytes: usize,
}

impl MockCaddyRuntime {
  pub fn new(paths: RuntimePaths) -> Self {
    Self {
      paths,
      state: Arc::new(StdMutex::new(MockCaddyRuntimeState::default())),
    }
  }

  pub async fn inspect(&self) -> RuntimeState {
    let state = self.state.lock().expect("mock runtime state lock poisoned");
    if state.running {
      RuntimeState {
        status: RuntimeStatus::Running,
        binary_path: Some("mock-caddy".to_string()),
        version: Some(format!("mock:{}", state.applied_config_bytes)),
        process_id: None,
        admin_endpoint: None,
        diagnostics: Vec::new(),
      }
    } else {
      RuntimeState::idle()
    }
  }

  pub async fn apply_config(&self, rendered: &[u8], logs: &CaddyLogStore) -> Result<()> {
    CaddyRuntime::Mock(self.clone())
      .apply_config(rendered, logs)
      .await
  }

  pub(super) async fn begin_apply_config(
    &self,
    rendered: &[u8],
  ) -> Result<MockRuntimeApplyReceipt> {
    let previous_config = read_effective_config(&self.paths).await?;
    let staged = StagedRuntimeConfig::stage(&self.paths, rendered).await?;
    let previous_state = self
      .state
      .lock()
      .expect("mock runtime state lock poisoned")
      .clone();

    Ok(MockRuntimeApplyReceipt {
      runtime: self.clone(),
      staged,
      previous_config,
      previous_state,
      applied_config_bytes: rendered.len(),
    })
  }

  fn accept_state(&self, applied_config_bytes: usize) {
    let mut state = self.state.lock().expect("mock runtime state lock poisoned");
    state.running = true;
    state.applied_config_bytes = applied_config_bytes;
  }

  fn restore_state(&self, previous: MockCaddyRuntimeState) {
    *self.state.lock().expect("mock runtime state lock poisoned") = previous;
  }

  async fn record_apply(&self, logs: &CaddyLogStore) {
    logs
      .append(
        LogStreamIdentity::runtime_control(),
        LogSeverity::Info,
        "mock Caddy runtime accepted effective config without starting a process",
        LogAttributionKind::RuntimeControl,
        Some("mock-apply".to_string()),
      )
      .await;
  }

  pub async fn stop(&self) -> Result<()> {
    self
      .state
      .lock()
      .expect("mock runtime state lock poisoned")
      .running = false;
    Ok(())
  }

  pub(super) async fn begin_stop(&self) -> Result<MockRuntimeStopReceipt> {
    let previous_config = read_effective_config(&self.paths).await?;
    let previous_state = self
      .state
      .lock()
      .expect("mock runtime state lock poisoned")
      .clone();
    self.stop().await?;
    Ok(MockRuntimeStopReceipt {
      runtime: self.clone(),
      previous_config,
      previous_state,
    })
  }
}

#[derive(Debug)]
pub(crate) struct MockRuntimeApplyReceipt {
  runtime: MockCaddyRuntime,
  staged: StagedRuntimeConfig,
  previous_config: Option<Vec<u8>>,
  previous_state: MockCaddyRuntimeState,
  pub(super) applied_config_bytes: usize,
}

impl MockRuntimeApplyReceipt {
  pub(super) async fn accept(&mut self, logs: &CaddyLogStore) -> Result<()> {
    self.staged.promote()?;
    self.runtime.accept_state(self.applied_config_bytes);
    self.runtime.record_apply(logs).await;
    Ok(())
  }

  pub(super) async fn rollback(self) -> Result<()> {
    restore_effective_config(&self.runtime.paths, self.previous_config.as_deref()).await?;
    self.runtime.restore_state(self.previous_state);
    Ok(())
  }
}

#[derive(Debug)]
pub(crate) struct MockRuntimeStopReceipt {
  runtime: MockCaddyRuntime,
  previous_config: Option<Vec<u8>>,
  previous_state: MockCaddyRuntimeState,
}

impl MockRuntimeStopReceipt {
  pub(super) fn accept(&mut self) -> Result<()> {
    remove_effective_config(&self.runtime.paths)
  }

  pub(super) async fn rollback(self) -> Result<()> {
    restore_effective_config(&self.runtime.paths, self.previous_config.as_deref()).await?;
    self.runtime.restore_state(self.previous_state);
    Ok(())
  }
}
