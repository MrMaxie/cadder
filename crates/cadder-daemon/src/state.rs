use crate::{
  CaddyConfigCoordinator,
  caddy::IisProxyBackendProtocol,
  iis::{
    IisBindingRecord, IisMetadataStore, IisMutation, IisProvider, IisRestoreRecord,
    binding_to_view, unsupported_binding_issue,
  },
  logs::{CaddyLogStore, LogQuery},
  paths::RuntimePaths,
};
use anyhow::Result;
use cadder_protocol::{
  ActivationState, BasicResponse, ConfigApplyStatus, ConfigState, EntrypointRegistration,
  GuiStateSnapshot, HeartbeatEntrypointRequest, IisBinding, IisElevationApproval,
  IisFollowUpAction, IisHandoffState, IisIssue, IisIssueKind, IisOperationStep,
  IisOperationStepStatus, LogStreamIdentity, QueryIisBindingsResponse, QueryLogsResponse,
  QueryStateResponse, RegisterEntrypointResponse, RuntimeState, SetDomainEnabledRequest,
  SetEntrypointEnabledRequest, SetIisHandoffRequest, SetIisHandoffResponse, StateChangeKind,
  StateChangedEvent, canonicalize_domain,
};
use chrono::Utc;
use std::{
  collections::{BTreeMap, BTreeSet},
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
};
use tokio::sync::{Mutex, Notify, broadcast};

#[derive(Debug, Clone)]
pub struct DaemonState {
  inner: Arc<Mutex<DaemonInner>>,
  events: broadcast::Sender<StateChangedEvent>,
  logs: CaddyLogStore,
  iis_provider: IisProvider,
  iis_store: IisMetadataStore,
  iis_operation: Arc<Mutex<()>>,
  shutdown_signal: ShutdownSignal,
}

#[derive(Debug)]
struct DaemonInner {
  registrations: BTreeMap<String, EntrypointRegistration>,
  coordinator: CaddyConfigCoordinator,
  sequence: u64,
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

impl DaemonState {
  pub fn new(coordinator: CaddyConfigCoordinator) -> Self {
    let (events, _) = broadcast::channel(256);
    Self {
      inner: Arc::new(Mutex::new(DaemonInner {
        registrations: BTreeMap::new(),
        coordinator,
        sequence: 0,
      })),
      events,
      logs: CaddyLogStore::default(),
      iis_provider: IisProvider::system(),
      iis_store: IisMetadataStore::memory(),
      iis_operation: Arc::new(Mutex::new(())),
      shutdown_signal: ShutdownSignal::default(),
    }
  }

  pub async fn with_runtime_paths(
    mut coordinator: CaddyConfigCoordinator,
    paths: RuntimePaths,
  ) -> Result<Self> {
    let iis_store = IisMetadataStore::load(paths.metadata_path()).await?;
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
    state.iis_store = iis_store;
    if !handoffs.is_empty() {
      let mut inner = state.inner.lock().await;
      inner.coordinator.apply(&[], &state.logs).await;
    }
    Ok(state)
  }

