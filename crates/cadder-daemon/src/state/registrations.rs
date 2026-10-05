use super::*;

impl DaemonState {
  pub async fn register(&self, registration: EntrypointRegistration) -> RegisterEntrypointResponse {
    let fence = match self.issue_operation_fence() {
      Ok(fence) => fence,
      Err(error) => {
        return RegisterEntrypointResponse {
          request_id: String::new(),
          accepted: false,
          message: mutation_admission_message(error),
          registration_id: None,
        };
      }
    };
    self
      .register_fenced(registration, &fence)
      .await
      .unwrap_or_else(|error| RegisterEntrypointResponse {
        request_id: String::new(),
        accepted: false,
        message: mutation_admission_message(error),
        registration_id: None,
      })
  }

  pub(crate) async fn register_fenced(
    &self,
    registration: EntrypointRegistration,
    fence: &OperationFence,
  ) -> Result<RegisterEntrypointResponse, CommitRejection> {
    if let Err(message) = registration.validate_owner() {
      return Ok(RegisterEntrypointResponse {
        request_id: String::new(),
        accepted: false,
        message,
        registration_id: None,
      });
    }

    let _operation = self
      .config_operation
      .acquire()
      .await
      .expect("config operation semaphore closed");
    {
      let inner = self.inner.lock().await;
      if registration_has_different_owner(&inner, &registration) {
        return Ok(registration_owner_conflict());
      }
    }

    let source_path = registration.source_config_path.raw.clone();
    let adapter = {
      let coordinator = self.coordinator.lock().await;
      coordinator.adapter()
    };
    let prepared = adapter.prepare(registration).await;
    let (mut candidate_coordinator, mut candidate_registrations) = {
      let coordinator = self.coordinator.lock().await;
      let inner = self.inner.lock().await;
      if registration_has_different_owner(&inner, &prepared.registration) {
        return Ok(registration_owner_conflict());
      }
      (coordinator.clone(), inner.registrations.clone())
    };

    let mut prepared = candidate_coordinator.commit_prepared_registration(source_path, prepared);
    if let Some(database) = &self.database
      && let Err(error) = database.restore_desired_state(&mut prepared).await
    {
      return Ok(registration_storage_rejected(error));
    }
    let now = Utc::now();
    prepared.created_at_utc = now;
    prepared.last_heartbeat_utc = now;
    let id = prepared.registration_id.clone();
    candidate_registrations.insert(id.clone(), prepared);
    if let Some(conflict) = active_domain_ownership_conflict(&candidate_registrations, &id) {
      return Ok(registration_domain_conflict(conflict));
    }
    let transaction = RegistrationTransaction {
      coordinator: candidate_coordinator,
      registrations: candidate_registrations,
    };
    match self
      .execute_registration_transaction(transaction, fence)
      .await
    {
      Ok(()) => {}
      Err(RegistrationTransactionFailure::Fenced(rejection)) => return Err(rejection),
      Err(RegistrationTransactionFailure::Runtime(error)) => {
        return Ok(registration_runtime_rejected(error));
      }
      Err(RegistrationTransactionFailure::Storage(error)) => {
        return Ok(registration_storage_rejected(error));
      }
    }

    Ok(RegisterEntrypointResponse {
      request_id: String::new(),
      accepted: true,
      message: "Entrypoint registered.".to_string(),
      registration_id: Some(id),
    })
  }

