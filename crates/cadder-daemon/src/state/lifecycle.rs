use super::iis_handoff::{backend_dial, legacy_backend_binding};
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
      config_operation: Arc::new(Mutex::new(())),
      publish_operation: Arc::new(Mutex::new(())),
      events,
      logs: CaddyLogStore::default(),
      store: RuntimeStore::memory(),
      autostart: AutostartManager::disabled(),
      iis_provider: IisProvider::system(),
      iis_store: IisMetadataStore::memory(),
      iis_operation: Arc::new(Mutex::new(())),
      shutdown_signal: ShutdownSignal::default(),
      operation_fences: OperationFenceAuthority::default(),
    }
  }

  pub async fn with_runtime_paths(
    mut coordinator: CaddyConfigCoordinator,
    paths: RuntimePaths,
  ) -> Result<Self> {
    let iis_store = IisMetadataStore::load(paths.metadata_path()).await?;
    let store = RuntimeStore::open(paths.storage_path());
    let handoffs = iis_store.snapshot().await;
    for (binding_id, restore) in &handoffs {
      let backend_binding = legacy_backend_binding(restore);
      coordinator.set_iis_proxy_route(
        binding_id.clone(),
        restore.domain_key.clone(),
        backend_dial(&backend_binding),
        IisProxyBackendProtocol::from_iis_protocol(&backend_binding.protocol),
      );
    }
    let mut state = Self::new(coordinator);
    state.store = store;
    state.autostart = AutostartManager::new(&paths);
    state.iis_store = iis_store;
    if !handoffs.is_empty() {
      let _operation = state.config_operation.lock().await;
      let fence = state.issue_operation_fence()?;
      state.apply_registrations_fenced(Vec::new(), &fence).await?;
    }
    Ok(state)
  }

  #[cfg(test)]
  pub(crate) fn with_iis_provider(
    coordinator: CaddyConfigCoordinator,
    iis_provider: IisProvider,
  ) -> Self {
    let mut state = Self::new(coordinator);
    state.iis_provider = iis_provider;
    state
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

  pub fn logs(&self) -> CaddyLogStore {
    self.logs.clone()
  }
}