  #[cfg(test)]
  fn with_iis_provider(coordinator: CaddyConfigCoordinator, iis_provider: IisProvider) -> Self {
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

  pub fn logs(&self) -> CaddyLogStore {
    self.logs.clone()
  }

  pub async fn register(
    &self,
    request_id: String,
    registration: EntrypointRegistration,
  ) -> RegisterEntrypointResponse {
    if let Err(message) = registration.validate_owner() {
      return RegisterEntrypointResponse {
        request_id,
        accepted: false,
        message,
        registration_id: None,
      };
    }

    let mut inner = self.inner.lock().await;
    let mut prepared = inner.coordinator.prepare_registration(registration).await;
    let now = Utc::now();
    prepared.created_at_utc = now;
    prepared.last_heartbeat_utc = now;
    let id = prepared.registration_id.clone();
    inner.registrations.insert(id.clone(), prepared);
    let registrations = inner.registrations.values().cloned().collect::<Vec<_>>();
    inner.coordinator.apply(&registrations, &self.logs).await;
    self
      .publish_locked(
        &mut inner,
        StateChangeKind::RegistrationsChanged,
        Some(id.clone()),
      )
      .await;

    RegisterEntrypointResponse {
      request_id,
      accepted: true,
      message: "Entrypoint registered.".to_string(),
      registration_id: Some(id),
    }
  }

  pub async fn unregister(
    &self,
    request_id: String,
    registration_id: &str,
    shim_session_nonce: &str,
  ) -> BasicResponse {
    let mut inner = self.inner.lock().await;
    let removed = inner
      .registrations
      .get(registration_id)
      .is_some_and(|registration| {
        registration.entrypoint_instance.shim_session_nonce == shim_session_nonce
      });
    if removed {
      inner.registrations.remove(registration_id);
      let registrations = inner.registrations.values().cloned().collect::<Vec<_>>();
      inner.coordinator.apply(&registrations, &self.logs).await;
      self
        .publish_locked(
          &mut inner,
          StateChangeKind::RegistrationsChanged,
          Some(registration_id.to_string()),
        )
        .await;
    }

    BasicResponse {
      request_id,
      accepted: removed,
      message: if removed {
        "Entrypoint unregistered."
      } else {
        "Entrypoint was not found for the requested owner."
      }
      .to_string(),
    }
  }

  pub async fn heartbeat(&self, request: HeartbeatEntrypointRequest) -> BasicResponse {
    let mut inner = self.inner.lock().await;
    let accepted = inner
      .registrations
      .get_mut(&request.registration_id)
      .filter(|registration| {
        registration.entrypoint_instance.shim_session_nonce == request.shim_session_nonce
      })
      .map(|registration| {
        registration.last_heartbeat_utc = Utc::now();
      })
      .is_some();
    if accepted {
      self
        .publish_locked(
          &mut inner,
          StateChangeKind::RegistrationsChanged,
          Some(request.registration_id),
        )
        .await;
    }

    BasicResponse {
      request_id: request.request_id,
      accepted,
      message: if accepted {
        "Heartbeat accepted."
      } else {
        "Entrypoint was not found for the requested owner."
      }
      .to_string(),
    }
  }

  pub async fn set_entrypoint_enabled(
    &self,
    request: SetEntrypointEnabledRequest,
  ) -> BasicResponse {
    let mut inner = self.inner.lock().await;
    let accepted = inner
      .registrations
      .get_mut(&request.registration_id)
      .filter(|registration| {
        request
          .shim_session_nonce
          .as_ref()
          .is_none_or(|nonce| registration.entrypoint_instance.shim_session_nonce == *nonce)
      })
      .map(|registration| {
        registration.activation_state = ActivationState::from_enabled(request.enabled);
      })
      .is_some();

    if accepted {
      let registrations = inner.registrations.values().cloned().collect::<Vec<_>>();
      inner.coordinator.apply(&registrations, &self.logs).await;
      self
        .publish_locked(
          &mut inner,
          StateChangeKind::RegistrationsChanged,
          Some(request.registration_id),
        )
        .await;
    }

    BasicResponse {
      request_id: request.request_id,
      accepted,
      message: if accepted {
        "Entrypoint activation updated."
      } else {
        "Entrypoint was not found."
      }
      .to_string(),
    }
  }

  pub async fn set_domain_enabled(&self, request: SetDomainEnabledRequest) -> BasicResponse {
    let mut inner = self.inner.lock().await;
    let accepted = inner
      .registrations
      .get_mut(&request.registration_id)
      .and_then(|registration| {
        registration.registered_domains.iter_mut().find(|domain| {
          domain
            .name
            .canonical
            .eq_ignore_ascii_case(&request.domain_key)
        })
      })
      .map(|domain| {
        domain.activation_state = ActivationState::from_enabled(request.enabled);
      })
      .is_some();

    if accepted {
      let registrations = inner.registrations.values().cloned().collect::<Vec<_>>();
      inner.coordinator.apply(&registrations, &self.logs).await;
      self
        .publish_locked(
          &mut inner,
          StateChangeKind::RegistrationsChanged,
          Some(request.registration_id),
        )
        .await;
    }

    BasicResponse {
      request_id: request.request_id,
      accepted,
      message: if accepted {
        "Domain activation updated."
      } else {
        "Domain was not found."
      }
      .to_string(),
    }
  }

  pub async fn query_state(&self, request_id: String) -> QueryStateResponse {
    QueryStateResponse {
      request_id,
      accepted: true,
      message: "State snapshot returned.".to_string(),
      snapshot: Some(self.snapshot().await),
    }
  }

  pub async fn query_iis_bindings(&self, request_id: String) -> QueryIisBindingsResponse {
    let records = match self.iis_provider.discover().await {
      Ok(records) => records,
      Err(issue) => {
        return QueryIisBindingsResponse {
          request_id,
          accepted: false,
          message: issue.message.clone(),
          bindings: Vec::new(),
          issue: Some(issue),
        };
      }
    };
    let handoffs = self.iis_store.snapshot().await;
    let inner = self.inner.lock().await;
    QueryIisBindingsResponse {
      request_id,
      accepted: true,
      message: "IIS bindings returned.".to_string(),
      bindings: self.iis_binding_views_locked(&inner, &records, &handoffs),
      issue: None,
    }
  }

  pub async fn set_iis_handoff(&self, request: SetIisHandoffRequest) -> SetIisHandoffResponse {
    let request_id = request.request_id.clone();
    let Ok(_operation) = self.iis_operation.try_lock() else {
      let issue = IisIssue::new(
        IisIssueKind::Busy,
        "Another IIS handoff operation is already running.",
      );
      return iis_response(request_id, false, issue.message.clone(), None, issue);
    };

    if request.enabled {
      self.enable_iis_handoff(request).await
    } else {
      self.disable_iis_handoff(request).await
    }
  }

  pub async fn query_logs(&self, request: cadder_protocol::QueryLogsRequest) -> QueryLogsResponse {
    let active = self.stream_is_active(&request.stream).await;
    let result = self.logs.query(
      LogQuery {
        stream: request.stream.clone(),
        limit: request.limit.unwrap_or(100).clamp(1, 500),
        after_sequence: request
          .cursor
          .as_deref()
          .and_then(|cursor| cursor.strip_prefix("seq:"))
          .and_then(|sequence| sequence.parse::<u64>().ok()),
        minimum_severity: request.minimum_severity,
      },
      active,
    );

    QueryLogsResponse {
      request_id: request.request_id,
      accepted: true,
      message: "Caddy logs returned.".to_string(),
      stream: request.stream,
      stream_status: result.status,
      entries: result.entries,
      next_cursor: result.next_cursor,
      has_gap: result.has_gap,
      has_more_before: result.has_more_before,
      truncated_by_retention: result.truncated_by_retention,
    }
  }

  pub async fn shutdown(&self) -> BasicResponse {
    let mut inner = self.inner.lock().await;
    let result = inner.coordinator.shutdown().await;
    if result.is_ok() {
      self.shutdown_signal.request();
    }
    BasicResponse {
      request_id: "shutdown".to_string(),
      accepted: result.is_ok(),
      message: result
        .map(|_| "Daemon shutdown requested.".to_string())
        .unwrap_or_else(|error| error.to_string()),
    }
  }

  pub async fn snapshot(&self) -> GuiStateSnapshot {
    let inner = self.inner.lock().await;
    self.snapshot_locked(&inner).await
  }

  async fn publish_locked(
    &self,
    inner: &mut DaemonInner,
    kind: StateChangeKind,
    registration_id: Option<String>,
  ) {
    inner.sequence += 1;
    let event = StateChangedEvent {
      request_id: "state-change".to_string(),
      sequence_number: inner.sequence,
      change_kind: kind,
      snapshot: self.snapshot_locked(inner).await,
      registration_id,
    };
    let _ = self.events.send(event);
  }

  async fn snapshot_locked(&self, inner: &DaemonInner) -> GuiStateSnapshot {
    GuiStateSnapshot {
      captured_at_utc: Utc::now(),
      registrations: inner.registrations.values().cloned().collect(),
      runtime: self.runtime_state_locked(inner).await,
      config: self.config_state_locked(inner),
    }
  }

  async fn runtime_state_locked(&self, inner: &DaemonInner) -> RuntimeState {
    inner.coordinator_runtime_state().await
  }

  fn config_state_locked(&self, inner: &DaemonInner) -> ConfigState {
    inner.coordinator.current_state()
  }

  async fn stream_is_active(&self, stream: &LogStreamIdentity) -> bool {
    let inner = self.inner.lock().await;
    if stream.stream_id == "runtime" || stream.stream_id == "runtime-control" {
      return true;
    }
    inner.registrations.values().any(|registration| {
      (registration.log_stream == *stream && registration.activation_state.is_enabled())
        || registration
          .registered_domains
          .iter()
          .any(|domain| domain.log_stream == *stream && domain.activation_state.is_enabled())
    })
  }

  async fn enable_iis_handoff(&self, request: SetIisHandoffRequest) -> SetIisHandoffResponse {
    let records = match self.iis_provider.discover().await {
      Ok(records) => records,
      Err(issue) => {
        return iis_response(
          request.request_id,
          false,
          issue.message.clone(),
          None,
          issue,
        );
      }
    };
    let Some(binding) = records
      .iter()
      .find(|binding| binding.binding_id() == request.binding_id)
      .cloned()
    else {
      return iis_response(
        request.request_id,
        false,
        "IIS binding was not found.",
        None,
        IisIssue::new(IisIssueKind::MissingBinding, "IIS binding was not found."),
      );
    };
    let mut steps = enable_iis_steps();
    mark_step_succeeded(&mut steps, "iis-discover-bindings");

    if let Some(issue) = unsupported_binding_issue(&binding) {
      mark_step_issue(&mut steps, "iis-classify-binding", &issue);
      let view = binding_to_view(
        &binding,
        IisHandoffState::Unsupported,
        Some(issue.clone()),
        None,
      );
      return iis_response_with_steps(
        request.request_id,
        false,
        issue.message.clone(),
        Some(view),
        issue.clone(),
        steps,
        Vec::new(),
      );
    }

    let domain_key = match route_host_for_binding(&binding, request.route_host.as_deref()) {
      Ok(domain_key) => domain_key,
      Err(issue) => {
        mark_step_issue(&mut steps, "iis-classify-binding", &issue);
        let view = binding_to_view(
          &binding,
          IisHandoffState::MissingRoute,
          Some(issue.clone()),
          None,
        );
        return iis_response_with_steps(
          request.request_id,
          false,
          issue.message.clone(),
          Some(view),
          issue.clone(),
          steps,
          Vec::new(),
        );
      }
    };
    if iis_route_host_conflicts(&records, &binding.binding_id(), &domain_key) {
      let issue = IisIssue::new(
        IisIssueKind::Conflict,
        format!("IIS host `{domain_key}` appears on multiple bindings."),
      );
      mark_step_issue(&mut steps, "iis-classify-binding", &issue);
      let view = binding_to_view(
        &binding,
        IisHandoffState::Conflict,
        Some(issue.clone()),
        None,
      );
      return iis_response_with_steps(
        request.request_id,
        false,
        issue.message.clone(),
        Some(view),
        issue.clone(),
        steps,
        Vec::new(),
      );
    }

    let registration_conflict = {
      let inner = self.inner.lock().await;
      active_registration_conflict(&inner.registrations, &domain_key)
    };
    if let Some(issue) = registration_conflict {
      mark_step_issue(&mut steps, "iis-classify-binding", &issue);
      let view = binding_to_view(
        &binding,
        IisHandoffState::Conflict,
        Some(issue.clone()),
        None,
      );
      return iis_response_with_steps(
        request.request_id,
        false,
        issue.message.clone(),
        Some(view),
        issue.clone(),
        steps,
        Vec::new(),
      );
    }
    mark_step_succeeded(&mut steps, "iis-classify-binding");

    let backend_binding =
      match binding.backend_binding(backend_port_for_binding(&binding), &domain_key) {
        Ok(backend_binding) => backend_binding,
        Err(issue) => {
          mark_step_issue(&mut steps, "iis-classify-binding", &issue);
          let view = binding_to_view(
            &binding,
            IisHandoffState::Unsupported,
            Some(issue.clone()),
            None,
          );
          return iis_response_with_steps(
            request.request_id,
            false,
            issue.message.clone(),
            Some(view),
            issue.clone(),
            steps,
            Vec::new(),
          );
        }
      };
    let restore = IisRestoreRecord {
      binding: binding.clone(),
      domain_key: domain_key.clone(),
      registration_id: None,
      backend_binding: Some(backend_binding.clone()),
    };
    if let Err(error) = self
      .iis_store
      .insert(binding.binding_id(), restore.clone())
      .await
    {
      let issue = IisIssue::new(
        IisIssueKind::ProviderError,
        format!("Could not persist IIS restore metadata: {error}"),
      );
      mark_step_issue(&mut steps, "iis-write-restore-metadata", &issue);
      return iis_response_with_steps(
        request.request_id,
        false,
        issue.message.clone(),
        None,
        issue.clone(),
        steps,
        Vec::new(),
      );
    }
    mark_step_succeeded(&mut steps, "iis-write-restore-metadata");

    let privileged_step_ids = ["iis-create-loopback-binding", "iis-remove-public-binding"];
    let privileged_reason = format!(
      "Cadder needs administrator approval to hand IIS host `{domain_key}` to Caddy by creating the loopback backend binding and removing the original public IIS binding."
    );
    if let Err(issue) = self
      .iis_provider
      .execute_privileged_batch(
        &privileged_reason,
        &[
          IisMutation::add(backend_binding.clone()),
          IisMutation::remove(binding.clone()),
        ],
      )
      .await
    {
      let _ = self.iis_store.remove(&binding.binding_id()).await;
      mark_privileged_batch_issue(&mut steps, &privileged_step_ids, &issue);
      mark_step_skipped(&mut steps, "caddy-apply-proxy-route");
      let view = binding_to_view(
        &binding,
        IisHandoffState::Available,
        Some(issue.clone()),
        None,
      );
      let mut follow_up_actions = follow_up_actions_for_issue(&issue);
      if matches!(issue.kind, IisIssueKind::ProviderError) {
        follow_up_actions.push(IisFollowUpAction::RemoveLoopbackBinding);
      }
      return iis_response_with_steps(
        request.request_id,
        false,
        issue.message.clone(),
        Some(view),
        issue.clone(),
        steps,
        follow_up_actions,
      );
    }
    mark_privileged_batch_approved(&mut steps, &privileged_step_ids);

    let apply_state = {
      let mut inner = self.inner.lock().await;
      inner.coordinator.set_iis_proxy_route(
        binding.binding_id(),
        domain_key.clone(),
        backend_dial(&backend_binding),
        IisProxyBackendProtocol::from_iis_protocol(&backend_binding.protocol),
      );
      let registrations = inner.registrations.values().cloned().collect::<Vec<_>>();
      let apply_state = inner.coordinator.apply(&registrations, &self.logs).await;
      self
        .publish_locked(&mut inner, StateChangeKind::RegistrationsChanged, None)
        .await;
      apply_state
    };

    if apply_state.status == ConfigApplyStatus::Failed {
      let apply_issue = IisIssue::new(
        IisIssueKind::ProviderError,
        "Cadder could not apply the IIS proxy route.",
      );
      mark_step_issue(&mut steps, "caddy-apply-proxy-route", &apply_issue);
      {
        let mut inner = self.inner.lock().await;
        inner.coordinator.remove_iis_proxy_route(&domain_key);
        let registrations = inner.registrations.values().cloned().collect::<Vec<_>>();
        inner.coordinator.apply(&registrations, &self.logs).await;
        self
          .publish_locked(&mut inner, StateChangeKind::RegistrationsChanged, None)
          .await;
      }
      steps.push(IisOperationStep::administrator(
        "iis-rollback-public-binding",
        "Restore original IIS binding after Caddy apply failure.",
      ));
      steps.push(IisOperationStep::administrator(
        "iis-rollback-loopback-binding",
        "Remove loopback IIS binding after Caddy apply failure.",
      ));
      let rollback_step_ids = [
        "iis-rollback-public-binding",
        "iis-rollback-loopback-binding",
      ];
      let rollback = self
        .iis_provider
        .execute_privileged_batch(
          "Cadder needs administrator approval to roll IIS back after Caddy route apply failed.",
          &[
            IisMutation::restore(binding.clone()),
            IisMutation::remove(backend_binding.clone()),
          ],
        )
        .await;
      let (issue, view) = match rollback {
        Ok(()) => {
          let _ = self.iis_store.remove(&binding.binding_id()).await;
          let issue = IisIssue::new(
            IisIssueKind::RollbackSucceeded,
            "Cadder could not apply the IIS proxy route; original IIS binding was restored.",
          );
          mark_privileged_batch_approved(&mut steps, &rollback_step_ids);
          let view = binding_to_view(
            &binding,
            IisHandoffState::Available,
            Some(issue.clone()),
            None,
          );
          (issue, view)
        }
        Err(error) => {
          let issue = IisIssue::new(
            IisIssueKind::RollbackFailed,
            format!(
              "Cadder could not apply the IIS proxy route and IIS rollback failed: {}",
              error.message
            ),
          );
          mark_privileged_batch_issue(&mut steps, &rollback_step_ids, &issue);
          let view = handoff_binding_to_view(&binding, Some(issue.clone()), &restore);
          (issue, view)
        }
      };
      let follow_up_actions = follow_up_actions_for_issue(&issue);
      return iis_response_with_steps(
        request.request_id,
        false,
        issue.message.clone(),
        Some(view),
        issue.clone(),
        steps,
        follow_up_actions,
      );
    }
    mark_step_succeeded(&mut steps, "caddy-apply-proxy-route");

    let view = binding_to_view(
      &binding,
      IisHandoffState::HandedOff,
      None,
      Some(binding.restore_summary()),
    );
    let mut view = view;
    view.domain_key = Some(domain_key.clone());
    SetIisHandoffResponse {
      request_id: request.request_id,
      accepted: true,
      message: format!(
        "IIS binding `{domain_key}` is proxied through Cadder to 127.0.0.1:{}.",
        backend_binding.port
      ),
      binding: Some(view),
      issue: None,
      steps,
      follow_up_actions: Vec::new(),
    }
  }

  async fn disable_iis_handoff(&self, request: SetIisHandoffRequest) -> SetIisHandoffResponse {
    let mut steps = disable_iis_steps();
    let handoffs = self.iis_store.snapshot().await;
    let Some(restore) = handoffs.get(&request.binding_id).cloned() else {
      let issue = IisIssue::new(
        IisIssueKind::MissingBinding,
        "IIS handoff restore metadata was not found.",
      );
      mark_step_issue(&mut steps, "iis-read-restore-metadata", &issue);
      return iis_response_with_steps(
        request.request_id,
        false,
        "IIS handoff restore metadata was not found.",
        None,
        issue.clone(),
        steps,
        Vec::new(),
      );
    };
    mark_step_succeeded(&mut steps, "iis-read-restore-metadata");
    let backend_binding = legacy_backend_binding(&restore);

    {
      let inner = self.inner.lock().await;
      if caddy_front_door_needed(&inner.registrations, &restore.domain_key)
        || inner
          .coordinator
          .has_iis_proxy_routes_except(&restore.domain_key)
      {
        let issue = IisIssue::new(
          IisIssueKind::Conflict,
          format!(
            "Cannot restore IIS binding `{}` while other Cadder routes still need the front-door port.",
            restore.domain_key
          ),
        );
        let view = binding_to_view(
          &restore.binding,
          IisHandoffState::HandedOff,
          Some(issue.clone()),
          Some(restore.binding.restore_summary()),
        );
        mark_step_issue(&mut steps, "caddy-remove-proxy-route", &issue);
        return iis_response_with_steps(
          request.request_id,
          false,
          issue.message.clone(),
          Some(view),
          issue.clone(),
          steps,
          Vec::new(),
        );
      }
    }

    {
      let mut inner = self.inner.lock().await;
      inner
        .coordinator
        .remove_iis_proxy_route(&restore.domain_key);
      let registrations = inner.registrations.values().cloned().collect::<Vec<_>>();
      inner.coordinator.apply(&registrations, &self.logs).await;
      self
        .publish_locked(&mut inner, StateChangeKind::RegistrationsChanged, None)
        .await;
    }
    mark_step_succeeded(&mut steps, "caddy-remove-proxy-route");

    let privileged_step_ids = ["iis-restore-public-binding", "iis-remove-loopback-binding"];
    if let Err(issue) = self
      .iis_provider
      .execute_privileged_batch(
        "Cadder needs administrator approval to restore the original IIS binding and remove the loopback backend binding.",
        &[
          IisMutation::restore(restore.binding.clone()),
          IisMutation::remove(backend_binding.clone()),
        ],
      )
      .await
    {
      let mut inner = self.inner.lock().await;
      inner.coordinator.set_iis_proxy_route(
        request.binding_id.clone(),
        restore.domain_key.clone(),
        backend_dial(&backend_binding),
        IisProxyBackendProtocol::from_iis_protocol(&backend_binding.protocol),
      );
      let registrations = inner.registrations.values().cloned().collect::<Vec<_>>();
      inner.coordinator.apply(&registrations, &self.logs).await;
      self
        .publish_locked(&mut inner, StateChangeKind::RegistrationsChanged, None)
        .await;
      let issue = IisIssue::new(
        IisIssueKind::RestoreFailed,
        format!(
          "Cadder restored the proxy route because the privileged IIS restore batch failed: {}",
          issue.message
        ),
      );
      mark_privileged_batch_issue(&mut steps, &privileged_step_ids, &issue);
      mark_step_skipped(&mut steps, "iis-clear-restore-metadata");
      let view = handoff_binding_to_view(&restore.binding, Some(issue.clone()), &restore);
      return iis_response_with_steps(
        request.request_id,
        false,
        issue.message.clone(),
        Some(view),
        issue.clone(),
        steps,
        follow_up_actions_for_issue(&issue),
      );
    }
    mark_privileged_batch_approved(&mut steps, &privileged_step_ids);
    if let Err(error) = self.iis_store.remove(&request.binding_id).await {
      let issue = IisIssue::new(
        IisIssueKind::ProviderError,
        format!("IIS binding was restored but restore metadata could not be cleared: {error}"),
      );
      mark_step_issue(&mut steps, "iis-clear-restore-metadata", &issue);
      return iis_response_with_steps(
        request.request_id,
        false,
        issue.message.clone(),
        None,
        issue.clone(),
        steps,
        vec![IisFollowUpAction::ClearRestoreMetadata],
      );
    }
    mark_step_succeeded(&mut steps, "iis-clear-restore-metadata");

    let view = binding_to_view(&restore.binding, IisHandoffState::Available, None, None);
    SetIisHandoffResponse {
      request_id: request.request_id,
      accepted: true,
      message: format!("IIS binding `{}` restored to IIS.", restore.domain_key),
      binding: Some(view),
      issue: None,
      steps,
      follow_up_actions: Vec::new(),
    }
  }

  fn iis_binding_views_locked(
    &self,
    inner: &DaemonInner,
    records: &[IisBindingRecord],
    handoffs: &BTreeMap<String, IisRestoreRecord>,
  ) -> Vec<IisBinding> {
    let backend_binding_ids = handoffs
      .values()
      .filter_map(|restore| restore.backend_binding.as_ref())
      .map(IisBindingRecord::binding_id)
      .collect::<BTreeSet<_>>();
    let public_records = records
      .iter()
      .filter(|binding| !backend_binding_ids.contains(&binding.binding_id()))
      .cloned()
      .collect::<Vec<_>>();
    let duplicate_hosts = duplicate_iis_hosts(&public_records);
    let mut seen = BTreeSet::new();
    let mut bindings = public_records
      .iter()
      .map(|binding| {
        let binding_id = binding.binding_id();
        seen.insert(binding_id.clone());
        if let Some(restore) = handoffs.get(&binding_id) {
          return handoff_binding_to_view(binding, None, restore);
        }
        if let Some(issue) = unsupported_binding_issue(binding) {
          return binding_to_view(binding, IisHandoffState::Unsupported, Some(issue), None);
        }
        let domain_key = match route_host_for_binding(binding, None) {
          Ok(domain_key) => domain_key,
          Err(issue) => {
            return binding_to_view(binding, IisHandoffState::MissingRoute, Some(issue), None);
          }
        };
        if duplicate_hosts.contains(&domain_key) {
          return binding_to_view(
            binding,
            IisHandoffState::Conflict,
            Some(IisIssue::new(
              IisIssueKind::Conflict,
              format!("IIS host `{domain_key}` appears on multiple bindings."),
            )),
            None,
          );
        }
        if let Err(issue) = binding.backend_binding(backend_port_for_binding(binding), &domain_key)
        {
          return binding_to_view(binding, IisHandoffState::Unsupported, Some(issue), None);
        }
        if let Some(issue) = active_registration_conflict(&inner.registrations, &domain_key) {
          binding_to_view(binding, IisHandoffState::Conflict, Some(issue), None)
        } else {
          binding_to_view(binding, IisHandoffState::Available, None, None)
        }
      })
      .collect::<Vec<_>>();

    bindings.extend(
      handoffs
        .iter()
        .filter(|(binding_id, _)| !seen.contains(*binding_id))
        .map(|(_, restore)| handoff_binding_to_view(&restore.binding, None, restore)),
    );
    bindings
  }
}

impl DaemonInner {
  async fn coordinator_runtime_state(&self) -> RuntimeState {
    self.coordinator.runtime_state().await
  }
}

fn handoff_binding_to_view(
  binding: &IisBindingRecord,
  issue: Option<IisIssue>,
  restore: &IisRestoreRecord,
) -> IisBinding {
  let mut view = binding_to_view(
    binding,
    IisHandoffState::HandedOff,
    issue,
    Some(restore.binding.restore_summary()),
  );
  view.domain_key = Some(restore.domain_key.clone());
  view
}

fn duplicate_iis_hosts(records: &[IisBindingRecord]) -> BTreeSet<String> {
  let mut counts = BTreeMap::<String, usize>::new();
  for binding in records {
    if unsupported_binding_issue(binding).is_none()
      && let Ok(domain_key) = route_host_for_binding(binding, None)
    {
      *counts.entry(domain_key).or_default() += 1;
    }
  }
  counts
    .into_iter()
    .filter_map(|(host, count)| (count > 1).then_some(host))
    .collect()
}

fn iis_route_host_conflicts(
  records: &[IisBindingRecord],
  selected_binding_id: &str,
  domain_key: &str,
) -> bool {
  duplicate_iis_hosts(records).contains(domain_key)
    || records.iter().any(|binding| {
      binding.binding_id() != selected_binding_id
        && unsupported_binding_issue(binding).is_none()
        && route_host_for_binding(binding, None)
          .is_ok_and(|host| host.eq_ignore_ascii_case(domain_key))
    })
}

fn route_host_for_binding(
  binding: &IisBindingRecord,
  route_host: Option<&str>,
) -> std::result::Result<String, IisIssue> {
  let binding_host = binding.host_header.trim();
  let selected = if binding_host.is_empty() || binding_host == "*" {
    route_host.unwrap_or_default()
  } else {
    binding_host
  };
  let candidate = extract_host_candidate(selected);
  let domain_key = canonicalize_domain(candidate);
  if domain_key.is_empty() || domain_key == "*" {
    return Err(IisIssue::new(
      IisIssueKind::MissingRoute,
      "Wildcard IIS bindings need a route host. In the TUI, enter the host with `/` before pressing Space.",
    ));
  }
  if domain_key.contains('/') || domain_key.contains('\\') || domain_key.contains(' ') {
    return Err(IisIssue::new(
      IisIssueKind::UnsupportedBindingShape,
      format!("IIS route host `{selected}` is not a valid DNS host."),
    ));
  }
  Ok(domain_key)
}

fn extract_host_candidate(raw: &str) -> &str {
  let without_scheme = raw
    .split_once("://")
    .map(|(_, rest)| rest)
    .unwrap_or(raw)
    .trim();
  let without_path = without_scheme
    .split(['/', '?', '#'])
    .next()
    .unwrap_or(without_scheme)
    .trim();
  if let Some((host, port)) = without_path.rsplit_once(':')
    && !host.is_empty()
    && port.chars().all(|ch| ch.is_ascii_digit())
  {
    return host;
  }
  without_path
}

fn backend_port_for_binding(binding: &IisBindingRecord) -> u16 {
  let hash = binding.binding_id().bytes().fold(0_u32, |hash, byte| {
    hash.wrapping_mul(33).wrapping_add(byte as u32)
  });
  41000 + (hash % 8000) as u16
}

fn legacy_backend_binding(restore: &IisRestoreRecord) -> IisBindingRecord {
  restore.backend_binding.clone().unwrap_or_else(|| {
    restore.binding.backend_http_binding(
      backend_port_for_binding(&restore.binding),
      &restore.domain_key,
    )
  })
}

fn backend_dial(binding: &IisBindingRecord) -> String {
  format!("127.0.0.1:{}", binding.port)
}

fn active_registration_conflict(
  registrations: &BTreeMap<String, EntrypointRegistration>,
  domain_key: &str,
) -> Option<IisIssue> {
  let matches = registrations
    .values()
    .filter(|registration| registration.activation_state.is_enabled())
    .filter(|registration| {
      registration
        .registered_domains
        .iter()
        .filter(|domain| domain.activation_state.is_enabled())
        .any(|domain| domain.name.canonical.eq_ignore_ascii_case(domain_key))
    })
    .map(|registration| registration.registration_id.clone())
    .collect::<Vec<_>>();
  (!matches.is_empty()).then(|| {
    IisIssue::new(
      IisIssueKind::Conflict,
      format!("Cadder already has an active route for IIS host `{domain_key}`."),
    )
  })
}

fn caddy_front_door_needed(
  registrations: &BTreeMap<String, EntrypointRegistration>,
  restoring_domain: &str,
) -> bool {
  registrations
    .values()
    .filter(|registration| registration.activation_state.is_enabled())
    .flat_map(|registration| &registration.registered_domains)
    .any(|domain| {
      domain.activation_state.is_enabled()
        && !domain.name.canonical.eq_ignore_ascii_case(restoring_domain)
    })
}

fn enable_iis_steps() -> Vec<IisOperationStep> {
  vec![
    IisOperationStep::user("iis-discover-bindings", "Discover IIS bindings."),
    IisOperationStep::user("iis-classify-binding", "Classify selected IIS binding."),
    IisOperationStep::user(
      "iis-write-restore-metadata",
      "Write IIS restore metadata before mutation.",
    ),
    IisOperationStep::administrator(
      "iis-create-loopback-binding",
      "Create loopback IIS binding for Cadder proxying.",
    ),
    IisOperationStep::administrator(
      "iis-remove-public-binding",
      "Remove original public IIS binding.",
    ),
    IisOperationStep::user("caddy-apply-proxy-route", "Apply Caddy IIS proxy route."),
  ]
}

fn disable_iis_steps() -> Vec<IisOperationStep> {
  vec![
    IisOperationStep::user("iis-read-restore-metadata", "Read IIS restore metadata."),
    IisOperationStep::user("caddy-remove-proxy-route", "Remove Caddy IIS proxy route."),
    IisOperationStep::administrator(
      "iis-restore-public-binding",
      "Restore original public IIS binding.",
    ),
    IisOperationStep::administrator(
      "iis-remove-loopback-binding",
      "Remove loopback IIS backend binding.",
    ),
    IisOperationStep::user("iis-clear-restore-metadata", "Clear IIS restore metadata."),
  ]
}

fn mark_step_succeeded(steps: &mut [IisOperationStep], step_id: &str) {
  if let Some(step) = steps.iter_mut().find(|step| step.step_id == step_id) {
    step.status = IisOperationStepStatus::Succeeded;
    step.approval = IisElevationApproval::NotRequired;
  }
}

fn mark_step_issue(steps: &mut [IisOperationStep], step_id: &str, issue: &IisIssue) {
  if let Some(step) = steps.iter_mut().find(|step| step.step_id == step_id) {
    step.status = match issue.kind {
      IisIssueKind::ElevationDenied => IisOperationStepStatus::Denied,
      IisIssueKind::ElevationUnsupported | IisIssueKind::IisUnavailable => {
        IisOperationStepStatus::Unsupported
      }
      _ => IisOperationStepStatus::Failed,
    };
    step.issue = Some(issue.clone());
  }
}

fn mark_privileged_batch_approved(steps: &mut [IisOperationStep], step_ids: &[&str]) {
  for step_id in step_ids {
    if let Some(step) = steps.iter_mut().find(|step| step.step_id == *step_id) {
      step.status = IisOperationStepStatus::Succeeded;
      step.approval = IisElevationApproval::Approved;
    }
  }
}

fn mark_privileged_batch_issue(
  steps: &mut [IisOperationStep],
  step_ids: &[&str],
  issue: &IisIssue,
) {
  for step_id in step_ids {
    if let Some(step) = steps.iter_mut().find(|step| step.step_id == *step_id) {
      step.status = match issue.kind {
        IisIssueKind::ElevationDenied => IisOperationStepStatus::Denied,
        IisIssueKind::ElevationUnsupported | IisIssueKind::IisUnavailable => {
          IisOperationStepStatus::Unsupported
        }
        _ => IisOperationStepStatus::Failed,
      };
      step.approval = match issue.kind {
        IisIssueKind::ElevationDenied => IisElevationApproval::Denied,
        IisIssueKind::ElevationUnsupported | IisIssueKind::IisUnavailable => {
          IisElevationApproval::Unsupported
        }
        _ => IisElevationApproval::Approved,
      };
      step.issue = Some(issue.clone());
    }
  }
}

fn mark_step_skipped(steps: &mut [IisOperationStep], step_id: &str) {
  if let Some(step) = steps.iter_mut().find(|step| step.step_id == step_id) {
    step.status = IisOperationStepStatus::Skipped;
  }
}

fn follow_up_actions_for_issue(issue: &IisIssue) -> Vec<IisFollowUpAction> {
  match issue.kind {
    IisIssueKind::ElevationDenied
    | IisIssueKind::ElevationRequired
    | IisIssueKind::InsufficientPrivileges => vec![IisFollowUpAction::RetryElevation],
    IisIssueKind::ElevationUnsupported => Vec::new(),
    IisIssueKind::RollbackFailed => vec![
      IisFollowUpAction::RollbackHandoff,
      IisFollowUpAction::RetryElevation,
    ],
    IisIssueKind::RestoreFailed => vec![
      IisFollowUpAction::RetryRestore,
      IisFollowUpAction::RetryElevation,
    ],
    IisIssueKind::ProviderError => vec![IisFollowUpAction::RetryElevation],
    _ => Vec::new(),
  }
}

fn iis_response(
  request_id: String,
  accepted: bool,
  message: impl Into<String>,
  binding: Option<IisBinding>,
  issue: IisIssue,
) -> SetIisHandoffResponse {
  iis_response_with_steps(
    request_id,
    accepted,
    message,
    binding,
    issue,
    Vec::new(),
    Vec::new(),
  )
}

fn iis_response_with_steps(
  request_id: String,
  accepted: bool,
  message: impl Into<String>,
  binding: Option<IisBinding>,
  issue: IisIssue,
  steps: Vec<IisOperationStep>,
  follow_up_actions: Vec<IisFollowUpAction>,
) -> SetIisHandoffResponse {
  SetIisHandoffResponse {
    request_id,
    accepted,
    message: message.into(),
    binding,
    issue: Some(issue),
    steps,
    follow_up_actions,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    CaddyConfigAdapter, CaddyConfigCoordinator, ProcessRuntime, RealCaddyResolver, RuntimePaths,
  };
  use cadder_protocol::{
    EntrypointInstanceIdentity, LogAttributionKind, LogSeverity, LogStreamIdentity,
    LogStreamStatus, OwnerProcessIdentity, QueryLogsRequest, RegisteredDomain, SourcePath,
  };
  use chrono::Utc;
  use std::{fs, path::Path};

  fn state() -> DaemonState {
    let paths = RuntimePaths::resolve(Some(tempfile::tempdir().unwrap().keep())).unwrap();
    let resolver = RealCaddyResolver::new(Some("definitely-missing-caddy".to_string()));
    let adapter = CaddyConfigAdapter::new(resolver.clone());
    let runtime = ProcessRuntime::new(resolver, paths);
    DaemonState::new(CaddyConfigCoordinator::new(adapter, runtime))
  }

  fn state_with_iis(provider: IisProvider) -> DaemonState {
    let paths = RuntimePaths::resolve(Some(tempfile::tempdir().unwrap().keep())).unwrap();
    let resolver = RealCaddyResolver::new(Some("definitely-missing-caddy".to_string()));
    let adapter = CaddyConfigAdapter::new(resolver.clone());
    let runtime = ProcessRuntime::new(resolver, paths);
    DaemonState::with_iis_provider(CaddyConfigCoordinator::new(adapter, runtime), provider)
  }

  fn state_with_fake_caddy(provider: IisProvider, caddy: &Path) -> DaemonState {
    let (state, _) = state_with_fake_caddy_paths(provider, caddy);
    state
  }

  fn state_with_fake_caddy_paths(
    provider: IisProvider,
    caddy: &Path,
  ) -> (DaemonState, RuntimePaths) {
    let paths = RuntimePaths::resolve(Some(tempfile::tempdir().unwrap().keep())).unwrap();
    let resolver = RealCaddyResolver::new(Some(caddy.display().to_string()));
    let adapter = CaddyConfigAdapter::new(resolver.clone());
    let runtime = ProcessRuntime::new(resolver, paths.clone());
    (
      DaemonState::with_iis_provider(CaddyConfigCoordinator::new(adapter, runtime), provider),
      paths,
    )
  }

  fn iis_binding(site: &str, protocol: &str, binding: &str) -> IisBindingRecord {
    IisBindingRecord::from_binding_information(site, protocol, binding).unwrap()
  }

  fn tls_certificate() -> crate::iis::IisTlsCertificate {
    crate::iis::IisTlsCertificate {
      thumbprint: "aabbcc".to_string(),
      store_name: "My".to_string(),
      ssl_flags: Some(1),
    }
  }

  fn https_iis_binding(site: &str, binding: &str) -> IisBindingRecord {
    let mut binding = iis_binding(site, "https", binding);
    binding.tls_certificate = Some(tls_certificate());
    binding
  }

  fn write_fake_caddy(path: &Path) {
    #[cfg(windows)]
    fs::write(
      path,
      r#"@echo off
if "%1"=="adapt" (
  echo {"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["app.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}
  exit /b 0
)
if "%1"=="reload" exit /b 0
if "%1"=="stop" exit /b 0
if "%1"=="run" exit /b 0
exit /b 1
"#,
    )
    .unwrap();

    #[cfg(not(windows))]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::write(
        path,
        r#"#!/usr/bin/env sh
case "$1" in
  adapt)
    printf '%s\n' '{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["app.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}'
    exit 0
    ;;
  reload|stop)
    exit 0
    ;;
  run)
    exit 0
    ;;
esac
exit 1
"#,
      )
      .unwrap();
      let mut permissions = fs::metadata(path).unwrap().permissions();
      permissions.set_mode(0o755);
      fs::set_permissions(path, permissions).unwrap();
    }
  }

  fn registration(id: &str, nonce: &str) -> EntrypointRegistration {
    let now = Utc::now();
    EntrypointRegistration {
      registration_id: id.to_string(),
      entrypoint_instance: EntrypointInstanceIdentity {
        instance_id: id.to_string(),
        started_at_utc: now,
        shim_session_nonce: nonce.to_string(),
      },
      source_working_directory: SourcePath::new(".", None),
      source_config_path: SourcePath::new("Caddyfile", None),
      registered_domains: vec![RegisteredDomain::active("app.localhost")],
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 1,
        process_start_time_utc: now,
        shim_session_nonce: nonce.to_string(),
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint(id),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
    }
  }

  #[tokio::test]
  async fn register_and_unregister_preserve_owner_boundary() {
    let state = state();
    let response = state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;
    assert!(response.accepted);

    let wrong = state
      .unregister("wrong".to_string(), "shim-1", "other")
      .await;
    assert!(!wrong.accepted);
    assert_eq!(state.snapshot().await.registrations.len(), 1);

    let right = state
      .unregister("right".to_string(), "shim-1", "nonce-1")
      .await;
    assert!(right.accepted);
    assert!(state.snapshot().await.registrations.is_empty());
  }

  #[tokio::test]
  async fn disabled_domain_log_stream_is_reported_stale() {
    let state = state();
    let response = state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;
    assert!(response.accepted);
    let stream = LogStreamIdentity::domain("app.localhost");
    state.logs().append(
      stream.clone(),
      LogSeverity::Info,
      "domain log",
      LogAttributionKind::Domain,
      None,
    );

    let toggle = state
      .set_domain_enabled(SetDomainEnabledRequest {
        request_id: "toggle".to_string(),
        registration_id: "shim-1".to_string(),
        domain_key: "app.localhost".to_string(),
        enabled: false,
      })
      .await;
    assert!(toggle.accepted);

    let logs = state
      .query_logs(QueryLogsRequest {
        request_id: "logs".to_string(),
        stream,
        limit: Some(10),
        cursor: None,
        minimum_severity: None,
      })
      .await;

    assert_eq!(logs.stream_status, LogStreamStatus::Stale);
    assert_eq!(logs.entries.len(), 1);
  }

  #[tokio::test]
  async fn query_iis_bindings_reports_safety_states_from_fake_provider() {
    let provider = IisProvider::fake(vec![
      iis_binding("Default Web Site", "http", "*:80:app.localhost"),
      iis_binding("Default Web Site", "http", "127.0.0.1:80:app.localhost"),
      https_iis_binding("Default Web Site", "*:443:secure.localhost"),
      iis_binding("Other", "http", "*:80:missing.localhost"),
      iis_binding("Default Web Site", "https", "*:443:"),
    ]);
    let state = state_with_iis(provider);
    state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;

    let response = state.query_iis_bindings("iis".to_string()).await;

    assert!(response.accepted);
    assert_eq!(response.bindings.len(), 5);
    assert_eq!(
      response.bindings[0].handoff_state,
      IisHandoffState::Conflict
    );
    assert_eq!(
      response.bindings[2].handoff_state,
      IisHandoffState::Available
    );
    assert_eq!(
      response.bindings[3].handoff_state,
      IisHandoffState::Available
    );
    assert_eq!(
      response.bindings[4].handoff_state,
      IisHandoffState::MissingRoute
    );
  }

  #[tokio::test]
  async fn iis_handoff_proxies_wildcard_https_binding_with_route_host() {
    let temp = tempfile::tempdir().unwrap();
    let caddy = temp.path().join(if cfg!(windows) {
      "fake-caddy.cmd"
    } else {
      "fake-caddy"
    });
    write_fake_caddy(&caddy);
    let binding = https_iis_binding("Default Web Site", "*:443:");
    let provider = IisProvider::fake(vec![binding.clone()]);
    let (state, paths) = state_with_fake_caddy_paths(provider, &caddy);
    let backend_port = backend_port_for_binding(&binding);
    let expected_backend_dial = format!("127.0.0.1:{backend_port}");
    assert!((41000..49000).contains(&backend_port));

    let enabled = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-on".to_string(),
        binding_id: binding.binding_id(),
        enabled: true,
        route_host: Some("https://iis-app.localhost/legacy/status".to_string()),
      })
      .await;
    let handed_off = state.query_iis_bindings("iis-handed-off".to_string()).await;
    let rendered = fs::read_to_string(paths.effective_config_path()).unwrap();
    let config: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    let routes = config
      .pointer("/apps/http/servers/cadder_https/routes")
      .and_then(serde_json::Value::as_array)
      .unwrap();
    let iis_route = routes
      .iter()
      .find(|route| {
        route
          .pointer("/match/0/host/0")
          .and_then(serde_json::Value::as_str)
          == Some("iis-app.localhost")
      })
      .unwrap();
    let _ = state.shutdown().await;

    assert!(enabled.accepted, "{enabled:?}");
    assert_eq!(
      enabled
        .binding
        .as_ref()
        .and_then(|binding| binding.domain_key.as_deref()),
      Some("iis-app.localhost")
    );
    assert_eq!(handed_off.bindings.len(), 1);
    assert_eq!(
      handed_off.bindings[0].identity.binding_id,
      "Default Web Site|https|*:443:"
    );
    assert_eq!(
      handed_off.bindings[0].handoff_state,
      IisHandoffState::HandedOff
    );
    assert_eq!(
      handed_off.bindings[0].domain_key.as_deref(),
      Some("iis-app.localhost")
    );
    assert_eq!(
      iis_route
        .pointer("/handle/0/upstreams/0/dial")
        .and_then(serde_json::Value::as_str),
      Some(expected_backend_dial.as_str())
    );
    assert_eq!(
      iis_route
        .pointer("/handle/0/transport/tls/server_name")
        .and_then(serde_json::Value::as_str),
      Some("iis-app.localhost")
    );
    assert_eq!(
      iis_route
        .pointer("/handle/0/transport/tls/insecure_skip_verify")
        .and_then(serde_json::Value::as_bool),
      Some(true)
    );
  }

  #[tokio::test]
  async fn iis_handoff_rejects_https_binding_without_certificate_before_mutation() {
    let binding = iis_binding("Default Web Site", "https", "*:443:secure.localhost");
    let provider = IisProvider::fake(vec![binding.clone()]);
    let state = state_with_iis(provider.clone());

    let response = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-on".to_string(),
        binding_id: binding.binding_id(),
        enabled: true,
        route_host: None,
      })
      .await;
    let rediscovered = provider.discover().await.unwrap();

    assert!(!response.accepted);
    assert_eq!(
      response.issue.as_ref().map(|issue| issue.kind),
      Some(IisIssueKind::MissingTlsCertificate)
    );
    assert!(response.steps.iter().any(|step| {
      step.step_id == "iis-classify-binding" && step.status == IisOperationStepStatus::Failed
    }));
    assert_eq!(rediscovered, vec![binding]);
    assert!(state.iis_store.snapshot().await.is_empty());
  }

  #[tokio::test]
  async fn iis_handoff_rejects_explicit_route_host_used_by_another_binding() {
    let selected = iis_binding("Default Web Site", "https", "*:443:");
    let existing = iis_binding("Default Web Site", "https", "*:443:iis-app.localhost");
    let provider = IisProvider::fake(vec![selected.clone(), existing]);
    let state = state_with_iis(provider);

    let response = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-on".to_string(),
        binding_id: selected.binding_id(),
        enabled: true,
        route_host: Some("iis-app.localhost".to_string()),
      })
      .await;

    assert!(!response.accepted);
    assert_eq!(
      response.issue.as_ref().map(|issue| issue.kind),
      Some(IisIssueKind::Conflict)
    );
  }

  #[tokio::test]
  async fn runtime_paths_hydrate_iis_proxy_routes_from_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let caddy = temp.path().join(if cfg!(windows) {
      "fake-caddy.cmd"
    } else {
      "fake-caddy"
    });
    write_fake_caddy(&caddy);
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let binding = https_iis_binding("Default Web Site", "*:443:");
    let domain_key = "iis-app.localhost".to_string();
    let backend_binding = binding
      .backend_binding(backend_port_for_binding(&binding), &domain_key)
      .unwrap();
    let store = IisMetadataStore::load(paths.metadata_path()).await.unwrap();
    store
      .insert(
        binding.binding_id(),
        IisRestoreRecord {
          binding: binding.clone(),
          domain_key: domain_key.clone(),
          registration_id: None,
          backend_binding: Some(backend_binding.clone()),
        },
      )
      .await
      .unwrap();
    let resolver = RealCaddyResolver::new(Some(caddy.display().to_string()));
    let adapter = CaddyConfigAdapter::new(resolver.clone());
    let runtime = ProcessRuntime::new(resolver, paths.clone());
    let state =
      DaemonState::with_runtime_paths(CaddyConfigCoordinator::new(adapter, runtime), paths.clone())
        .await
        .unwrap();

    let rendered = fs::read_to_string(paths.effective_config_path()).unwrap();
    let _ = state.shutdown().await;

    assert!(rendered.contains("iis-app.localhost"));
    assert!(rendered.contains(&format!("127.0.0.1:{}", backend_binding.port)));
    assert!(rendered.contains("\"server_name\": \"iis-app.localhost\""));
  }

  #[tokio::test]
  async fn iis_handoff_rolls_back_when_caddy_apply_fails() {
    let binding = iis_binding("Default Web Site", "http", "*:80:iis.localhost");
    let provider = IisProvider::fake(vec![binding.clone()]);
    let state = state_with_iis(provider);
    state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;

    let response = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-on".to_string(),
        binding_id: binding.binding_id(),
        enabled: true,
        route_host: None,
      })
      .await;
    let rediscovered = state.query_iis_bindings("iis".to_string()).await;

    assert!(!response.accepted);
    assert_eq!(
      response.issue.as_ref().map(|issue| issue.kind),
      Some(IisIssueKind::RollbackSucceeded)
    );
    assert_eq!(rediscovered.bindings.len(), 1);
    assert_eq!(
      rediscovered.bindings[0].handoff_state,
      IisHandoffState::Available
    );
    assert!(state.iis_store.snapshot().await.is_empty());
  }

  #[tokio::test]
  async fn iis_handoff_keeps_restore_metadata_when_caddy_apply_and_rollback_fail() {
    let binding = iis_binding("Default Web Site", "http", "*:80:iis.localhost");
    let provider = IisProvider::fake(vec![binding.clone()]);
    provider
      .set_fail_restore(IisIssue::new(IisIssueKind::ProviderError, "restore denied"))
      .await;
    let state = state_with_iis(provider);
    state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;

    let response = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-on".to_string(),
        binding_id: binding.binding_id(),
        enabled: true,
        route_host: None,
      })
      .await;
    let rediscovered = state.query_iis_bindings("iis".to_string()).await;
    let handoffs = state.iis_store.snapshot().await;

    assert!(!response.accepted);
    assert_eq!(
      response.issue.as_ref().map(|issue| issue.kind),
      Some(IisIssueKind::RollbackFailed)
    );
    assert!(handoffs.contains_key(&binding.binding_id()));
    assert_eq!(rediscovered.bindings.len(), 1);
    assert_eq!(
      rediscovered.bindings[0].handoff_state,
      IisHandoffState::HandedOff
    );
    assert_eq!(
      rediscovered.bindings[0].domain_key.as_deref(),
      Some("iis.localhost")
    );
  }

  #[tokio::test]
  async fn iis_handoff_reports_busy_when_another_operation_is_running() {
    let binding = iis_binding("Default Web Site", "http", "*:80:iis.localhost");
    let provider = IisProvider::fake(vec![binding.clone()]);
    let state = state_with_iis(provider);
    let _operation = state.iis_operation.try_lock().unwrap();

    let response = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-on".to_string(),
        binding_id: binding.binding_id(),
        enabled: true,
        route_host: None,
      })
      .await;

    assert!(!response.accepted);
    assert_eq!(
      response.issue.as_ref().map(|issue| issue.kind),
      Some(IisIssueKind::Busy)
    );
  }

  #[tokio::test]
  async fn iis_handoff_success_and_restore_use_fake_provider() {
    let temp = tempfile::tempdir().unwrap();
    let caddy = temp.path().join(if cfg!(windows) {
      "fake-caddy.cmd"
    } else {
      "fake-caddy"
    });
    write_fake_caddy(&caddy);
    let binding = iis_binding("Default Web Site", "http", "*:80:app.localhost");
    let provider = IisProvider::fake(vec![binding.clone()]);
    let state = state_with_fake_caddy(provider, &caddy);
    state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;
    state
      .set_domain_enabled(SetDomainEnabledRequest {
        request_id: "disable".to_string(),
        registration_id: "shim-1".to_string(),
        domain_key: "app.localhost".to_string(),
        enabled: false,
      })
      .await;

    let enabled = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-on".to_string(),
        binding_id: binding.binding_id(),
        enabled: true,
        route_host: None,
      })
      .await;
    let handed_off = state.query_iis_bindings("iis-handed-off".to_string()).await;
    let restored = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-off".to_string(),
        binding_id: binding.binding_id(),
        enabled: false,
        route_host: None,
      })
      .await;
    let rediscovered = state.query_iis_bindings("iis".to_string()).await;
    let _ = state.shutdown().await;

    assert!(enabled.accepted, "{enabled:?}");
    assert_eq!(
      enabled
        .binding
        .as_ref()
        .map(|binding| binding.handoff_state),
      Some(IisHandoffState::HandedOff)
    );
    assert!(enabled.steps.iter().any(|step| {
      step.step_id == "iis-remove-public-binding"
        && step.approval == IisElevationApproval::Approved
        && step.status == IisOperationStepStatus::Succeeded
    }));
    assert_eq!(handed_off.bindings.len(), 1);
    assert_eq!(
      handed_off.bindings[0].handoff_state,
      IisHandoffState::HandedOff
    );
    assert!(restored.accepted, "{restored:?}");
    assert_eq!(rediscovered.bindings.len(), 1);
    assert_eq!(
      rediscovered.bindings[0].handoff_state,
      IisHandoffState::Available
    );
  }

  #[tokio::test]
  async fn iis_https_handoff_success_and_restore_use_https_backend_binding() {
    let temp = tempfile::tempdir().unwrap();
    let caddy = temp.path().join(if cfg!(windows) {
      "fake-caddy.cmd"
    } else {
      "fake-caddy"
    });
    write_fake_caddy(&caddy);
    let binding = https_iis_binding("Default Web Site", "*:443:secure.localhost");
    let provider = IisProvider::fake(vec![binding.clone()]);
    let state = state_with_fake_caddy(provider.clone(), &caddy);

    let enabled = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-on".to_string(),
        binding_id: binding.binding_id(),
        enabled: true,
        route_host: None,
      })
      .await;
    let after_enable = provider.discover().await.unwrap();
    let restored = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-off".to_string(),
        binding_id: binding.binding_id(),
        enabled: false,
        route_host: None,
      })
      .await;
    let after_restore = provider.discover().await.unwrap();
    let _ = state.shutdown().await;

    assert!(enabled.accepted, "{enabled:?}");
    assert_eq!(after_enable.len(), 1);
    assert_eq!(after_enable[0].protocol, "https");
    assert_eq!(after_enable[0].ip_address, "127.0.0.1");
    assert_eq!(after_enable[0].host_header, "secure.localhost");
    assert_eq!(after_enable[0].tls_certificate, Some(tls_certificate()));
    assert!(restored.accepted, "{restored:?}");
    assert_eq!(after_restore, vec![binding]);
  }

  #[tokio::test]
  async fn iis_handoff_denied_by_elevation_keeps_user_level_state_usable() {
    let binding = iis_binding("Default Web Site", "http", "*:80:app.localhost");
    let provider = IisProvider::fake(vec![binding.clone()]);
    provider
      .set_elevation_issue(IisIssue::new(
        IisIssueKind::ElevationDenied,
        "Administrator approval was denied.",
      ))
      .await;
    let state = state_with_iis(provider);

    let response = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-on".to_string(),
        binding_id: binding.binding_id(),
        enabled: true,
        route_host: None,
      })
      .await;
    let rediscovered = state.query_iis_bindings("iis".to_string()).await;

    assert!(!response.accepted);
    assert_eq!(
      response.issue.as_ref().map(|issue| issue.kind),
      Some(IisIssueKind::ElevationDenied)
    );
    assert!(response.steps.iter().any(|step| {
      step.step_id == "iis-remove-public-binding"
        && step.approval == IisElevationApproval::Denied
        && step.status == IisOperationStepStatus::Denied
    }));
    assert_eq!(
      response.follow_up_actions,
      vec![IisFollowUpAction::RetryElevation]
    );
    assert!(state.iis_store.snapshot().await.is_empty());
    assert_eq!(rediscovered.bindings.len(), 1);
    assert_eq!(
      rediscovered.bindings[0].handoff_state,
      IisHandoffState::Available
    );
  }

  #[tokio::test]
  async fn iis_handoff_reports_unsupported_elevation_without_mutating_iis() {
    let binding = https_iis_binding("Default Web Site", "*:443:secure.localhost");
    let provider = IisProvider::fake(vec![binding.clone()]);
    provider
      .set_elevation_issue(IisIssue::new(
        IisIssueKind::ElevationUnsupported,
        "IIS elevation prompts are only available on Windows.",
      ))
      .await;
    let state = state_with_iis(provider);

    let response = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-on".to_string(),
        binding_id: binding.binding_id(),
        enabled: true,
        route_host: None,
      })
      .await;
    let rediscovered = state.query_iis_bindings("iis".to_string()).await;

    assert!(!response.accepted);
    assert_eq!(
      response.issue.as_ref().map(|issue| issue.kind),
      Some(IisIssueKind::ElevationUnsupported)
    );
    assert!(response.follow_up_actions.is_empty());
    assert!(response.steps.iter().any(|step| {
      step.step_id == "iis-create-loopback-binding"
        && step.approval == IisElevationApproval::Unsupported
        && step.status == IisOperationStepStatus::Unsupported
    }));
    assert!(state.iis_store.snapshot().await.is_empty());
    assert_eq!(rediscovered.bindings.len(), 1);
    assert_eq!(
      rediscovered.bindings[0].handoff_state,
      IisHandoffState::Available
    );
  }

  #[tokio::test]
  async fn iis_restore_keeps_metadata_when_backend_cleanup_fails() {
    let temp = tempfile::tempdir().unwrap();
    let caddy = temp.path().join(if cfg!(windows) {
      "fake-caddy.cmd"
    } else {
      "fake-caddy"
    });
    write_fake_caddy(&caddy);
    let binding = iis_binding("Default Web Site", "http", "*:80:app.localhost");
    let provider = IisProvider::fake(vec![binding.clone()]);
    let state = state_with_fake_caddy(provider.clone(), &caddy);
    state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;
    state
      .set_domain_enabled(SetDomainEnabledRequest {
        request_id: "disable".to_string(),
        registration_id: "shim-1".to_string(),
        domain_key: "app.localhost".to_string(),
        enabled: false,
      })
      .await;

    let enabled = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-on".to_string(),
        binding_id: binding.binding_id(),
        enabled: true,
        route_host: None,
      })
      .await;
    provider
      .set_fail_remove(IisIssue::new(IisIssueKind::ProviderError, "cleanup failed"))
      .await;
    let restored = state
      .set_iis_handoff(SetIisHandoffRequest {
        request_id: "iis-off".to_string(),
        binding_id: binding.binding_id(),
        enabled: false,
        route_host: None,
      })
      .await;
    let handoffs = state.iis_store.snapshot().await;
    let _ = state.shutdown().await;

    assert!(enabled.accepted, "{enabled:?}");
    assert!(!restored.accepted);
    assert_eq!(
      restored.issue.as_ref().map(|issue| issue.kind),
      Some(IisIssueKind::RestoreFailed)
    );
    assert!(handoffs.contains_key(&binding.binding_id()));
  }

  #[tokio::test]
  async fn register_rejects_invalid_owner_identity() {
    let state = state();
    let mut registration = registration("shim-1", "nonce-1");
    registration.owner_process.shim_session_nonce = "different".to_string();

    let response = state.register("register".to_string(), registration).await;

    assert!(!response.accepted);
    assert!(response.message.contains("nonce values must match"));
    assert!(state.snapshot().await.registrations.is_empty());
  }

  #[tokio::test]
  async fn subscribe_receives_registration_change_event() {
    let state = state();
    let mut events = state.subscribe();

    let response = state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;
    let event = events.recv().await.unwrap();

    assert!(response.accepted);
    assert_eq!(event.sequence_number, 1);
    assert_eq!(event.change_kind, StateChangeKind::RegistrationsChanged);
    assert_eq!(event.registration_id.as_deref(), Some("shim-1"));
    assert_eq!(event.snapshot.registrations.len(), 1);
  }

  #[tokio::test]
  async fn heartbeat_accepts_owner_and_rejects_wrong_nonce() {
    let state = state();
    state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;

    let accepted = state
      .heartbeat(HeartbeatEntrypointRequest {
        request_id: "heartbeat".to_string(),
        registration_id: "shim-1".to_string(),
        shim_session_nonce: "nonce-1".to_string(),
      })
      .await;
    let rejected = state
      .heartbeat(HeartbeatEntrypointRequest {
        request_id: "heartbeat".to_string(),
        registration_id: "shim-1".to_string(),
        shim_session_nonce: "wrong".to_string(),
      })
      .await;

    assert!(accepted.accepted);
    assert_eq!(accepted.message, "Heartbeat accepted.");
    assert!(!rejected.accepted);
    assert_eq!(
      rejected.message,
      "Entrypoint was not found for the requested owner."
    );
  }

  #[tokio::test]
  async fn set_entrypoint_enabled_accepts_optional_owner_and_rejects_wrong_owner() {
    let state = state();
    state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;

    let accepted = state
      .set_entrypoint_enabled(SetEntrypointEnabledRequest {
        request_id: "disable".to_string(),
        registration_id: "shim-1".to_string(),
        shim_session_nonce: None,
        enabled: false,
      })
      .await;
    let rejected = state
      .set_entrypoint_enabled(SetEntrypointEnabledRequest {
        request_id: "enable".to_string(),
        registration_id: "shim-1".to_string(),
        shim_session_nonce: Some("wrong".to_string()),
        enabled: true,
      })
      .await;
    let snapshot = state.snapshot().await;

    assert!(accepted.accepted);
    assert_eq!(accepted.message, "Entrypoint activation updated.");
    assert!(!rejected.accepted);
    assert_eq!(
      snapshot.registrations[0].activation_state,
      ActivationState::Inactive
    );
  }

  #[tokio::test]
  async fn set_domain_enabled_rejects_unknown_domain() {
    let state = state();
    state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;

    let response = state
      .set_domain_enabled(SetDomainEnabledRequest {
        request_id: "toggle".to_string(),
        registration_id: "shim-1".to_string(),
        domain_key: "missing.localhost".to_string(),
        enabled: false,
      })
      .await;

    assert!(!response.accepted);
    assert_eq!(response.message, "Domain was not found.");
  }

  #[tokio::test]
  async fn query_state_returns_snapshot_with_request_id() {
    let state = state();
    state
      .register("register".to_string(), registration("shim-1", "nonce-1"))
      .await;

    let response = state.query_state("state".to_string()).await;

    assert!(response.accepted);
    assert_eq!(response.request_id, "state");
    assert_eq!(response.message, "State snapshot returned.");
    assert_eq!(response.snapshot.unwrap().registrations.len(), 1);
  }

  #[tokio::test]
  async fn runtime_control_logs_are_active_and_limit_is_clamped() {
    let state = state();
    let stream = LogStreamIdentity::runtime_control();
    state.logs().append(
      stream.clone(),
      LogSeverity::Info,
      "first",
      LogAttributionKind::RuntimeControl,
      Some("start".to_string()),
    );
    state.logs().append(
      stream.clone(),
      LogSeverity::Warn,
      "second",
      LogAttributionKind::RuntimeControl,
      Some("reload".to_string()),
    );

    let response = state
      .query_logs(QueryLogsRequest {
        request_id: "logs".to_string(),
        stream: stream.clone(),
        limit: Some(0),
        cursor: Some("not-a-sequence".to_string()),
        minimum_severity: Some(LogSeverity::Info),
      })
      .await;

    assert_eq!(response.stream, stream);
    assert_eq!(response.stream_status, LogStreamStatus::Active);
    assert_eq!(response.entries.len(), 1);
    assert_eq!(response.entries[0].raw_message, "second");
    assert!(response.next_cursor.is_some());
  }

  #[tokio::test]
  async fn shutdown_returns_success_when_runtime_is_idle() {
    let state = state();

    let response = state.shutdown().await;

    assert!(response.accepted);
    assert_eq!(response.message, "Daemon shutdown requested.");
  }
}
