use super::*;

impl DaemonState {
  pub fn new(coordinator: CaddyConfigCoordinator) -> Self {
    let (events, _) = broadcast::channel(256);
    Self {
      inner: Arc::new(Mutex::new(DaemonInner {
        registrations: BTreeMap::new(),
        sequence: 0,
      })),
      coordinator: Arc::new(Mutex::new(coordinator)),
      config_operation: Arc::new(Semaphore::new(1)),
      publish_operation: Arc::new(Mutex::new(())),
      events,
      logs: CaddyLogStore::default(),
      store: RuntimeStore::memory(),
      autostart: AutostartManager::disabled(),
      shutdown_signal: ShutdownSignal::default(),
      operation_fences: OperationFenceAuthority::default(),
      #[cfg(test)]
      registration_publish_hook: None,
    }
  }

  pub async fn with_runtime_paths(
    coordinator: CaddyConfigCoordinator,
    paths: RuntimePaths,
  ) -> Result<Self> {
    crate::runtime_file::cleanup_stale_config_candidates(&paths)?;
    let store = RuntimeStore::try_open(paths.storage_paths())?;
    let mut state = Self::new(coordinator);
    state.store = store;
    state.autostart = AutostartManager::new(&paths);
    Ok(state)
  }

  pub fn subscribe(&self) -> broadcast::Receiver<StateChangedEvent> {
    self.events.subscribe()
  }

  pub(crate) fn shutdown_signal(&self) -> ShutdownSignal {
    self.shutdown_signal.clone()
  }

  pub(crate) fn issue_operation_fence(&self) -> Result<OperationFence, CommitRejection> {
    self.operation_fences.issue()
  }

  pub(crate) fn begin_operation_drain(&self) -> u64 {
    self.operation_fences.begin_drain()
  }

  #[cfg(test)]
  pub(crate) fn set_registration_publish_hook(&mut self, hook: RegistrationPublishTestHook) {
    self.registration_publish_hook = Some(hook);
  }

  #[cfg(test)]
  pub(crate) fn set_runtime_store_for_test(&mut self, store: RuntimeStore) {
    self.store = store;
  }

  pub fn logs(&self) -> CaddyLogStore {
    self.logs.clone()
  }
}