  async fn execute_registration_transaction(
    &self,
    mut transaction: RegistrationTransaction,
    fence: &OperationFence,
  ) -> Result<(), RegistrationTransactionFailure> {
    let mut runtime_receipt = self
      .apply_runtime_transition(&mut transaction, fence)
      .await?;
    let publish_result: Result<(), RegistrationTransactionFailure> = async {
      let mut coordinator = self.coordinator.lock().await;
      let mut inner = self.inner.lock().await;
      merge_live_heartbeats(&mut transaction.registrations, &inner.registrations);
      let final_commit = fence
        .begin_final_commit()
        .map_err(RegistrationTransactionFailure::Fenced)?;
      if let Some(receipt) = runtime_receipt.as_mut()
        && let Err(error) = receipt.accept(&self.logs).await
      {
        return Err(RegistrationTransactionFailure::Runtime(error));
      }
      if let Some(database) = &self.database {
        database
          .persist_desired_state(transaction.registrations.values().cloned().collect())
          .await
          .map_err(RegistrationTransactionFailure::Storage)?;
      }
      final_commit.commit(|| {
        *coordinator = transaction.coordinator;
        inner.registrations = transaction.registrations;
      });
      Ok(())
    }
    .await;

    match publish_result {
      Ok(()) => Ok(()),
      Err(RegistrationTransactionFailure::Fenced(rejection)) => {
        rollback_runtime_transition(self, runtime_receipt).await;
        Err(RegistrationTransactionFailure::Fenced(rejection))
      }
      Err(RegistrationTransactionFailure::Runtime(error)) => {
        rollback_runtime_transition(self, runtime_receipt).await;
        Err(RegistrationTransactionFailure::Runtime(error))
      }
      Err(RegistrationTransactionFailure::Storage(error)) => {
        rollback_runtime_transition(self, runtime_receipt).await;
        Err(RegistrationTransactionFailure::Storage(error))
      }
    }
  }

  async fn apply_runtime_transition(
    &self,
    transaction: &mut RegistrationTransaction,
    fence: &OperationFence,
  ) -> Result<Option<RuntimeTransitionReceipt>, RegistrationTransactionFailure> {
    let registrations = transaction
      .registrations
      .values()
      .cloned()
      .collect::<Vec<_>>();
    let runtime = transaction.coordinator.runtime();
    match transaction.coordinator.begin_apply(&registrations) {
      CaddyApplyAction::Current(_) => Ok(None),
      CaddyApplyAction::Stop { attempted } => {
        fence
          .commit(|| ())
          .map_err(RegistrationTransactionFailure::Fenced)?;
        let attempt = runtime
          .begin_stop(&self.logs)
          .await
          .map_err(RegistrationTransactionFailure::Runtime)?;
        let (receipt, outcome) = attempt.into_parts();
        if let Err(error) = outcome {
          log_runtime_transition_failure(&self.logs, &error).await;
          rollback_runtime_transition(self, Some(RuntimeTransitionReceipt::Stop(receipt))).await;
          return Err(RegistrationTransactionFailure::Runtime(error));
        }
        transaction.coordinator.finish_idle(attempted);
        Ok(Some(RuntimeTransitionReceipt::Stop(receipt)))
      }
      CaddyApplyAction::Apply {
        attempted,
        rendered,
        hash,
        source_config_paths,
      } => {
        fence
          .commit(|| ())
          .map_err(RegistrationTransactionFailure::Fenced)?;
        let attempt = runtime
          .begin_apply_config(&rendered, &self.logs)
          .await
          .map_err(RegistrationTransactionFailure::Runtime)?;
        let (receipt, outcome) = attempt.into_parts();
        if let Err(error) = outcome {
          log_runtime_transition_failure(&self.logs, &error).await;
          rollback_runtime_transition(self, Some(RuntimeTransitionReceipt::Apply(receipt))).await;
          return Err(RegistrationTransactionFailure::Runtime(error));
        }
        transaction
          .coordinator
          .finish_runtime_apply(attempted, hash, source_config_paths, Ok(()));
        Ok(Some(RuntimeTransitionReceipt::Apply(receipt)))
      }
    }
  }

  pub async fn unregister(&self, registration_id: &str, shim_session_nonce: &str) -> BasicResponse {
    let fence = match self.issue_operation_fence() {
      Ok(fence) => fence,
      Err(error) => return mutation_rejected(error),
    };
    self
      .unregister_fenced(registration_id, shim_session_nonce, &fence)
      .await
      .unwrap_or_else(mutation_rejected)
  }

