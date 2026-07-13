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

    let _operation = self.config_operation.lock().await;
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
    let (id, registrations) = {
      let mut coordinator = self.coordinator.lock().await;
      let mut inner = self.inner.lock().await;
      if registration_has_different_owner(&inner, &prepared.registration) {
        return Ok(registration_owner_conflict(request_id));
      }
      let id = fence.commit(|| {
        let mut prepared = coordinator.commit_prepared_registration(source_path, prepared);
        let now = Utc::now();
        prepared.created_at_utc = now;
        prepared.last_heartbeat_utc = now;
        let id = prepared.registration_id.clone();
        inner.registrations.insert(id.clone(), prepared);
        id
      })?;
      let registrations = inner.registrations.values().cloned().collect::<Vec<_>>();
      (id, registrations)
    };

    self
      .apply_registrations_fenced(registrations.clone(), fence)
      .await?;
    fence.commit(|| {
      self.store.record_history(
        HistoryKind::Registration,
        format!("Registered entrypoint `{id}`."),
        Some(&id),
        None,
        &serde_json::json!({
          "registrationId": id,
          "domainCount": registrations
            .iter()
            .find(|registration| registration.registration_id == id)
            .map(|registration| registration.registered_domains.len())
            .unwrap_or_default()
        }),
      );
    })?;
    self
      .publish_change_fenced(
        StateChangeKind::RegistrationsChanged,
        Some(id.clone()),
        fence,
      )
      .await?;

    Ok(RegisterEntrypointResponse {
      request_id,
      accepted: true,
      message: "Entrypoint registered.".to_string(),
      registration_id: Some(id),
    })
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
    let _operation = self.config_operation.lock().await;
    let (removed, removed_registration, registrations) = {
      let mut inner = self.inner.lock().await;
      let (removed, removed_registration) = fence.commit(|| {
        let removed = inner
          .registrations
          .get(registration_id)
          .is_some_and(|registration| {
            registration.entrypoint_instance.shim_session_nonce == shim_session_nonce
          });
        if !removed {
          (false, None)
        } else {
          let removed_registration = inner.registrations.remove(registration_id);
          (true, removed_registration)
        }
      })?;
      let registrations = if removed {
        inner.registrations.values().cloned().collect::<Vec<_>>()
      } else {
        Vec::new()
      };
      (removed, removed_registration, registrations)
    };
    if removed {
      self
        .apply_registrations_fenced(registrations, fence)
        .await?;
      fence.commit(|| {
        self.store.record_history(
          HistoryKind::Registration,
          format!("Unregistered entrypoint `{registration_id}`."),
          Some(registration_id),
          None,
          &serde_json::json!({
            "registrationId": registration_id,
            "domainCount": removed_registration
              .as_ref()
              .map(|registration| registration.registered_domains.len())
              .unwrap_or_default()
          }),
        );
      })?;
      self
        .publish_change_fenced(
          StateChangeKind::RegistrationsChanged,
          Some(registration_id.to_string()),
          fence,
        )
        .await?;
    }

    Ok(BasicResponse {
      request_id,
      accepted: removed,
      message: if removed {
        "Entrypoint unregistered."
      } else {
        "Entrypoint was not found for the requested owner."
      }
      .to_string(),
    })
  }

  pub(crate) async fn unregister_for_ipc_disconnect(
    &self,
    registration_id: &str,
    shim_session_nonce: &str,
  ) {
    let response = match self.issue_operation_fence() {
      Ok(fence) => self
        .unregister_fenced(
          "pipe-disconnect".to_string(),
          registration_id,
          shim_session_nonce,
          &fence,
        )
        .await
        .ok(),
      Err(_) => None,
    };
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
    let registration_id = request.registration_id.clone();
    let accepted = {
      let mut inner = self.inner.lock().await;
      fence.commit(|| {
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
    if accepted {
      self
        .publish_change_fenced(
          StateChangeKind::RegistrationsChanged,
          Some(registration_id),
          fence,
        )
        .await?;
    }

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
    let _operation = self.config_operation.lock().await;
    let (accepted, registrations) = {
      let mut inner = self.inner.lock().await;
      let accepted = fence.commit(|| {
        inner
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
          .is_some()
      })?;
      let registrations = inner.registrations.values().cloned().collect::<Vec<_>>();
      (accepted, registrations)
    };

    if accepted {
      self
        .apply_registrations_fenced(registrations, fence)
        .await?;
      fence.commit(|| {
        self.store.record_history(
          HistoryKind::Registration,
          format!(
            "{} entrypoint `{}`.",
            if request.enabled {
              "Enabled"
            } else {
              "Disabled"
            },
            request.registration_id
          ),
          Some(&request.registration_id),
          None,
          &serde_json::json!({
            "registrationId": request.registration_id,
            "enabled": request.enabled
          }),
        );
      })?;
      self
        .publish_change_fenced(
          StateChangeKind::RegistrationsChanged,
          Some(request.registration_id),
          fence,
        )
        .await?;
    }

    Ok(BasicResponse {
      request_id: request.request_id,
      accepted,
      message: if accepted {
        "Entrypoint activation updated."
      } else {
        "Entrypoint was not found."
      }
      .to_string(),
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
    let _operation = self.config_operation.lock().await;
    let (accepted, registrations) = {
      let mut inner = self.inner.lock().await;
      let accepted = fence.commit(|| {
        inner
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
          .is_some()
      })?;
      let registrations = inner.registrations.values().cloned().collect::<Vec<_>>();
      (accepted, registrations)
    };

    if accepted {
      self
        .apply_registrations_fenced(registrations, fence)
        .await?;
      fence.commit(|| {
        self.store.record_history(
          HistoryKind::Registration,
          format!(
            "{} domain `{}` on entrypoint `{}`.",
            if request.enabled {
              "Enabled"
            } else {
              "Disabled"
            },
            request.domain_key,
            request.registration_id
          ),
          Some(&request.registration_id),
          Some(&request.domain_key),
          &serde_json::json!({
            "registrationId": request.registration_id,
            "domainKey": request.domain_key,
            "enabled": request.enabled
          }),
        );
      })?;
      self
        .publish_change_fenced(
          StateChangeKind::RegistrationsChanged,
          Some(request.registration_id),
          fence,
        )
        .await?;
    }

    Ok(BasicResponse {
      request_id: request.request_id,
      accepted,
      message: if accepted {
        "Domain activation updated."
      } else {
        "Domain was not found."
      }
      .to_string(),
    })
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
