use super::*;

impl DaemonState {
  pub fn new(coordinator: CaddyConfigCoordinator) -> Self {
    Self {
      inner: Arc::new(Mutex::new(DaemonInner {
        registrations: BTreeMap::new(),
      })),
      coordinator: Arc::new(Mutex::new(coordinator)),
      config_operation: Arc::new(Semaphore::new(1)),
      logs: CaddyLogStore::default(),
      database: None,
      shutdown_signal: ShutdownSignal::default(),
      operation_fences: OperationFenceAuthority::default(),
    }
  }

  pub async fn with_runtime_paths(
    coordinator: CaddyConfigCoordinator,
    paths: RuntimePaths,
  ) -> Result<Self> {
    crate::runtime_file::cleanup_stale_config_candidates(&paths)?;
    let database = Database::open(paths.storage_paths()).await?;
    let mut state = Self::new(coordinator);
    state.logs = CaddyLogStore::with_database(database.clone());
    state.database = Some(database);
    Ok(state)
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

  pub(crate) fn storage_state(&self) -> cadder_ipc::StorageState {
    let mut state = self
      .database
      .as_ref()
      .map(Database::state)
      .unwrap_or_else(|| cadder_ipc::StorageState {
        backend: "memory".to_string(),
        path: None,
        schema_version: 0,
        diagnostics: Vec::new(),
      });
    if let Some(message) = self.logs.durability_diagnostic() {
      state.diagnostics.push(cadder_ipc::RuntimeDiagnostic {
        code: "log-durability-failed".to_string(),
        message,
        operation: Some("persist-log".to_string()),
      });
    }
    state
  }
}
