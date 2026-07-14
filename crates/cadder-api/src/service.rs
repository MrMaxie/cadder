use crate::{
  OperatorError, daemon_error_indicates_unavailable, daemon_status_connected, domains_view,
  error::sentence,
  resolve_domain,
  view::{ConnectionStateView, LogsView, SelectedDomain},
};
use cadder_daemon::{
  CadderClient, DaemonLaunchOptions, IpcClientError, IpcClientResult, RuntimePaths,
  StateSubscription, ensure_daemon_running_with_options,
};
use cadder_ipc::{
  BasicResponse, GuiStateSnapshot, LegacyCorrelatedRequest, LogSeverity, LogStreamIdentity,
  QueryLogsRequest, QueryStateRequest, QueryStateResponse, SetDomainEnabledRequest,
  SetEntrypointEnabledRequest, message_types, new_request_id,
};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainSelector {
  pub domain: String,
  pub registration: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogsTarget {
  Runtime,
  Entrypoint { registration_id: String },
  Domain(DomainSelector),
}

#[derive(Debug, Clone)]
pub struct OperatorContext {
  paths: RuntimePaths,
  client: CadderClient,
  launch_options: DaemonLaunchOptions,
}

impl OperatorContext {
  pub fn new(
    command: &'static str,
    runtime_dir: Option<PathBuf>,
    launch_options: DaemonLaunchOptions,
  ) -> Result<Self, OperatorError> {
    let paths = RuntimePaths::resolve_with_profile(runtime_dir, launch_options.runtime_profile)
      .map_err(|error| {
        OperatorError::invalid_usage(
          command,
          format!("Could not resolve the Cadder runtime directory: {error}."),
          None,
        )
      })?;
    Ok(Self::from_paths(paths, launch_options))
  }

  pub fn from_paths(paths: RuntimePaths, launch_options: DaemonLaunchOptions) -> Self {
    Self {
      client: CadderClient::new(paths.clone()),
      paths,
      launch_options,
    }
  }

  pub fn paths(&self) -> &RuntimePaths {
    &self.paths
  }

  pub fn client(&self) -> &CadderClient {
    &self.client
  }

  pub fn launch_options(&self) -> &DaemonLaunchOptions {
    &self.launch_options
  }

  pub async fn query_state_response(&self) -> IpcClientResult<QueryStateResponse> {
    self
      .client
      .request(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: new_request_id("ctl-state"),
        },
      )
      .await
  }

  pub async fn query_snapshot(
    &self,
    command: &'static str,
    action: &str,
  ) -> Result<GuiStateSnapshot, OperatorError> {
    let response = self.query_state_response().await.map_err(|error| {
      OperatorError::daemon_request(command, self.paths.runtime_dir(), action, error)
    })?;
    response.snapshot.ok_or_else(|| {
      OperatorError::new(
        command,
        crate::OperatorErrorKind::IpcFailure,
        "Cadder daemon returned no state snapshot.".to_string(),
        Some(
          "Retry the command after the daemon finishes its current state transition.".to_string(),
        ),
      )
    })
  }

  pub async fn request_basic<TRequest>(
    &self,
    command: &'static str,
    action: &str,
    message_type: &str,
    response_type: &str,
    request: &TRequest,
  ) -> Result<BasicResponse, OperatorError>
  where
    TRequest: LegacyCorrelatedRequest<Response = BasicResponse>,
  {
    self
      .client
      .request(message_type, response_type, request)
      .await
      .map_err(|error| {
        OperatorError::daemon_request(command, self.paths.runtime_dir(), action, error)
      })
  }

  pub async fn query_logs(
    &self,
    command: &'static str,
    action: &str,
    stream: LogStreamIdentity,
    limit: usize,
    cursor: Option<String>,
    minimum_severity: Option<LogSeverity>,
  ) -> Result<LogsView, OperatorError> {
    let response = self
      .client
      .request::<_>(
        message_types::QUERY_LOGS_REQUEST,
        message_types::QUERY_LOGS_RESPONSE,
        &QueryLogsRequest {
          request_id: new_request_id("ctl-logs"),
          stream,
          limit: Some(limit),
          cursor,
          minimum_severity,
        },
      )
      .await
      .map_err(|error| {
        OperatorError::daemon_request(command, self.paths.runtime_dir(), action, error)
      })?;
    Ok(LogsView::from(response))
  }

  pub async fn subscribe_state(
    &self,
    command: &'static str,
    action: &str,
  ) -> Result<StateSubscription, OperatorError> {
    self
      .client
      .subscribe_state(new_request_id("ctl-watch"))
      .await
      .map_err(|error| {
        OperatorError::daemon_request(command, self.paths.runtime_dir(), action, error)
      })
  }

  pub async fn ensure_daemon_running(&self, command: &'static str) -> Result<(), OperatorError> {
    ensure_daemon_running_with_options(&self.paths, self.launch_options.clone())
      .await
      .map_err(|error| OperatorError::daemon_start(command, self.paths.runtime_dir(), error))
  }

  pub async fn resolve_logs_target(
    &self,
    command: &'static str,
    target: LogsTarget,
  ) -> Result<LogStreamIdentity, OperatorError> {
    match target {
      LogsTarget::Runtime => Ok(LogStreamIdentity::runtime_control()),
      LogsTarget::Entrypoint { registration_id } => {
        let snapshot = self.query_snapshot(command, "query entrypoints").await?;
        let entrypoint = snapshot
          .registrations
          .iter()
          .find(|entrypoint| entrypoint.registration_id == registration_id)
          .ok_or_else(|| {
            OperatorError::target_not_found(
              command,
              format!("Entrypoint `{registration_id}` was not found."),
              Some("Run `cadder entrypoints list` to inspect valid registration IDs.".to_string()),
            )
          })?;
        Ok(entrypoint.log_stream.clone())
      }
      LogsTarget::Domain(selector) => {
        let snapshot = self.query_snapshot(command, "query domains").await?;
        let selected = self.select_domain(command, &snapshot, &selector)?;
        let domain = domains_view(&snapshot, Some(&selected.registration_id))
          .domains
          .into_iter()
          .find(|domain| domain.canonical_domain == selected.canonical_domain)
          .ok_or_else(|| {
            OperatorError::target_not_found(
              command,
              "The selected domain no longer exists in the current daemon snapshot.",
              Some("Retry the command after refreshing the daemon state.".to_string()),
            )
          })?;
        Ok(domain.log_stream)
      }
    }
  }

  pub async fn set_entrypoint_enabled(
    &self,
    command: &'static str,
    registration_id: String,
    enabled: bool,
  ) -> Result<BasicResponse, OperatorError> {
    let response = self
      .request_basic(
        command,
        "toggle entrypoint activation",
        message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
        message_types::SET_ENTRYPOINT_ENABLED_RESPONSE,
        &SetEntrypointEnabledRequest {
          request_id: new_request_id("ctl-entrypoint-toggle"),
          registration_id: registration_id.clone(),
          shim_session_nonce: None,
          enabled,
        },
      )
      .await?;

    if !response.accepted {
      return Err(OperatorError::target_not_found(
        command,
        format!("Entrypoint `{registration_id}` was not found."),
        Some("Run `cadder entrypoints list` to inspect valid registration IDs.".to_string()),
      ));
    }

    Ok(response)
  }

  pub async fn set_domain_enabled(
    &self,
    command: &'static str,
    selector: &DomainSelector,
    enabled: bool,
  ) -> Result<BasicResponse, OperatorError> {
    let snapshot = self.query_snapshot(command, "query domains").await?;
    let selected = self.select_domain(command, &snapshot, selector)?;
    let response = self
      .request_basic(
        command,
        "toggle domain activation",
        message_types::SET_DOMAIN_ENABLED_REQUEST,
        message_types::SET_DOMAIN_ENABLED_RESPONSE,
        &SetDomainEnabledRequest {
          request_id: new_request_id("ctl-domain-toggle"),
          registration_id: selected.registration_id,
          domain_key: selected.canonical_domain,
          enabled,
        },
      )
      .await?;
    if !response.accepted {
      return Err(OperatorError::conflict_or_rejected(
        command,
        response.message,
        Some(
          "Refresh the domain list and retry the command with an explicit `--registration` filter if needed.".to_string(),
        ),
      ));
    }
    Ok(response)
  }

  pub fn select_domain(
    &self,
    command: &'static str,
    snapshot: &GuiStateSnapshot,
    selector: &DomainSelector,
  ) -> Result<SelectedDomain, OperatorError> {
    resolve_domain(snapshot, &selector.domain, selector.registration.as_deref()).map_err(|error| {
      match error {
        crate::DomainResolveError::NotFound {
          canonical_domain,
          requested_registration,
        } => OperatorError::target_not_found(
          command,
          match requested_registration {
            Some(registration_id) => format!(
              "Domain `{canonical_domain}` was not found under entrypoint `{registration_id}`."
            ),
            None => format!("Domain `{canonical_domain}` was not found."),
          },
          Some(
            "Run `cadder domains list` to inspect the current domain registrations.".to_string(),
          ),
        ),
        crate::DomainResolveError::Ambiguous {
          canonical_domain,
          matches,
        } => OperatorError::conflict_or_rejected(
          command,
          format!(
            "Domain `{canonical_domain}` matches multiple entrypoints: {}.",
            matches.join(", ")
          ),
          Some(
            "Retry the command with `--registration <id>` to choose the exact entrypoint."
              .to_string(),
          ),
        ),
      }
    })
  }
}