  pub(crate) async fn unregister_fenced(
    &self,
    registration_id: &str,
    shim_session_nonce: &str,
    fence: &OperationFence,
  ) -> Result<BasicResponse, CommitRejection> {
    let _operation = self
      .config_operation
      .acquire()
      .await
      .expect("config operation semaphore closed");
    fence.commit(|| ())?;
    let (candidate_coordinator, mut candidate_registrations) = {
      let coordinator = self.coordinator.lock().await;
      let inner = self.inner.lock().await;
      (coordinator.clone(), inner.registrations.clone())
    };
    let owned_registration = candidate_registrations
      .get(registration_id)
      .filter(|registration| {
        registration.entrypoint_instance.shim_session_nonce == shim_session_nonce
      })
      .cloned();
    let Some(_owned_registration) = owned_registration else {
      return Ok(BasicResponse {
        request_id: String::new(),
        accepted: false,
        message: "Entrypoint was not found for the requested owner.".to_string(),
      });
    };
    candidate_registrations.remove(registration_id);
    let transaction = RegistrationTransaction {
      coordinator: candidate_coordinator,
      registrations: candidate_registrations,
    };
    match self
      .execute_registration_transaction(transaction, fence)
      .await
    {
      Ok(()) => {}
      Err(RegistrationTransactionFailure::Fenced(rejection)) => return Err(rejection),
      Err(RegistrationTransactionFailure::Runtime(error)) => {
        return Ok(registration_mutation_runtime_rejected(
          "unregister entrypoint",
          error,
        ));
      }
      Err(RegistrationTransactionFailure::Storage(error)) => {
        return Ok(registration_mutation_storage_rejected(
          "unregister entrypoint",
          error,
        ));
      }
    }

    Ok(BasicResponse {
      request_id: String::new(),
      accepted: true,
      message: "Entrypoint unregistered.".to_string(),
    })
  }

  pub(crate) async fn unregister_for_ipc_disconnect(
    &self,
    registration_id: &str,
    shim_session_nonce: &str,
  ) {
    let Ok(fence) = self.issue_operation_fence() else {
      return;
    };
    let response = self
      .unregister_fenced(registration_id, shim_session_nonce, &fence)
      .await
      .ok();
    if !response.as_ref().is_some_and(|response| response.accepted) {
      self
        .logs
        .append(
          LogStreamIdentity::runtime_control(),
          LogSeverity::Warn,
          response
            .map(|response| response.message)
            .unwrap_or_else(|| "Entrypoint cleanup was cancelled during daemon drain.".to_string()),
          LogAttributionKind::RuntimeControl,
          Some("ipc-disconnect-cleanup".to_string()),
        )
        .await;
    }
  }

  pub async fn heartbeat(&self, request: HeartbeatEntrypointPayload) -> BasicResponse {
    let fence = match self.issue_operation_fence() {
      Ok(fence) => fence,
      Err(error) => return mutation_rejected(error),
    };
    self
      .heartbeat_fenced(request, &fence)
      .await
      .unwrap_or_else(mutation_rejected)
  }

  pub(crate) async fn heartbeat_fenced(
    &self,
    request: HeartbeatEntrypointPayload,
    fence: &OperationFence,
  ) -> Result<BasicResponse, CommitRejection> {
    let accepted = {
      let mut inner = self.inner.lock().await;
      fence.commit_final(|| {
        inner
          .registrations
          .get_mut(&request.registration_id)
          .filter(|registration| {
            registration.entrypoint_instance.shim_session_nonce == request.shim_session_nonce
          })
          .map(|registration| {
            registration.last_heartbeat_utc = Utc::now();
          })
          .is_some()
      })?
    };

    Ok(BasicResponse {
      request_id: String::new(),
      accepted,
      message: if accepted {
        "Heartbeat accepted."
      } else {
        "Entrypoint was not found for the requested owner."
      }
      .to_string(),
    })
  }

  pub async fn set_entrypoint_enabled(
    &self,
    request: SetEntrypointEnabledPayload,
  ) -> BasicResponse {
    let fence = match self.issue_operation_fence() {
      Ok(fence) => fence,
      Err(error) => return mutation_rejected(error),
    };
    self
      .set_entrypoint_enabled_fenced(request, &fence)
      .await
      .unwrap_or_else(mutation_rejected)
  }

