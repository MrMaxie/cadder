use super::*;

impl DaemonState {
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

    let _operation = self.config_operation.lock().await;
    {
      let inner = self.inner.lock().await;
      if inner
        .registrations
        .get(&registration.registration_id)
        .is_some_and(|existing| {
          existing.entrypoint_instance.shim_session_nonce
            != registration.entrypoint_instance.shim_session_nonce
        })
      {
        return RegisterEntrypointResponse {
          request_id,
          accepted: false,
          message: "Entrypoint registration ID is already owned by another shim session."
            .to_string(),
          registration_id: None,
        };
      }
    }

    let source_path = registration.source_config_path.raw.clone();
    let adapter = {
      let coordinator = self.coordinator.lock().await;
      coordinator.adapter()
    };
    let prepared = adapter.prepare(registration).await;
    {
      let inner = self.inner.lock().await;
      if inner
        .registrations
        .get(&prepared.registration.registration_id)
        .is_some_and(|existing| {
          existing.entrypoint_instance.shim_session_nonce
            != prepared.registration.entrypoint_instance.shim_session_nonce
        })
      {
        return RegisterEntrypointResponse {
          request_id,
          accepted: false,
          message: "Entrypoint registration ID is already owned by another shim session."
            .to_string(),
          registration_id: None,
        };
      }
    }
    let mut prepared = {
      let mut coordinator = self.coordinator.lock().await;
      coordinator.commit_prepared_registration(source_path, prepared)
    };
    let now = Utc::now();
    prepared.created_at_utc = now;
    prepared.last_heartbeat_utc = now;
    let id = prepared.registration_id.clone();
    let registrations = {
      let mut inner = self.inner.lock().await;
      inner.registrations.insert(id.clone(), prepared);
      inner.registrations.values().cloned().collect::<Vec<_>>()
    };
    self.apply_registrations(registrations.clone()).await;
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
    self
      .publish_change(StateChangeKind::RegistrationsChanged, Some(id.clone()))
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
    let _operation = self.config_operation.lock().await;
    let (removed, removed_registration, registrations) = {
      let mut inner = self.inner.lock().await;
      let removed = inner
        .registrations
        .get(registration_id)
        .is_some_and(|registration| {
          registration.entrypoint_instance.shim_session_nonce == shim_session_nonce
        });
      if !removed {
        (false, None, Vec::new())
      } else {
        let removed_registration = inner.registrations.remove(registration_id);
        (
          true,
          removed_registration,
          inner.registrations.values().cloned().collect::<Vec<_>>(),
        )
      }
    };
    if removed {
      self.apply_registrations(registrations).await;
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
      self
        .publish_change(
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

  pub(crate) async fn unregister_for_ipc_disconnect(
    &self,
    registration_id: &str,
    shim_session_nonce: &str,
  ) {
    let response = self
      .unregister(
        "pipe-disconnect".to_string(),
        registration_id,
        shim_session_nonce,
      )
      .await;
    if !response.accepted {
      self.logs.append(
        LogStreamIdentity::runtime_control(),
        LogSeverity::Warn,
        response.message,
        LogAttributionKind::RuntimeControl,
        Some("ipc-disconnect-cleanup".to_string()),
      );
    }
  }

  pub async fn heartbeat(&self, request: HeartbeatEntrypointRequest) -> BasicResponse {
    let registration_id = request.registration_id.clone();
    let accepted = {
      let mut inner = self.inner.lock().await;
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
    };
    if accepted {
      self
        .publish_change(StateChangeKind::RegistrationsChanged, Some(registration_id))
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
    let _operation = self.config_operation.lock().await;
    let (accepted, registrations) = {
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
      (
        accepted,
        inner.registrations.values().cloned().collect::<Vec<_>>(),
      )
    };

    if accepted {
      self.apply_registrations(registrations).await;
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
      self
        .publish_change(
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
    let _operation = self.config_operation.lock().await;
    let (accepted, registrations) = {
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
      (
        accepted,
        inner.registrations.values().cloned().collect::<Vec<_>>(),
      )
    };

    if accepted {
      self.apply_registrations(registrations).await;
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
      self
        .publish_change(
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
}
