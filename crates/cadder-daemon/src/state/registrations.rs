use super::*;

impl DaemonState {
  pub async fn register(
    &self,
    request_id: String,
    registration: EntrypointRegistration,
  ) -> RegisterEntrypointResponse {
    let fallback_request_id = request_id.clone();
    let fence = match self.issue_operation_fence() {
      Ok(fence) => fence,
      Err(error) => {
        return RegisterEntrypointResponse {
          request_id,
          accepted: false,
          message: mutation_admission_message(error),
          registration_id: None,
        };
      }
    };
    self
      .register_fenced(request_id, registration, &fence)
      .await
      .unwrap_or_else(|error| RegisterEntrypointResponse {
        request_id: fallback_request_id,
        accepted: false,
        message: mutation_admission_message(error),
        registration_id: None,
      })
  }

  pub(crate) async fn register_fenced(
    &self,
    request_id: String,
    registration: EntrypointRegistration,
    fence: &OperationFence,
  ) -> Result<RegisterEntrypointResponse, CommitRejection> {
    if let Err(message) = registration.validate_owner() {
      return Ok(RegisterEntrypointResponse {
        request_id,
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
        return Ok(registration_owner_conflict(request_id));
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
        return Ok(registration_owner_conflict(request_id));
      }
      (coordinator.clone(), inner.registrations.clone())
    };

    let mut prepared = candidate_coordinator.commit_prepared_registration(source_path, prepared);
    let now = Utc::now();
    prepared.created_at_utc = now;
    prepared.last_heartbeat_utc = now;
    let id = prepared.registration_id.clone();
    let domain_count = prepared.registered_domains.len();
    candidate_registrations.insert(id.clone(), prepared);
    let transaction = RegistrationTransaction {
      coordinator: candidate_coordinator,
      registrations: candidate_registrations,
      history: RegistrationHistoryDraft {
        summary: format!("Registered entrypoint `{id}`."),
        registration_id: Some(id.clone()),
        domain_key: None,
        details: serde_json::json!({
          "registrationId": id,
          "domainCount": domain_count
        }),
      },
      event_registration_id: Some(id.clone()),
    };
    match self
      .execute_registration_transaction(transaction, fence)
      .await
    {
      Ok(()) => {}
      Err(RegistrationTransactionFailure::Fenced(rejection)) => return Err(rejection),
      Err(RegistrationTransactionFailure::Runtime(error)) => {
        return Ok(registration_runtime_rejected(request_id, error));
      }
      Err(RegistrationTransactionFailure::Storage(error)) => {
        return Ok(registration_storage_rejected(request_id, error));
      }
    }

    Ok(RegisterEntrypointResponse {
      request_id,
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
    let runtime = transaction.coordinator.runtime();
    let runtime_state = match &runtime_receipt {
      Some(receipt) => receipt.projected_state().await,
      None => runtime.inspect().await,
    };
    #[cfg(test)]
    if let Some(hook) = &self.registration_publish_hook {
      hook.pause().await;
    }
    let storage_state = self.store.state();
    let publish_result: Result<(), RegistrationTransactionFailure> = async {
      let _publish = self.publish_operation.lock().await;
      let mut coordinator = self.coordinator.lock().await;
      let mut inner = self.inner.lock().await;
      merge_live_heartbeats(&mut transaction.registrations, &inner.registrations);
      let snapshot = GuiStateSnapshot {
        captured_at_utc: Utc::now(),
        registrations: transaction.registrations.values().cloned().collect(),
        runtime: runtime_state,
        config: transaction.coordinator.current_state(),
        storage: Some(storage_state),
      };
      let final_commit = fence
        .begin_final_commit()
        .map_err(RegistrationTransactionFailure::Fenced)?;
      if let Some(receipt) = runtime_receipt.as_mut()
        && let Err(error) = receipt.accept(&self.logs)
      {
        return Err(RegistrationTransactionFailure::Runtime(error));
      }
      self
        .store
        .commit_history(
          HistoryKind::Registration,
          transaction.history.summary.clone(),
          transaction.history.registration_id.as_deref(),
          transaction.history.domain_key.as_deref(),
          &transaction.history.details,
        )
        .await
        .map_err(RegistrationTransactionFailure::Storage)?;
      final_commit.commit(|| {
        *coordinator = transaction.coordinator;
        inner.registrations = transaction.registrations;
        inner.sequence += 1;
        let _ = self.events.send(StateChangedEvent {
          request_id: "state-change".to_string(),
          sequence_number: inner.sequence,
          change_kind: StateChangeKind::RegistrationsChanged,
          snapshot,
          registration_id: transaction.event_registration_id,
        });
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
          log_runtime_transition_failure(&self.logs, &error);
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
          log_runtime_transition_failure(&self.logs, &error);
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

  pub async fn unregister(
    &self,
    request_id: String,
    registration_id: &str,
    shim_session_nonce: &str,
  ) -> BasicResponse {
    let fallback_request_id = request_id.clone();
    let fence = match self.issue_operation_fence() {
      Ok(fence) => fence,
      Err(error) => return mutation_rejected(fallback_request_id, error),
    };
    self
      .unregister_fenced(request_id, registration_id, shim_session_nonce, &fence)
      .await
      .unwrap_or_else(|error| mutation_rejected(fallback_request_id, error))
  }

  pub(crate) async fn unregister_fenced(
    &self,
    request_id: String,
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
    let removed_registration = candidate_registrations
      .get(registration_id)
      .filter(|registration| {
        registration.entrypoint_instance.shim_session_nonce == shim_session_nonce
      })
      .cloned();
    let Some(removed_registration) = removed_registration else {
      return Ok(BasicResponse {
        request_id,
        accepted: false,
        message: "Entrypoint was not found for the requested owner.".to_string(),
      });
    };
    candidate_registrations.remove(registration_id);
    let transaction = RegistrationTransaction {
      coordinator: candidate_coordinator,
      registrations: candidate_registrations,
      history: RegistrationHistoryDraft {
        summary: format!("Unregistered entrypoint `{registration_id}`."),
        registration_id: Some(registration_id.to_string()),
        domain_key: None,
        details: serde_json::json!({
          "registrationId": registration_id,
          "domainCount": removed_registration.registered_domains.len()
        }),
      },
      event_registration_id: Some(registration_id.to_string()),
    };
    match self
      .execute_registration_transaction(transaction, fence)
      .await
    {
      Ok(()) => {}
      Err(RegistrationTransactionFailure::Fenced(rejection)) => return Err(rejection),
      Err(RegistrationTransactionFailure::Runtime(error)) => {
        return Ok(registration_mutation_runtime_rejected(
          request_id,
          "unregister entrypoint",
          error,
        ));
      }
      Err(RegistrationTransactionFailure::Storage(error)) => {
        return Ok(registration_mutation_storage_rejected(
          request_id,
          "unregister entrypoint",
          error,
        ));
      }
    }

    Ok(BasicResponse {
      request_id,
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
      .unregister_fenced(
        "pipe-disconnect".to_string(),
        registration_id,
        shim_session_nonce,
        &fence,
      )
      .await
      .ok();
    if !response.as_ref().is_some_and(|response| response.accepted) {
      self.logs.append(
        LogStreamIdentity::runtime_control(),
        LogSeverity::Warn,
        response
          .map(|response| response.message)
          .unwrap_or_else(|| "Entrypoint cleanup was cancelled during daemon drain.".to_string()),
        LogAttributionKind::RuntimeControl,
        Some("ipc-disconnect-cleanup".to_string()),
      );
    }
  }

  pub async fn heartbeat(&self, request: HeartbeatEntrypointRequest) -> BasicResponse {
    let request_id = request.request_id.clone();
    let fence = match self.issue_operation_fence() {
      Ok(fence) => fence,
      Err(error) => return mutation_rejected(request_id, error),
    };
    self
      .heartbeat_fenced(request, &fence)
      .await
      .unwrap_or_else(|error| mutation_rejected(request_id, error))
  }

  pub(crate) async fn heartbeat_fenced(
    &self,
    request: HeartbeatEntrypointRequest,
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
      request_id: request.request_id,
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
    request: SetEntrypointEnabledRequest,
  ) -> BasicResponse {
    let request_id = request.request_id.clone();
    let fence = match self.issue_operation_fence() {
      Ok(fence) => fence,
      Err(error) => return mutation_rejected(request_id, error),
    };
    self
      .set_entrypoint_enabled_fenced(request, &fence)
      .await
      .unwrap_or_else(|error| mutation_rejected(request_id, error))
  }

  pub(crate) async fn set_entrypoint_enabled_fenced(
    &self,
    request: SetEntrypointEnabledRequest,
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
        request_id: request.request_id,
        accepted: false,
        message: "Entrypoint was not found.".to_string(),
      });
    }
    let transaction = RegistrationTransaction {
      coordinator: candidate_coordinator,
      registrations: candidate_registrations,
      history: RegistrationHistoryDraft {
        summary: format!(
          "{} entrypoint `{}`.",
          if request.enabled {
            "Enabled"
          } else {
            "Disabled"
          },
          request.registration_id
        ),
        registration_id: Some(request.registration_id.clone()),
        domain_key: None,
        details: serde_json::json!({
          "registrationId": request.registration_id,
          "enabled": request.enabled
        }),
      },
      event_registration_id: Some(request.registration_id.clone()),
    };
    match self
      .execute_registration_transaction(transaction, fence)
      .await
    {
      Ok(()) => {}
      Err(RegistrationTransactionFailure::Fenced(rejection)) => return Err(rejection),
      Err(RegistrationTransactionFailure::Runtime(error)) => {
        return Ok(registration_mutation_runtime_rejected(
          request.request_id,
          "update entrypoint activation",
          error,
        ));
      }
      Err(RegistrationTransactionFailure::Storage(error)) => {
        return Ok(registration_mutation_storage_rejected(
          request.request_id,
          "change entrypoint activation",
          error,
        ));
      }
    }

    Ok(BasicResponse {
      request_id: request.request_id,
      accepted: true,
      message: "Entrypoint activation updated.".to_string(),
    })
  }

  pub async fn set_domain_enabled(&self, request: SetDomainEnabledRequest) -> BasicResponse {
    let request_id = request.request_id.clone();
    let fence = match self.issue_operation_fence() {
      Ok(fence) => fence,
      Err(error) => return mutation_rejected(request_id, error),
    };
    self
      .set_domain_enabled_fenced(request, &fence)
      .await
      .unwrap_or_else(|error| mutation_rejected(request_id, error))
  }

  pub(crate) async fn set_domain_enabled_fenced(
    &self,
    request: SetDomainEnabledRequest,
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
        request_id: request.request_id,
        accepted: false,
        message: "Domain was not found.".to_string(),
      });
    }
    let transaction = RegistrationTransaction {
      coordinator: candidate_coordinator,
      registrations: candidate_registrations,
      history: RegistrationHistoryDraft {
        summary: format!(
          "{} domain `{}` on entrypoint `{}`.",
          if request.enabled {
            "Enabled"
          } else {
            "Disabled"
          },
          request.domain_key,
          request.registration_id
        ),
        registration_id: Some(request.registration_id.clone()),
        domain_key: Some(request.domain_key.clone()),
        details: serde_json::json!({
          "registrationId": request.registration_id,
          "domainKey": request.domain_key,
          "enabled": request.enabled
        }),
      },
      event_registration_id: Some(request.registration_id.clone()),
    };
    match self
      .execute_registration_transaction(transaction, fence)
      .await
    {
      Ok(()) => {}
      Err(RegistrationTransactionFailure::Fenced(rejection)) => return Err(rejection),
      Err(RegistrationTransactionFailure::Runtime(error)) => {
        return Ok(registration_mutation_runtime_rejected(
          request.request_id,
          "update domain activation",
          error,
        ));
      }
      Err(RegistrationTransactionFailure::Storage(error)) => {
        return Ok(registration_mutation_storage_rejected(
          request.request_id,
          "change domain activation",
          error,
        ));
      }
    }

    Ok(BasicResponse {
      request_id: request.request_id,
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
  history: RegistrationHistoryDraft,
  event_registration_id: Option<String>,
}

struct RegistrationHistoryDraft {
  summary: String,
  registration_id: Option<String>,
  domain_key: Option<String>,
  details: serde_json::Value,
}

enum RegistrationTransactionFailure {
  Fenced(CommitRejection),
  Runtime(anyhow::Error),
  Storage(anyhow::Error),
}

enum RuntimeTransitionReceipt {
  Apply(crate::runtime::RuntimeApplyReceipt),
  Stop(crate::runtime::RuntimeStopReceipt),
}

impl RuntimeTransitionReceipt {
  fn accept(&mut self, logs: &CaddyLogStore) -> Result<()> {
    match self {
      Self::Apply(receipt) => receipt.accept(logs),
      Self::Stop(receipt) => receipt.accept(),
    }
  }

  async fn rollback(self, logs: &CaddyLogStore) -> Result<()> {
    match self {
      Self::Apply(receipt) => receipt.rollback(logs).await,
      Self::Stop(receipt) => receipt.rollback(logs).await,
    }
  }

  async fn projected_state(&self) -> cadder_protocol::RuntimeState {
    match self {
      Self::Apply(receipt) => receipt.projected_state().await,
      Self::Stop(receipt) => receipt.projected_state(),
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
    state.logs.append(
      LogStreamIdentity::runtime_control(),
      LogSeverity::Error,
      format!(
        "runtime transition rollback failed after rejected registration; Cadder entered read-only drain: {error:#}"
      ),
      LogAttributionKind::RuntimeControl,
      Some("registration-rollback".to_string()),
    );
  }
}

fn log_runtime_transition_failure(logs: &CaddyLogStore, error: &anyhow::Error) {
  logs.append(
    LogStreamIdentity::runtime_control(),
    LogSeverity::Error,
    format!("Caddy runtime transition failed; Cadder is restoring the previous state: {error:#}"),
    LogAttributionKind::RuntimeControl,
    Some("registration-runtime-transition".to_string()),
  );
}

fn registration_runtime_rejected(
  request_id: String,
  error: impl std::fmt::Display,
) -> RegisterEntrypointResponse {
  RegisterEntrypointResponse {
    request_id,
    accepted: false,
    message: format!("Entrypoint registration could not update Caddy: {error}."),
    registration_id: None,
  }
}

fn registration_storage_rejected(
  request_id: String,
  error: impl std::fmt::Display,
) -> RegisterEntrypointResponse {
  RegisterEntrypointResponse {
    request_id,
    accepted: false,
    message: format!(
      "Entrypoint registration was not committed because durable storage failed: {error}. Cadder kept the previous state. Resolve the reported storage error, then retry."
    ),
    registration_id: None,
  }
}

fn registration_mutation_runtime_rejected(
  request_id: String,
  operation: &str,
  error: impl std::fmt::Display,
) -> BasicResponse {
  BasicResponse {
    request_id,
    accepted: false,
    message: format!("Could not {operation} because Caddy rejected the runtime update: {error}."),
  }
}

fn registration_mutation_storage_rejected(
  request_id: String,
  operation: &str,
  error: impl std::fmt::Display,
) -> BasicResponse {
  BasicResponse {
    request_id,
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

fn registration_owner_conflict(request_id: String) -> RegisterEntrypointResponse {
  RegisterEntrypointResponse {
    request_id,
    accepted: false,
    message: "Entrypoint registration ID is already owned by another shim session.".to_string(),
    registration_id: None,
  }
}

fn mutation_rejected(request_id: String, error: CommitRejection) -> BasicResponse {
  BasicResponse {
    request_id,
    accepted: false,
    message: mutation_admission_message(error),
  }
}

fn mutation_admission_message(error: CommitRejection) -> String {
  format!("Daemon mutation rejected: {error}.")
}
