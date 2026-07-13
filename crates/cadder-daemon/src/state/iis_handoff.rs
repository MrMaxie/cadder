use super::*;

impl DaemonState {
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

  pub(crate) async fn set_iis_handoff_fenced(
    &self,
    request: SetIisHandoffRequest,
    fence: &OperationFence,
  ) -> Result<SetIisHandoffResponse, CommitRejection> {
    fence.commit_final(|| ())?;
    let issue = IisIssue::new(
      IisIssueKind::HandoffUnavailable,
      "IIS handoff changes are unavailable on this installation. Upgrade Cadder before retrying this operation.",
    );
    Ok(iis_failure(request.request_id, issue, None, Vec::new()))
  }

  #[cfg(test)]
  pub async fn set_iis_handoff(&self, request: SetIisHandoffRequest) -> SetIisHandoffResponse {
    let request_id = request.request_id.clone();
    let Ok(_operation) = self.iis_operation.try_lock() else {
      let issue = IisIssue::new(
        IisIssueKind::Busy,
        "Another IIS handoff operation is already running.",
      );
      return iis_failure(request_id, issue, None, Vec::new());
    };

    if request.enabled {
      self.enable_iis_handoff(request).await
    } else {
      self.disable_iis_handoff(request).await
    }
  }

  #[cfg(test)]
  async fn enable_iis_handoff(&self, request: SetIisHandoffRequest) -> SetIisHandoffResponse {
    let records = match self.iis_provider.discover().await {
      Ok(records) => records,
      Err(issue) => {
        return iis_failure(request.request_id, issue, None, Vec::new());
      }
    };
    let Some(binding) = records
      .iter()
      .find(|binding| binding.binding_id() == request.binding_id)
      .cloned()
    else {
      let issue = IisIssue::new(IisIssueKind::MissingBinding, "IIS binding was not found.");
      return iis_failure(request.request_id, issue, None, Vec::new());
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
      return iis_failure(request.request_id, issue, Some(view), steps);
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
        return iis_failure(request.request_id, issue, Some(view), steps);
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
      return iis_failure(request.request_id, issue, Some(view), steps);
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
      return iis_failure(request.request_id, issue, Some(view), steps);
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
          return iis_failure(request.request_id, issue, Some(view), steps);
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
      return iis_failure(request.request_id, issue, None, steps);
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
      return iis_failure_with_actions(
        request.request_id,
        issue,
        Some(view),
        steps,
        follow_up_actions,
      );
    }
    mark_privileged_batch_approved(&mut steps, &privileged_step_ids);

    let apply_state = {
      let _operation = self
        .config_operation
        .acquire()
        .await
        .expect("config operation semaphore closed");
      {
        let mut coordinator = self.coordinator.lock().await;
        coordinator.set_iis_proxy_route(
          binding.binding_id(),
          domain_key.clone(),
          backend_dial(&backend_binding),
          IisProxyBackendProtocol::from_iis_protocol(&backend_binding.protocol),
        );
      }
      let registrations = {
        let inner = self.inner.lock().await;
        inner.registrations.values().cloned().collect::<Vec<_>>()
      };
      let apply_state = self.apply_registrations(registrations).await;
      self
        .publish_change(StateChangeKind::RegistrationsChanged, None)
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
        let _operation = self
          .config_operation
          .acquire()
          .await
          .expect("config operation semaphore closed");
        {
          let mut coordinator = self.coordinator.lock().await;
          coordinator.remove_iis_proxy_route(&domain_key);
        }
        let registrations = {
          let inner = self.inner.lock().await;
          inner.registrations.values().cloned().collect::<Vec<_>>()
        };
        self.apply_registrations(registrations).await;
        self
          .publish_change(StateChangeKind::RegistrationsChanged, None)
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
      return iis_failure_with_actions(
        request.request_id,
        issue,
        Some(view),
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

  #[cfg(test)]
  async fn disable_iis_handoff(&self, request: SetIisHandoffRequest) -> SetIisHandoffResponse {
    let mut steps = disable_iis_steps();
    let handoffs = self.iis_store.snapshot().await;
    let Some(restore) = handoffs.get(&request.binding_id).cloned() else {
      let issue = IisIssue::new(
        IisIssueKind::MissingBinding,
        "IIS handoff restore metadata was not found.",
      );
      mark_step_issue(&mut steps, "iis-read-restore-metadata", &issue);
      return iis_failure(request.request_id, issue, None, steps);
    };
    mark_step_succeeded(&mut steps, "iis-read-restore-metadata");
    let backend_binding = legacy_backend_binding(&restore);

    {
      let front_door_needed = {
        let inner = self.inner.lock().await;
        caddy_front_door_needed(&inner.registrations, &restore.domain_key)
      };
      let other_iis_routes = {
        let coordinator = self.coordinator.lock().await;
        coordinator.has_iis_proxy_routes_except(&restore.domain_key)
      };
      if front_door_needed || other_iis_routes {
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
        return iis_failure(request.request_id, issue, Some(view), steps);
      }
    }

    {
      let _operation = self
        .config_operation
        .acquire()
        .await
        .expect("config operation semaphore closed");
      {
        let mut coordinator = self.coordinator.lock().await;
        coordinator.remove_iis_proxy_route(&restore.domain_key);
      }
      let registrations = {
        let inner = self.inner.lock().await;
        inner.registrations.values().cloned().collect::<Vec<_>>()
      };
      self.apply_registrations(registrations).await;
      self
        .publish_change(StateChangeKind::RegistrationsChanged, None)
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
      let _operation = self
        .config_operation
        .acquire()
        .await
        .expect("config operation semaphore closed");
      {
        let mut coordinator = self.coordinator.lock().await;
        coordinator.set_iis_proxy_route(
          request.binding_id.clone(),
          restore.domain_key.clone(),
          backend_dial(&backend_binding),
          IisProxyBackendProtocol::from_iis_protocol(&backend_binding.protocol),
        );
      }
      let registrations = {
        let inner = self.inner.lock().await;
        inner.registrations.values().cloned().collect::<Vec<_>>()
      };
      self.apply_registrations(registrations).await;
      self
        .publish_change(StateChangeKind::RegistrationsChanged, None)
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
      return iis_failure_with_actions(
        request.request_id,
        issue.clone(),
        Some(view),
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
      return iis_failure_with_actions(
        request.request_id,
        issue,
        None,
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

pub(super) fn handoff_binding_to_view(
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

pub(super) fn duplicate_iis_hosts(records: &[IisBindingRecord]) -> BTreeSet<String> {
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

#[cfg(test)]
pub(super) fn iis_route_host_conflicts(
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

pub(super) fn route_host_for_binding(
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

pub(super) fn extract_host_candidate(raw: &str) -> &str {
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

pub(super) fn backend_port_for_binding(binding: &IisBindingRecord) -> u16 {
  let hash = binding.binding_id().bytes().fold(0_u32, |hash, byte| {
    hash.wrapping_mul(33).wrapping_add(byte as u32)
  });
  41000 + (hash % 8000) as u16
}

pub(super) fn legacy_backend_binding(restore: &IisRestoreRecord) -> IisBindingRecord {
  restore.backend_binding.clone().unwrap_or_else(|| {
    restore.binding.backend_http_binding(
      backend_port_for_binding(&restore.binding),
      &restore.domain_key,
    )
  })
}

pub(super) fn backend_dial(binding: &IisBindingRecord) -> String {
  format!("127.0.0.1:{}", binding.port)
}

pub(super) fn active_registration_conflict(
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

#[cfg(test)]
pub(super) fn caddy_front_door_needed(
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

#[cfg(test)]
pub(super) fn enable_iis_steps() -> Vec<IisOperationStep> {
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

#[cfg(test)]
pub(super) fn disable_iis_steps() -> Vec<IisOperationStep> {
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

#[cfg(test)]
pub(super) fn mark_step_succeeded(steps: &mut [IisOperationStep], step_id: &str) {
  if let Some(step) = steps.iter_mut().find(|step| step.step_id == step_id) {
    step.status = IisOperationStepStatus::Succeeded;
    step.approval = IisElevationApproval::NotRequired;
  }
}

#[cfg(test)]
pub(super) fn mark_step_issue(steps: &mut [IisOperationStep], step_id: &str, issue: &IisIssue) {
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

#[cfg(test)]
pub(super) fn mark_privileged_batch_approved(steps: &mut [IisOperationStep], step_ids: &[&str]) {
  for step_id in step_ids {
    if let Some(step) = steps.iter_mut().find(|step| step.step_id == *step_id) {
      step.status = IisOperationStepStatus::Succeeded;
      step.approval = IisElevationApproval::Approved;
    }
  }
}

#[cfg(test)]
pub(super) fn mark_privileged_batch_issue(
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

#[cfg(test)]
pub(super) fn mark_step_skipped(steps: &mut [IisOperationStep], step_id: &str) {
  if let Some(step) = steps.iter_mut().find(|step| step.step_id == step_id) {
    step.status = IisOperationStepStatus::Skipped;
  }
}

#[cfg(test)]
pub(super) fn follow_up_actions_for_issue(issue: &IisIssue) -> Vec<IisFollowUpAction> {
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

pub(super) fn iis_failure(
  request_id: String,
  issue: IisIssue,
  binding: Option<IisBinding>,
  steps: Vec<IisOperationStep>,
) -> SetIisHandoffResponse {
  iis_failure_with_actions(request_id, issue, binding, steps, Vec::new())
}

pub(super) fn iis_failure_with_actions(
  request_id: String,
  issue: IisIssue,
  binding: Option<IisBinding>,
  steps: Vec<IisOperationStep>,
  follow_up_actions: Vec<IisFollowUpAction>,
) -> SetIisHandoffResponse {
  iis_response_with_steps(
    request_id,
    false,
    issue.message.clone(),
    binding,
    issue,
    steps,
    follow_up_actions,
  )
}

pub(super) fn iis_response_with_steps(
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
