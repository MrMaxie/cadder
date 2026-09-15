use crate::{
  OperatorError, daemon_error_indicates_unavailable, daemon_status_connected,
  error::sentence,
  resolve_domain,
  view::{ConnectionStateView, LogsView, SelectedDomain},
};
use cadder_daemon::{
  CadderClient, DaemonLaunchOptions, IpcClientError, IpcClientResult, RuntimePaths,
  ensure_daemon_running_with_options,
};
use cadder_ipc::{
  BasicResponse, CorrelatedRequest, EntrypointRegistration, GuiStateSnapshot, LogStreamIdentity,
  QueryLogsPayload, QueryStatePayload, QueryStateResponse, RegisteredDomain,
  SetDomainEnabledPayload, SetEntrypointEnabledPayload, ShutdownDaemonPayload, new_request_id,
};
use std::path::PathBuf;
use tokio::time::{Duration, sleep};

const DAEMON_STOP_ATTEMPTS: usize = 100;
const DAEMON_STOP_CONFIRMATIONS: usize = 3;
const DAEMON_STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);

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
    let paths = RuntimePaths::resolve(runtime_dir).map_err(|error| {
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
      .request(new_request_id("ctl-state"), &QueryStatePayload::default())
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
        crate::AppExit::IpcFailure,
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
    request_id: String,
    request: &TRequest,
  ) -> Result<BasicResponse, OperatorError>
  where
    TRequest: CorrelatedRequest<Response = BasicResponse>,
  {
    self
      .client
      .request(request_id, request)
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
  ) -> Result<LogsView, OperatorError> {
    let response = self
      .client
      .request::<_>(
        new_request_id("ctl-logs"),
        &QueryLogsPayload {
          stream,
          limit: Some(limit),
        },
      )
      .await
      .map_err(|error| {
        OperatorError::daemon_request(command, self.paths.runtime_dir(), action, error)
      })?;
    Ok(LogsView::from(response))
  }

  pub async fn ensure_daemon_running(&self, command: &'static str) -> Result<(), OperatorError> {
    ensure_daemon_running_with_options(&self.paths, self.launch_options.clone())
      .await
      .map_err(|error| OperatorError::daemon_start(command, self.paths.runtime_dir(), error))
  }

  pub async fn stop_daemon(&self, command: &'static str) -> Result<(), OperatorError> {
    self
      .request_basic(
        command,
        "stop the daemon",
        new_request_id("ctl-shutdown"),
        &ShutdownDaemonPayload::default(),
      )
      .await
      .map(|_| ())
  }

  pub async fn restart_daemon(&self, command: &'static str) -> Result<(), OperatorError> {
    self.stop_daemon(command).await?;
    self.wait_for_daemon_stop(command).await?;
    self.ensure_daemon_running(command).await
  }

  async fn wait_for_daemon_stop(&self, command: &'static str) -> Result<(), OperatorError> {
    let mut stopped_confirmations = 0;
    for _ in 0..DAEMON_STOP_ATTEMPTS {
      match self.query_state_response().await {
        Ok(_) => stopped_confirmations = 0,
        Err(error)
          if error.retryable()
            || daemon_error_indicates_unavailable(&error)
            || error.is_stale_instance() =>
        {
          stopped_confirmations += 1;
          if stopped_confirmations == DAEMON_STOP_CONFIRMATIONS {
            return Ok(());
          }
        }
        Err(error) => {
          return Err(OperatorError::daemon_request(
            command,
            self.paths.runtime_dir(),
            "wait for the daemon to stop",
            error,
          ));
        }
      }
      sleep(DAEMON_STOP_POLL_INTERVAL).await;
    }

    Err(OperatorError::new(
      command,
      crate::AppExit::IpcFailure,
      "Cadder timed out waiting for the previous daemon process to release its local endpoint.",
      Some("Check the daemon diagnostics, then retry the restart once.".to_string()),
    ))
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
        let entrypoint = Self::find_entrypoint(&snapshot, &registration_id).ok_or_else(|| {
          OperatorError::target_not_found(
            command,
            format!("Entrypoint `{registration_id}` was not found."),
            Some("Run `cadder projects list` to inspect registered Caddyfiles.".to_string()),
          )
        })?;
        Ok(entrypoint.log_stream.clone())
      }
      LogsTarget::Domain(selector) => {
        let snapshot = self.query_snapshot(command, "query domains").await?;
        let selected = self.select_domain(command, &snapshot, &selector)?;
        let entrypoint =
          Self::find_entrypoint(&snapshot, &selected.registration_id).ok_or_else(|| {
            OperatorError::target_not_found(
              command,
              "The selected domain no longer exists in the current daemon snapshot.",
              Some("Retry the command after refreshing the daemon state.".to_string()),
            )
          })?;
        let domain = Self::find_domain_in_entrypoint(entrypoint, &selected.canonical_domain)
          .ok_or_else(|| {
            OperatorError::target_not_found(
              command,
              "The selected domain no longer exists in the current daemon snapshot.",
              Some("Retry the command after refreshing the daemon state.".to_string()),
            )
          })?;
        Ok(domain.log_stream.clone())
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
        new_request_id("ctl-entrypoint-toggle"),
        &SetEntrypointEnabledPayload {
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
        Some("Run `cadder projects list` to inspect registered Caddyfiles.".to_string()),
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
        new_request_id("ctl-domain-toggle"),
        &SetDomainEnabledPayload {
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
          Some("Run `cadder projects list`, then retry with the exact Caddyfile path.".to_string()),
        ),
      }
    })
  }

  fn find_entrypoint<'a>(
    snapshot: &'a GuiStateSnapshot,
    registration_id: &str,
  ) -> Option<&'a EntrypointRegistration> {
    snapshot
      .registrations
      .iter()
      .find(|entrypoint| entrypoint.registration_id == registration_id)
  }

  fn find_domain_in_entrypoint<'a>(
    entrypoint: &'a EntrypointRegistration,
    canonical_domain: &str,
  ) -> Option<&'a RegisteredDomain> {
    entrypoint
      .registered_domains
      .iter()
      .find(|domain| domain.name.canonical == canonical_domain)
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
mod tests;