pub fn connection_state_from_error(error: &IpcClientError) -> ConnectionStateView {
  if daemon_error_indicates_unavailable(error) || error.is_stale_instance() {
    ConnectionStateView::NotRunning
  } else {
    ConnectionStateView::ConnectionFailed
  }
}

pub fn unavailable_status(
  context: &OperatorContext,
  error: &IpcClientError,
) -> crate::DaemonStatusView {
  let connection_state = connection_state_from_error(error);
  let message = match connection_state {
    ConnectionStateView::NotRunning => "Cadder is not running.".to_string(),
    ConnectionStateView::ConnectionFailed => {
      format!("Could not connect to Cadder: {}", sentence(error.message()))
    }
    ConnectionStateView::Connected => "Connected to Cadder.".to_string(),
  };

  crate::daemon_status_unavailable(
    context.paths.runtime_dir(),
    connection_state,
    message,
    Some(if connection_state == ConnectionStateView::NotRunning {
      crate::start_guidance(context.paths.runtime_dir())
    } else {
      error.guidance().map(ToOwned::to_owned).unwrap_or_else(|| {
        "Inspect the daemon diagnostics for this runtime, correct the reported error, then retry."
          .to_string()
      })
    }),
  )
}

pub fn connected_status(
  context: &OperatorContext,
  snapshot: &GuiStateSnapshot,
) -> crate::DaemonStatusView {
  daemon_status_connected(context.paths.runtime_dir(), snapshot)
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_ipc::{
    ActivationState, ConfigState, DomainName, EntrypointInstanceIdentity, EntrypointRegistration,
    OwnerProcessIdentity, ProtocolError, ProtocolErrorCode, ProtocolErrorKind, RegisteredDomain,
    RuntimeState, SourcePath,
  };
  use chrono::{TimeZone, Utc};

  fn timestamp() -> chrono::DateTime<Utc> {
    Utc
      .with_ymd_and_hms(2026, 6, 17, 11, 30, 0)
      .single()
      .unwrap()
  }

  fn registration(id: &str, domains: &[&str]) -> EntrypointRegistration {
    let now = timestamp();
    let identity = EntrypointInstanceIdentity {
      instance_id: id.to_string(),
      started_at_utc: now,
      shim_session_nonce: format!("{id}-nonce"),
    };
    EntrypointRegistration {
      registration_id: id.to_string(),
      entrypoint_instance: identity.clone(),
      source_working_directory: SourcePath::new("D:/Projects/App", None),
      source_config_path: SourcePath::new("D:/Projects/App/Caddyfile", None),
      registered_domains: domains
        .iter()
        .map(|domain| RegisteredDomain {
          name: DomainName::parse(*domain),
          activation_state: ActivationState::Active,
          upstream: None,
          log_stream: LogStreamIdentity::domain(domain),
        })
        .collect(),
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 42,
        process_start_time_utc: now,
        shim_session_nonce: identity.shim_session_nonce.clone(),
        executable_path: Some("caddy.exe".to_string()),
      },
      log_stream: LogStreamIdentity::entrypoint(id),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
    }
  }

  fn snapshot(registrations: Vec<EntrypointRegistration>) -> GuiStateSnapshot {
    GuiStateSnapshot {
      captured_at_utc: timestamp(),
      registrations,
      runtime: RuntimeState::idle(),
      config: ConfigState::idle(),
      storage: None,
    }
  }

  fn context() -> (tempfile::TempDir, OperatorContext) {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    (
      temp,
      OperatorContext::from_paths(paths, DaemonLaunchOptions::default()),
    )
  }

  #[test]
  fn context_preserves_runtime_paths_and_launch_options() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    let daemon_path = temp.path().join("cadderd.exe");
    let context = OperatorContext::new(
      "test",
      Some(runtime_dir.clone()),
      DaemonLaunchOptions {
        explicit_daemon: Some(daemon_path.clone()),
        real_caddy_override: Some(PathBuf::from("caddy-real")),
        launch_mode: cadder_daemon::DaemonLaunchMode::ForegroundDiagnostic,
        ..DaemonLaunchOptions::default()
      },
    )
    .unwrap();

    assert_eq!(context.paths().runtime_dir(), runtime_dir.as_path());
    assert_eq!(
      context.launch_options().explicit_daemon.as_deref(),
      Some(daemon_path.as_path())
    );
    assert_eq!(
      context.launch_options().real_caddy_override.as_deref(),
      Some(std::path::Path::new("caddy-real"))
    );
    assert_eq!(
      context.launch_options().launch_mode,
      cadder_daemon::DaemonLaunchMode::ForegroundDiagnostic
    );
  }

  #[tokio::test]
  async fn typed_error_operator_preserves_local_ipc_source_and_machine_fields() {
    let (_temp, context) = context();

    let error = context
      .query_snapshot("status", "query state")
      .await
      .unwrap_err();

    let ipc_error = std::error::Error::source(&error)
      .and_then(|source| source.downcast_ref::<IpcClientError>())
      .expect("operator error should retain the typed IPC error");
    let local = ipc_error
      .local_error()
      .expect("missing daemon should remain a local IPC error");
    assert_eq!(local.phase(), cadder_daemon::IpcClientPhase::DiscoveryRead);
    assert!(std::error::Error::source(local).is_some());

    let json = serde_json::to_value(&error).unwrap();
    assert_eq!(json["local_error"]["code"], local.code().as_str());
    assert_eq!(json["local_error"]["message"], local.message());
    assert_eq!(
      json["local_error"]["guidance"],
      local
        .guidance()
        .expect("discovery error should include guidance")
    );
    assert_eq!(json["local_error"]["retryable"], local.retryable());
  }

  #[test]
  fn select_domain_returns_precise_operator_errors() {
    let (_temp, context) = context();
    let snapshot = snapshot(vec![
      registration("shim-1", &["app.localhost"]),
      registration("shim-2", &["app.localhost", "api.localhost"]),
    ]);

    let selected = context
      .select_domain(
        "domains enable",
        &snapshot,
        &DomainSelector {
          domain: "api.localhost".to_string(),
          registration: None,
        },
      )
      .unwrap();
    assert_eq!(selected.registration_id, "shim-2");
    assert_eq!(selected.canonical_domain, "api.localhost");

    let missing = context
      .select_domain(
        "domains enable",
        &snapshot,
        &DomainSelector {
          domain: "missing.localhost".to_string(),
          registration: Some("shim-1".to_string()),
        },
      )
      .unwrap_err();
    assert_eq!(missing.kind, crate::OperatorErrorKind::TargetNotFound);
    assert!(missing.message.contains("shim-1"));

    let missing_any_entrypoint = context
      .select_domain(
        "domains enable",
        &snapshot,
        &DomainSelector {
          domain: "missing.localhost".to_string(),
          registration: None,
        },
      )
      .unwrap_err();
    assert_eq!(
      missing_any_entrypoint.kind,
      crate::OperatorErrorKind::TargetNotFound
    );
    assert_eq!(
      missing_any_entrypoint.message,
      "Domain `missing.localhost` was not found."
    );

    let ambiguous = context
      .select_domain(
        "domains enable",
        &snapshot,
        &DomainSelector {
          domain: "app.localhost".to_string(),
          registration: None,
        },
      )
      .unwrap_err();
    assert_eq!(ambiguous.kind, crate::OperatorErrorKind::ConflictOrRejected);
    assert!(ambiguous.message.contains("shim-1, shim-2"));
  }

  #[test]
  fn selector_and_logs_target_contracts_are_stable() {
    let selector = DomainSelector {
      domain: "App.Localhost".to_string(),
      registration: Some("shim-1".to_string()),
    };
    let target = LogsTarget::Domain(selector.clone());

    assert_eq!(selector.clone(), selector);
    assert_eq!(target.clone(), LogsTarget::Domain(selector));
    assert!(format!("{target:?}").contains("Domain"));
    assert_eq!(LogsTarget::Runtime, LogsTarget::Runtime.clone());
    assert_eq!(
      LogsTarget::Entrypoint {
        registration_id: "shim-1".to_string(),
      },
      LogsTarget::Entrypoint {
        registration_id: "shim-1".to_string(),
      }
      .clone()
    );
  }

  #[tokio::test]
  async fn connected_and_unavailable_statuses_include_operator_context() {
    let (_temp, context) = context();
    let status = connected_status(
      &context,
      &snapshot(vec![registration(
        "shim-1",
        &["app.localhost", "api.localhost"],
      )]),
    );

    assert_eq!(status.connection_state, ConnectionStateView::Connected);
    assert_eq!(status.counts.entrypoints, 1);
    assert_eq!(status.counts.domains, 2);
    assert_eq!(status.counts.active_domains, 2);

    let not_running = context.query_state_response().await.unwrap_err();
    let status = unavailable_status(&context, &not_running);
    assert_eq!(status.connection_state, ConnectionStateView::NotRunning);
    assert!(status.message.contains("is not running"));
    assert!(
      status
        .guidance
        .as_deref()
        .is_some_and(|guidance| guidance == "Open Cadder and start it from Status, then retry.")
    );

    let failed = IpcClientError::Daemon(ProtocolError::new(
      ProtocolErrorKind::IncompatibleProtocolVersion,
      ProtocolErrorCode::parse("incompatible_protocol").unwrap(),
      "protocol mismatch",
      Some("Upgrade the older Cadder component.".into()),
      false,
    ));
    let status = unavailable_status(&context, &failed);
    assert_eq!(
      status.connection_state,
      ConnectionStateView::ConnectionFailed
    );
    assert!(status.message.contains("protocol mismatch"));
    assert_eq!(
      status.guidance.as_deref(),
      Some("Upgrade the older Cadder component.")
    );
    assert!(!status.guidance.unwrap().contains("daemon start"));

    let stale = IpcClientError::Daemon(ProtocolError::new(
      ProtocolErrorKind::StaleInstance,
      ProtocolErrorCode::parse("stale_instance").unwrap(),
      "the published daemon instance is no longer reachable",
      None,
      true,
    ));
    assert_eq!(
      connection_state_from_error(&stale),
      ConnectionStateView::NotRunning
    );
  }
}
