use crate::{
  CaddyConfigCoordinator,
  caddy::CaddyApplyAction,
  database::Database,
  logs::{CaddyLogStore, LogQuery},
  operation_fence::{CommitRejection, OperationFence, OperationFenceAuthority},
  paths::RuntimePaths,
};
use anyhow::Result;
use cadder_ipc::{
  ActivationState, BasicResponse, EntrypointRegistration, GuiStateSnapshot,
  HeartbeatEntrypointPayload, LogAttributionKind, LogSeverity, LogStreamIdentity,
  QueryLogsResponse, QueryStateResponse, RegisterEntrypointResponse, SetDomainEnabledPayload,
  SetEntrypointEnabledPayload,
};
use chrono::Utc;
use std::{
  collections::BTreeMap,
  sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
  },
};
use tokio::sync::{Mutex, Notify, Semaphore};

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
  logs: CaddyLogStore,
  database: Option<Database>,
  shutdown_signal: ShutdownSignal,
  operation_fences: OperationFenceAuthority,
}

#[derive(Debug)]
struct DaemonInner {
  registrations: BTreeMap<String, EntrypointRegistration>,
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
