use crate::{
  CaddyConfigCoordinator,
  autostart::AutostartManager,
  caddy::CaddyApplyAction,
  logs::{CaddyLogStore, LogQuery},
  operation_fence::{CommitRejection, OperationFence, OperationFenceAuthority},
  paths::RuntimePaths,
  storage::RuntimeStore,
};
use anyhow::Result;
use cadder_ipc::{
  ActivationState, BasicResponse, EntrypointRegistration, GuiStateSnapshot,
  HeartbeatEntrypointRequest, HistoryKind, LogAttributionKind, LogSeverity, LogStreamIdentity,
  QueryAutostartResponse, QueryHistoryResponse, QueryLogsResponse, QueryStateResponse,
  RegisterEntrypointResponse, SetAutostartRequest, SetAutostartResponse, SetDomainEnabledRequest,
  SetEntrypointEnabledRequest, StateChangeKind, StateChangedEvent,
};
use chrono::Utc;
use std::{
  collections::BTreeMap,
  sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
  },
};
use tokio::sync::{Mutex, Notify, Semaphore, broadcast};

#[cfg(test)]
use cadder_ipc::ConfigApplyStatus;

mod autostart_control;
mod history;
mod lifecycle;
mod log_queries;
mod registrations;
mod runtime_status;
mod shutdown;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub struct DaemonState {
  inner: Arc<Mutex<DaemonInner>>,
  coordinator: Arc<Mutex<CaddyConfigCoordinator>>,
  config_operation: Arc<Semaphore>,
  publish_operation: Arc<Mutex<()>>,
  events: broadcast::Sender<StateChangedEvent>,
  logs: CaddyLogStore,
  store: RuntimeStore,
  autostart: AutostartManager,
  shutdown_signal: ShutdownSignal,
  operation_fences: OperationFenceAuthority,
  #[cfg(test)]
  registration_publish_hook: Option<RegistrationPublishTestHook>,
}

#[derive(Debug)]
struct DaemonInner {
  registrations: BTreeMap<String, EntrypointRegistration>,
  sequence: u64,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct RegistrationPublishTestHook {
  reached: Arc<Semaphore>,
  release: Arc<Semaphore>,
}

#[cfg(test)]
impl RegistrationPublishTestHook {
  pub(crate) fn new() -> Self {
    Self {
      reached: Arc::new(Semaphore::new(0)),
      release: Arc::new(Semaphore::new(0)),
    }
  }

  async fn pause(&self) {
    self.reached.add_permits(1);
    self
      .release
      .acquire()
      .await
      .expect("register publish test hook closed")
      .forget();
  }

  pub(crate) async fn wait_until_reached(&self) {
    self
      .reached
      .acquire()
      .await
      .expect("register publish test hook closed")
      .forget();
  }

  pub(crate) fn release(&self) {
    self.release.add_permits(1);
  }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ShutdownSignal {
  requested: Arc<AtomicBool>,
  started_at: Arc<OnceLock<tokio::time::Instant>>,
  notify: Arc<Notify>,
}

impl ShutdownSignal {
  fn prepare(&self, started_at: tokio::time::Instant) {
    let _ = self.started_at.set(started_at);
  }

  fn request(&self) {
    self.prepare(tokio::time::Instant::now());
    if !self.requested.swap(true, Ordering::SeqCst) {
      self.notify.notify_waiters();
    }
  }

  pub(crate) fn started_at(&self) -> Option<tokio::time::Instant> {
    self.started_at.get().copied()
  }

  pub async fn wait(&self) {
    let notified = self.notify.notified();
    tokio::pin!(notified);
    if self.requested.load(Ordering::SeqCst) {
      return;
    }
    notified.await;
  }
}