  pub(crate) async fn set_entrypoint_enabled_fenced(
    &self,
    request: SetEntrypointEnabledPayload,
    fence: &OperationFence,
  ) -> Result<BasicResponse, CommitRejection> {
    let _operation = self
      .config_operation
      .acquire()
      .await
      .expect("config operation semaphore closed");
    fence.commit(|| ())?;
    let (candidate_coordinator, mut candidate_registrations) = {
      let coordinator = self.coordinator.lock().await;
      let inner = self.inner.lock().await;
      (coordinator.clone(), inner.registrations.clone())
    };
    let accepted = candidate_registrations
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
    if !accepted {
      return Ok(BasicResponse {
        request_id: String::new(),
        accepted: false,
        message: "Entrypoint was not found.".to_string(),
      });
    }
    if request.enabled
      && let Some(conflict) =
        active_domain_ownership_conflict(&candidate_registrations, &request.registration_id)
    {
      return Ok(registration_mutation_domain_conflict(conflict));
    }
    let transaction = RegistrationTransaction {
      coordinator: candidate_coordinator,
      registrations: candidate_registrations,
    };
    match self
      .execute_registration_transaction(transaction, fence)
      .await
    {
      Ok(()) => {}
      Err(RegistrationTransactionFailure::Fenced(rejection)) => return Err(rejection),
      Err(RegistrationTransactionFailure::Runtime(error)) => {
        return Ok(registration_mutation_runtime_rejected(
          "update entrypoint activation",
          error,
        ));
      }
      Err(RegistrationTransactionFailure::Storage(error)) => {
        return Ok(registration_mutation_storage_rejected(
          "change entrypoint activation",
          error,
        ));
      }
    }

    Ok(BasicResponse {
      request_id: String::new(),
      accepted: true,
      message: "Entrypoint activation updated.".to_string(),
    })
  }

  pub async fn set_domain_enabled(&self, request: SetDomainEnabledPayload) -> BasicResponse {
    let fence = match self.issue_operation_fence() {
      Ok(fence) => fence,
      Err(error) => return mutation_rejected(error),
    };
    self
      .set_domain_enabled_fenced(request, &fence)
      .await
      .unwrap_or_else(mutation_rejected)
  }

  pub(crate) async fn set_domain_enabled_fenced(
    &self,
    request: SetDomainEnabledPayload,
    fence: &OperationFence,
  ) -> Result<BasicResponse, CommitRejection> {
    let _operation = self
      .config_operation
      .acquire()
      .await
      .expect("config operation semaphore closed");
    fence.commit(|| ())?;
    let (candidate_coordinator, mut candidate_registrations) = {
      let coordinator = self.coordinator.lock().await;
      let inner = self.inner.lock().await;
      (coordinator.clone(), inner.registrations.clone())
    };
    let accepted = candidate_registrations
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
    if !accepted {
      return Ok(BasicResponse {
        request_id: String::new(),
        accepted: false,
        message: "Domain was not found.".to_string(),
      });
    }
    if request.enabled
      && let Some(conflict) =
        active_domain_ownership_conflict(&candidate_registrations, &request.registration_id)
    {
      return Ok(registration_mutation_domain_conflict(conflict));
    }
    let transaction = RegistrationTransaction {
      coordinator: candidate_coordinator,
      registrations: candidate_registrations,
    };
    match self
      .execute_registration_transaction(transaction, fence)
      .await
    {
      Ok(()) => {}
      Err(RegistrationTransactionFailure::Fenced(rejection)) => return Err(rejection),
      Err(RegistrationTransactionFailure::Runtime(error)) => {
        return Ok(registration_mutation_runtime_rejected(
          "update domain activation",
          error,
        ));
      }
      Err(RegistrationTransactionFailure::Storage(error)) => {
        return Ok(registration_mutation_storage_rejected(
          "change domain activation",
          error,
        ));
      }
    }

    Ok(BasicResponse {
      request_id: String::new(),
      accepted: true,
      message: "Domain activation updated.".to_string(),
    })
  }
}

fn merge_live_heartbeats(
  candidate: &mut BTreeMap<String, EntrypointRegistration>,
  live: &BTreeMap<String, EntrypointRegistration>,
) {
  for (registration_id, candidate_registration) in candidate {
    let Some(live_registration) = live.get(registration_id) else {
      continue;
    };
    if candidate_registration
      .entrypoint_instance
      .shim_session_nonce
      == live_registration.entrypoint_instance.shim_session_nonce
    {
      candidate_registration.last_heartbeat_utc = candidate_registration
        .last_heartbeat_utc
        .max(live_registration.last_heartbeat_utc);
    }
  }
}

struct RegistrationTransaction {
  coordinator: CaddyConfigCoordinator,
  registrations: BTreeMap<String, EntrypointRegistration>,
}

