use crate::{
  CaddyConfigCoordinator,
  autostart::AutostartManager,
  caddy::{CaddyApplyAction, IisProxyBackendProtocol},
  iis::{
    IisBindingRecord, IisMetadataStore, IisProvider, IisRestoreRecord, binding_to_view,
    unsupported_binding_issue,
  },
  logs::{CaddyLogStore, LogQuery},
  operation_fence::{CommitRejection, OperationFence, OperationFenceAuthority},
  paths::RuntimePaths,
  storage::RuntimeStore,
};
use anyhow::Result;
use cadder_protocol::{
  ActivationState, BasicResponse, ConfigState, EntrypointRegistration, GuiStateSnapshot,
  HeartbeatEntrypointRequest, HistoryKind, IisBinding, IisFollowUpAction, IisHandoffState,
  IisIssue, IisIssueKind, IisOperationStep, LogAttributionKind, LogSeverity, LogStreamIdentity,
  QueryAutostartResponse, QueryHistoryResponse, QueryIisBindingsResponse, QueryLogsResponse,
  QueryStateResponse, RegisterEntrypointResponse, SetAutostartRequest, SetAutostartResponse,
  SetDomainEnabledRequest, SetEntrypointEnabledRequest, SetIisHandoffRequest,
  SetIisHandoffResponse, StateChangeKind, StateChangedEvent, canonicalize_domain,
};
use chrono::Utc;
use std::{
  collections::{BTreeMap, BTreeSet},
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
};
use tokio::sync::{Mutex, Notify, Semaphore, broadcast};

#[cfg(test)]
use crate::iis::IisMutation;
#[cfg(test)]
use cadder_protocol::{ConfigApplyStatus, IisElevationApproval, IisOperationStepStatus};

mod autostart_control;
mod config_apply;
mod history;
mod iis_handoff;
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
  iis_provider: IisProvider,
  iis_store: IisMetadataStore,
  #[cfg(test)]
  iis_operation: Arc<Mutex<()>>,
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
  notify: Arc<Notify>,
}

impl ShutdownSignal {
  fn request(&self) {
    if !self.requested.swap(true, Ordering::SeqCst) {
      self.notify.notify_waiters();
    }
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