enum RegistrationTransactionFailure {
  Fenced(CommitRejection),
  Runtime(anyhow::Error),
  Storage(anyhow::Error),
}

#[derive(Debug)]
struct DomainOwnershipConflict {
  domain: String,
  owner_registration_id: String,
  conflicting_registration_id: String,
}

impl std::fmt::Display for DomainOwnershipConflict {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      formatter,
      "Domain `{}` is already owned by entrypoint `{}`; entrypoint `{}` was not changed. Stop or disable the existing entrypoint before retrying",
      self.domain, self.owner_registration_id, self.conflicting_registration_id
    )
  }
}

enum RuntimeTransitionReceipt {
  Apply(crate::runtime::RuntimeApplyReceipt),
  Stop(crate::runtime::RuntimeStopReceipt),
}

impl RuntimeTransitionReceipt {
  async fn accept(&mut self, logs: &CaddyLogStore) -> Result<()> {
    match self {
      Self::Apply(receipt) => receipt.accept(logs).await,
      Self::Stop(receipt) => receipt.accept(),
    }
  }

  async fn rollback(self, logs: &CaddyLogStore) -> Result<()> {
    match self {
      Self::Apply(receipt) => receipt.rollback(logs).await,
      Self::Stop(receipt) => receipt.rollback(logs).await,
    }
  }
}

async fn rollback_runtime_transition(
  state: &DaemonState,
  receipt: Option<RuntimeTransitionReceipt>,
) {
  let Some(receipt) = receipt else {
    return;
  };
  if let Err(error) = receipt.rollback(&state.logs).await {
    state.begin_operation_drain();
    state.request_shutdown();
    state.logs.append(
      LogStreamIdentity::runtime_control(),
      LogSeverity::Error,
      format!(
        "runtime transition rollback failed after rejected registration; Cadder requested shutdown: {error:#}"
      ),
      LogAttributionKind::RuntimeControl,
      Some("registration-rollback".to_string()),
    ).await;
  }
}

async fn log_runtime_transition_failure(logs: &CaddyLogStore, error: &anyhow::Error) {
  logs
    .append(
      LogStreamIdentity::runtime_control(),
      LogSeverity::Error,
      format!("Caddy runtime transition failed; Cadder is restoring the previous state: {error:#}"),
      LogAttributionKind::RuntimeControl,
      Some("registration-runtime-transition".to_string()),
    )
    .await;
}

fn registration_runtime_rejected(error: impl std::fmt::Display) -> RegisterEntrypointResponse {
  RegisterEntrypointResponse {
    request_id: String::new(),
    accepted: false,
    message: format!("Entrypoint registration could not update Caddy: {error}."),
    registration_id: None,
  }
}

fn registration_storage_rejected(error: impl std::fmt::Display) -> RegisterEntrypointResponse {
  RegisterEntrypointResponse {
    request_id: String::new(),
    accepted: false,
    message: format!(
      "Entrypoint registration was not committed because durable storage failed: {error}. Cadder kept the previous state. Resolve the reported storage error, then retry."
    ),
    registration_id: None,
  }
}

fn registration_mutation_runtime_rejected(
  operation: &str,
  error: impl std::fmt::Display,
) -> BasicResponse {
  BasicResponse {
    request_id: String::new(),
    accepted: false,
    message: format!("Could not {operation} because Caddy rejected the runtime update: {error}."),
  }
}

fn registration_mutation_storage_rejected(
  operation: &str,
  error: impl std::fmt::Display,
) -> BasicResponse {
  BasicResponse {
    request_id: String::new(),
    accepted: false,
    message: format!(
      "Cadder could not {operation} because durable storage failed: {error}. Cadder restored the previous runtime state. Resolve the reported storage error, then retry."
    ),
  }
}

fn registration_has_different_owner(
  inner: &DaemonInner,
  registration: &EntrypointRegistration,
) -> bool {
  inner
    .registrations
    .get(&registration.registration_id)
    .is_some_and(|existing| {
      existing.entrypoint_instance.shim_session_nonce
        != registration.entrypoint_instance.shim_session_nonce
    })
}

fn active_domain_ownership_conflict(
  registrations: &BTreeMap<String, EntrypointRegistration>,
  candidate_registration_id: &str,
) -> Option<DomainOwnershipConflict> {
  let candidate = registrations
    .get(candidate_registration_id)
    .filter(|registration| registration.activation_state.is_enabled())?;
  let owners = registrations
    .values()
    .filter(|registration| registration.registration_id != candidate_registration_id)
    .filter(|registration| registration.activation_state.is_enabled())
    .flat_map(|registration| {
      registration
        .registered_domains
        .iter()
        .filter(|domain| domain.activation_state.is_enabled())
        .map(|domain| {
          (
            domain.name.canonical.as_str(),
            registration.registration_id.as_str(),
          )
        })
    })
    .collect::<BTreeMap<_, _>>();

  for domain in candidate
    .registered_domains
    .iter()
    .filter(|domain| domain.activation_state.is_enabled())
  {
    if let Some(owner_registration_id) = owners.get(domain.name.canonical.as_str()) {
      return Some(DomainOwnershipConflict {
        domain: domain.name.canonical.clone(),
        owner_registration_id: (*owner_registration_id).to_string(),
        conflicting_registration_id: candidate.registration_id.clone(),
      });
    }
  }

  None
}

fn registration_owner_conflict() -> RegisterEntrypointResponse {
  RegisterEntrypointResponse {
    request_id: String::new(),
    accepted: false,
    message: "Entrypoint registration ID is already owned by another shim session.".to_string(),
    registration_id: None,
  }
}

fn registration_domain_conflict(conflict: DomainOwnershipConflict) -> RegisterEntrypointResponse {
  RegisterEntrypointResponse {
    request_id: String::new(),
    accepted: false,
    message: format!("{conflict}."),
    registration_id: None,
  }
}

fn registration_mutation_domain_conflict(conflict: DomainOwnershipConflict) -> BasicResponse {
  BasicResponse {
    request_id: String::new(),
    accepted: false,
    message: format!("{conflict}."),
  }
}

fn mutation_rejected(error: CommitRejection) -> BasicResponse {
  BasicResponse {
    request_id: String::new(),
    accepted: false,
    message: mutation_admission_message(error),
  }
}

fn mutation_admission_message(error: CommitRejection) -> String {
  format!("Daemon mutation rejected: {error}.")
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_ipc::{
    DomainName, EntrypointInstanceIdentity, OwnerProcessIdentity, RegisteredDomain, SourcePath,
  };
  use chrono::Duration;

  fn registration(
    id: &str,
    nonce: &str,
    domain: &str,
    entrypoint_enabled: bool,
    domain_enabled: bool,
  ) -> EntrypointRegistration {
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
      registered_domains: vec![RegisteredDomain {
        name: DomainName::parse(domain),
        activation_state: ActivationState::from_enabled(domain_enabled),
        upstream: None,
        log_stream: LogStreamIdentity::domain(domain),
      }],
      activation_state: ActivationState::from_enabled(entrypoint_enabled),
      owner_process: OwnerProcessIdentity {
        process_id: 42,
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

  #[test]
  fn heartbeat_merge_updates_only_the_same_live_registration_owner() {
    let mut candidate = BTreeMap::from([
      (
        "entry-1".to_string(),
        registration("entry-1", "nonce-1", "one.localhost", true, true),
      ),
      (
        "entry-2".to_string(),
        registration("entry-2", "old", "two.localhost", true, true),
      ),
      (
        "entry-3".to_string(),
        registration("entry-3", "nonce-3", "three.localhost", true, true),
      ),
    ]);
    let original_mismatch = candidate["entry-2"].last_heartbeat_utc;
    let original_missing = candidate["entry-3"].last_heartbeat_utc;
    let mut matching = registration("entry-1", "nonce-1", "one.localhost", true, true);
    matching.last_heartbeat_utc += Duration::seconds(30);
    let mut mismatch = registration("entry-2", "new", "two.localhost", true, true);
    mismatch.last_heartbeat_utc += Duration::seconds(30);
    let live = BTreeMap::from([
      ("entry-1".to_string(), matching.clone()),
      ("entry-2".to_string(), mismatch),
    ]);

    merge_live_heartbeats(&mut candidate, &live);

    assert_eq!(
      candidate["entry-1"].last_heartbeat_utc,
      matching.last_heartbeat_utc
    );
    assert_eq!(candidate["entry-2"].last_heartbeat_utc, original_mismatch);
    assert_eq!(candidate["entry-3"].last_heartbeat_utc, original_missing);
  }

  #[test]
  fn domain_conflicts_require_two_enabled_owners() {
    let active = registration("entry-1", "nonce-1", "app.localhost", true, true);
    let disabled_entrypoint = registration("entry-2", "nonce-2", "app.localhost", false, true);
    let disabled_domain = registration("entry-3", "nonce-3", "app.localhost", true, false);
    let mut registrations = BTreeMap::from([
      ("entry-1".to_string(), active),
      ("entry-2".to_string(), disabled_entrypoint),
      ("entry-3".to_string(), disabled_domain),
    ]);
    assert!(active_domain_ownership_conflict(&registrations, "missing").is_none());
    assert!(active_domain_ownership_conflict(&registrations, "entry-2").is_none());
    assert!(active_domain_ownership_conflict(&registrations, "entry-3").is_none());

    registrations.get_mut("entry-2").unwrap().activation_state = ActivationState::Active;
    let conflict = active_domain_ownership_conflict(&registrations, "entry-2").unwrap();
    assert_eq!(conflict.domain, "app.localhost");
    assert_eq!(conflict.owner_registration_id, "entry-1");
    assert_eq!(conflict.conflicting_registration_id, "entry-2");
    assert!(conflict.to_string().contains("Stop or disable"));
  }

  #[test]
  fn registration_rejection_helpers_keep_specific_recovery_context() {
    let runtime = registration_runtime_rejected("reload failed");
    assert!(!runtime.accepted);
    assert!(runtime.message.contains("reload failed"));
    let storage = registration_storage_rejected("database busy");
    assert!(!storage.accepted);
    assert!(storage.message.contains("previous state"));
    let mutation_runtime =
      registration_mutation_runtime_rejected("disable domain", "reload failed");
    assert!(mutation_runtime.message.contains("disable domain"));
    let mutation_storage =
      registration_mutation_storage_rejected("disable domain", "database busy");
    assert!(
      mutation_storage
        .message
        .contains("restored the previous runtime state")
    );

    let owner = registration_owner_conflict();
    assert!(!owner.accepted);
    assert!(owner.message.contains("another shim session"));
    let conflict = DomainOwnershipConflict {
      domain: "app.localhost".to_string(),
      owner_registration_id: "entry-1".to_string(),
      conflicting_registration_id: "entry-2".to_string(),
    };
    assert!(!registration_domain_conflict(conflict).accepted);
    let conflict = DomainOwnershipConflict {
      domain: "app.localhost".to_string(),
      owner_registration_id: "entry-1".to_string(),
      conflicting_registration_id: "entry-2".to_string(),
    };
    assert!(!registration_mutation_domain_conflict(conflict).accepted);
  }

  #[tokio::test]
  async fn failed_runtime_rollback_requests_daemon_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let coordinator = CaddyConfigCoordinator::new_mock(paths.clone());
    let runtime = coordinator.runtime();
    let state = DaemonState::new(coordinator);
    let attempt = runtime
      .begin_apply_config(br#"{"apps":{}}"#, &state.logs)
      .await
      .unwrap();
    let (receipt, outcome) = attempt.into_parts();
    outcome.unwrap();
    std::fs::create_dir(paths.effective_config_path()).unwrap();

    rollback_runtime_transition(&state, Some(RuntimeTransitionReceipt::Apply(receipt))).await;

    assert!(state.shutdown_signal().started_at().is_some());
  }

  #[test]
  fn owner_comparison_distinguishes_matching_missing_and_replaced_sessions() {
    let existing = registration("entry-1", "nonce-1", "app.localhost", true, true);
    let inner = DaemonInner {
      registrations: BTreeMap::from([("entry-1".to_string(), existing.clone())]),
    };
    assert!(!registration_has_different_owner(&inner, &existing));
    assert!(!registration_has_different_owner(
      &inner,
      &registration("entry-2", "nonce-2", "other.localhost", true, true)
    ));
    assert!(registration_has_different_owner(
      &inner,
      &registration("entry-1", "replacement", "app.localhost", true, true)
    ));
  }
}
