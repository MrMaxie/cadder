mod dto;
mod redaction;

use anyhow::Result;
use cadder_daemon::DaemonLaunchOptions;
use cadder_operator::{
  DomainSelector, OperatorContext, OperatorError, OperatorErrorKind, counts, diagnostics_view,
  domains_view, entrypoints_view,
};
use clap::Parser;
use dto::{
  ConfigDiagnosticOutput, ConfigSummaryOutput, DiagnosticsOutput, DomainListOutput, DomainOutput,
  EntrypointListOutput, EntrypointOutput, GetLogsParams, ListDomainsParams, ListEntrypointsParams,
  LogEntryOutput, LogTargetKind, LogsOutput, OverviewCountsOutput, OverviewOutput,
  RuntimeDiagnosticOutput, RuntimeSummaryOutput, SetDomainEnabledParams,
  SetEntrypointEnabledParams, SeverityParam, StartDaemonOutput, ToggleDomainOutput,
  ToggleEntrypointOutput, ToolErrorKind, ToolErrorPayload, ToolErrorResponse, TrustBoundaryOutput,
};
use redaction::{McpRedactor, truncate};
use rmcp::{
  ServerHandler, ServiceExt,
  handler::server::wrapper::{Json, Parameters},
  model::{CallToolResult, Implementation, ServerInfo},
  serde_json, tool, tool_handler, tool_router,
  transport::stdio,
};
use std::path::PathBuf;

const MAX_LOG_LIMIT: usize = 200;
const DEFAULT_LOG_LIMIT: usize = 50;

#[derive(Debug, Clone, Parser)]
#[command(
  name = "cadder-mcp",
  version,
  about = "Cadder local stdio MCP server for agent-safe inspection and bounded management"
)]
pub struct Args {
  #[arg(
    long,
    help = "Override the Cadder runtime directory used to find daemon IPC and state"
  )]
  pub runtime_dir: Option<PathBuf>,

  #[arg(
    long,
    help = "Path to a cadderd executable for the explicit daemon start tool"
  )]
  pub daemon_path: Option<PathBuf>,

  #[arg(
    long,
    help = "Command or path passed to cadderd when explicitly starting the real Caddy binary"
  )]
  pub real_caddy_command: Option<String>,
}

#[derive(Clone)]
pub struct CadderMcpServer {
  operator: OperatorContext,
  redactor: McpRedactor,
}

impl CadderMcpServer {
  pub fn new(args: Args) -> Result<Self> {
    let operator = OperatorContext::new(
      "cadder-mcp",
      args.runtime_dir,
      DaemonLaunchOptions {
        explicit_daemon: args.daemon_path,
        real_caddy_command: args.real_caddy_command,
        shim_path: None,
      },
    )?;
    let redactor = McpRedactor::new(
      std::env::current_dir().ok(),
      operator.paths().runtime_dir().to_path_buf(),
    );
    Ok(Self { operator, redactor })
  }

  pub async fn run_stdio(self) -> Result<()> {
    let service = self.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
  }

  fn map_error(&self, error: OperatorError) -> CallToolResult {
    let (kind, retryable, guidance) = match error.kind {
      OperatorErrorKind::InvalidUsage => (ToolErrorKind::InvalidUsage, false, None),
      OperatorErrorKind::DaemonUnavailable => (
        ToolErrorKind::DaemonUnavailable,
        true,
        Some("Call `cadder_start_daemon`, or start `cadderd` for the same runtime directory, then retry.".to_string()),
      ),
      OperatorErrorKind::DaemonStartFailure => (
        ToolErrorKind::DaemonStartFailure,
        true,
        Some("Fix the daemon path or startup configuration, then retry `cadder_start_daemon`.".to_string()),
      ),
      OperatorErrorKind::TargetNotFound => (ToolErrorKind::TargetNotFound, false, None),
      OperatorErrorKind::ConflictOrRejected => (
        ToolErrorKind::ConflictOrRejected,
        true,
        Some("Refresh Cadder state and retry with a more explicit selector if needed.".to_string()),
      ),
      OperatorErrorKind::PermissionOrElevation => (
        ToolErrorKind::PermissionOrElevation,
        true,
        Some("Check local IPC and runtime-directory permissions, then retry.".to_string()),
      ),
      OperatorErrorKind::UnsupportedOperation => (ToolErrorKind::UnsupportedOperation, false, None),
      OperatorErrorKind::IpcFailure => (
        ToolErrorKind::IpcFailure,
        true,
        Some("Retry after Cadder finishes its current transition or reconnect to the daemon.".to_string()),
      ),
    };

    self.error_response(
      kind,
      self.redactor.redact_text(&error.message),
      guidance,
      retryable,
    )
  }

  fn error_response(
    &self,
    kind: ToolErrorKind,
    message: String,
    guidance: Option<String>,
    retryable: bool,
  ) -> CallToolResult {
    let value = serde_json::to_value(ToolErrorResponse {
      ok: false,
      error: ToolErrorPayload {
        kind,
        message,
        guidance,
        retryable,
      },
    })
    .expect("tool error response serializes");
    CallToolResult::structured_error(value)
  }

  fn invalid_usage(&self, message: impl Into<String>, guidance: Option<String>) -> CallToolResult {
    self.error_response(
      ToolErrorKind::InvalidUsage,
      self.redactor.redact_text(&message.into()),
      guidance,
      false,
    )
  }

  fn build_overview(&self, snapshot: &cadder_protocol::GuiStateSnapshot) -> OverviewOutput {
    let counts_view = counts(snapshot);
    OverviewOutput {
      captured_at_utc: snapshot.captured_at_utc.to_rfc3339(),
      runtime_dir: self
        .redactor
        .redact_path(&self.operator.paths().runtime_dir().display().to_string()),
      trust_boundary: TrustBoundaryOutput {
        transport: "stdio".to_string(),
        exposure: "local-only".to_string(),
        daemon_model: "same-user per-runtime local daemon".to_string(),
        implicit_start: false,
        explicit_start_tool: "cadder_start_daemon".to_string(),
        preferred_surfaces: vec![
          "Use MCP for bounded inspect and toggle workflows.".to_string(),
          "Use cadderctl for watches, tails, and shell automation.".to_string(),
          "Use cadder-tui for interactive browsing and richer operator context.".to_string(),
        ],
      },
      counts: OverviewCountsOutput {
        entrypoints: counts_view.entrypoints,
        domains: counts_view.domains,
        active_domains: counts_view.active_domains,
        runtime_diagnostic_count: snapshot.runtime.diagnostics.len(),
        config_diagnostic_count: snapshot.config.diagnostics.len(),
      },
      runtime: RuntimeSummaryOutput {
        status: format!("{:?}", snapshot.runtime.status),
        version: snapshot.runtime.version.clone(),
        binary_path: snapshot
          .runtime
          .binary_path
          .as_deref()
          .map(|path| self.redactor.redact_path(path)),
        admin_endpoint: self
          .redactor
          .redact_endpoint(snapshot.runtime.admin_endpoint.as_deref()),
      },
      config: ConfigSummaryOutput {
        status: format!("{:?}", snapshot.config.status),
        effective_config_hash: snapshot.config.effective_config_hash.clone(),
        last_attempted_at_utc: snapshot
          .config
          .last_attempted_at_utc
          .map(|value| value.to_rfc3339()),
        last_successful_reload_at_utc: snapshot
          .config
          .last_successful_reload_at_utc
          .map(|value| value.to_rfc3339()),
      },
    }
  }

  fn build_entrypoints(
    &self,
    snapshot: &cadder_protocol::GuiStateSnapshot,
    registration_filter: Option<&str>,
  ) -> EntrypointListOutput {
    let view = entrypoints_view(snapshot);
    let entrypoints = view
      .entrypoints
      .into_iter()
      .filter(|entrypoint| {
        registration_filter.is_none_or(|filter| entrypoint.registration_id == filter)
      })
      .map(|entrypoint| {
        let domains = snapshot
          .registrations
          .iter()
          .find(|registration| registration.registration_id == entrypoint.registration_id)
          .map(|registration| {
            registration
              .registered_domains
              .iter()
              .map(|domain| domain.name.canonical.clone())
              .collect()
          })
          .unwrap_or_default();

        EntrypointOutput {
          registration_id: entrypoint.registration_id,
          activation_state: format!("{:?}", entrypoint.activation_state),
          working_directory: self.redactor.redact_path(&entrypoint.working_directory),
          config_path: self.redactor.redact_path(&entrypoint.config_path),
          started_at_utc: entrypoint.started_at_utc.to_rfc3339(),
          last_heartbeat_utc: entrypoint.last_heartbeat_utc.to_rfc3339(),
          executable_path: entrypoint
            .executable_path
            .as_deref()
            .map(|path| self.redactor.redact_path(path)),
          domain_count: entrypoint.domain_count,
          active_domain_count: entrypoint.active_domain_count,
          domains,
          adapter: entrypoint.adapter,
        }
      })
      .collect();

    EntrypointListOutput {
      captured_at_utc: view.captured_at_utc.to_rfc3339(),
      entrypoints,
    }
  }

  fn build_domains(&self, view: cadder_operator::DomainListView) -> DomainListOutput {
    DomainListOutput {
      captured_at_utc: view.captured_at_utc.to_rfc3339(),
      domains: view
        .domains
        .into_iter()
        .map(|domain| DomainOutput {
          registration_id: domain.registration_id,
          domain: domain.domain,
          canonical_domain: domain.canonical_domain,
          activation_state: format!("{:?}", domain.activation_state),
          entrypoint_activation_state: format!("{:?}", domain.entrypoint_activation_state),
          working_directory: self.redactor.redact_path(&domain.working_directory),
          config_path: self.redactor.redact_path(&domain.config_path),
        })
        .collect(),
    }
  }

  fn build_diagnostics(&self, snapshot: &cadder_protocol::GuiStateSnapshot) -> DiagnosticsOutput {
    let view = diagnostics_view(snapshot);
    DiagnosticsOutput {
      captured_at_utc: view.captured_at_utc.to_rfc3339(),
      runtime_status: view.runtime.status,
      config_status: view.config.status,
      runtime_diagnostics: view
        .runtime_diagnostics
        .into_iter()
        .map(|diagnostic| RuntimeDiagnosticOutput {
          code: diagnostic.code,
          message: self.redactor.redact_text(&diagnostic.message),
          operation: diagnostic
            .operation
            .as_deref()
            .map(|operation| truncate(&self.redactor.redact_text(operation), 120)),
        })
        .collect(),
      config_diagnostics: view
        .config_diagnostics
        .into_iter()
        .map(|diagnostic| ConfigDiagnosticOutput {
          code: diagnostic.code,
          message: self.redactor.redact_text(&diagnostic.message),
          domain_key: diagnostic.domain_key,
          source_config_paths: diagnostic
            .source_config_paths
            .iter()
            .map(|path| self.redactor.redact_path(path))
            .collect(),
        })
        .collect(),
    }
  }

  fn build_logs(&self, view: cadder_operator::LogsView) -> LogsOutput {
    LogsOutput {
      stream_id: view.stream.stream_id,
      channel: view.stream.channel,
      domain_key: view.stream.domain_key,
      stream_status: format!("{:?}", view.stream_status),
      entries: view
        .entries
        .into_iter()
        .map(|entry| LogEntryOutput {
          sequence_number: entry.sequence_number,
          timestamp_utc: entry.timestamp_utc.to_rfc3339(),
          severity: format!("{:?}", entry.severity),
          attribution_kind: format!("{:?}", entry.attribution_kind),
          entry_kind: format!("{:?}", entry.entry_kind),
          message: self.redactor.redact_text(&entry.raw_message),
          operation: entry
            .operation
            .as_deref()
            .map(|operation| truncate(&self.redactor.redact_text(operation), 120)),
          source_registration_id: entry.source_registration_id,
        })
        .collect(),
      next_cursor: view.next_cursor,
      has_gap: view.has_gap,
      has_more_before: view.has_more_before,
      truncated_by_retention: view.truncated_by_retention,
    }
  }
}

#[tool_router]
impl CadderMcpServer {
  #[tool(
    name = "cadder_get_overview",
    description = "Inspect local Cadder daemon health, runtime state, config summary, counts, and trust boundaries."
  )]
  async fn cadder_get_overview(&self) -> std::result::Result<Json<OverviewOutput>, CallToolResult> {
    let snapshot = self
      .operator
      .query_snapshot("cadder_get_overview", "query daemon overview")
      .await
      .map_err(|error| self.map_error(error))?;
    Ok(Json(self.build_overview(&snapshot)))
  }

  #[tool(
    name = "cadder_list_entrypoints",
    description = "List Cadder entrypoints with bounded, redacted metadata suitable for agent inspection."
  )]
  async fn cadder_list_entrypoints(
    &self,
    Parameters(params): Parameters<ListEntrypointsParams>,
  ) -> std::result::Result<Json<EntrypointListOutput>, CallToolResult> {
    let snapshot = self
      .operator
      .query_snapshot("cadder_list_entrypoints", "query entrypoints")
      .await
      .map_err(|error| self.map_error(error))?;
    Ok(Json(self.build_entrypoints(
      &snapshot,
      params.registration_id.as_deref(),
    )))
  }

  #[tool(
    name = "cadder_list_domains",
    description = "List Cadder domains and their activation state without exposing raw host-specific paths."
  )]
  async fn cadder_list_domains(
    &self,
    Parameters(params): Parameters<ListDomainsParams>,
  ) -> std::result::Result<Json<DomainListOutput>, CallToolResult> {
    let snapshot = self
      .operator
      .query_snapshot("cadder_list_domains", "query domains")
      .await
      .map_err(|error| self.map_error(error))?;
    let view = domains_view(&snapshot, params.registration_id.as_deref());
    Ok(Json(self.build_domains(view)))
  }

  #[tool(
    name = "cadder_show_diagnostics",
    description = "Show bounded Cadder runtime and config diagnostics with secret and path redaction."
  )]
  async fn cadder_show_diagnostics(
    &self,
  ) -> std::result::Result<Json<DiagnosticsOutput>, CallToolResult> {
    let snapshot = self
      .operator
      .query_snapshot("cadder_show_diagnostics", "query diagnostics")
      .await
      .map_err(|error| self.map_error(error))?;
    Ok(Json(self.build_diagnostics(&snapshot)))
  }

  #[tool(
    name = "cadder_get_logs",
    description = "Read recent Cadder logs for the runtime, one entrypoint, or one domain with bounded results and redacted text."
  )]
  async fn cadder_get_logs(
    &self,
    Parameters(params): Parameters<GetLogsParams>,
  ) -> std::result::Result<Json<LogsOutput>, CallToolResult> {
    let limit = params.limit.unwrap_or(DEFAULT_LOG_LIMIT);
    if !(1..=MAX_LOG_LIMIT).contains(&limit) {
      return Err(self.invalid_usage(
        format!("`limit` must be between 1 and {MAX_LOG_LIMIT}."),
        None,
      ));
    }

    if let Some(cursor) = params.cursor.as_deref() {
      let valid_cursor = cursor
        .strip_prefix("seq:")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()));
      if !valid_cursor {
        return Err(self.invalid_usage("Cursor must use the daemon `seq:<number>` format.", None));
      }
    }

    let target = match params.target {
      LogTargetKind::Runtime => {
        if params.registration_id.is_some() || params.domain.is_some() {
          return Err(self.invalid_usage(
            "Runtime log queries do not accept `registrationId` or `domain`.",
            None,
          ));
        }
        cadder_operator::LogsTarget::Runtime
      }
      LogTargetKind::Entrypoint => {
        let Some(registration_id) = params.registration_id else {
          return Err(self.invalid_usage("Entrypoint log queries require `registrationId`.", None));
        };
        if params.domain.is_some() {
          return Err(self.invalid_usage("Entrypoint log queries do not accept `domain`.", None));
        }
        cadder_operator::LogsTarget::Entrypoint { registration_id }
      }
      LogTargetKind::Domain => {
        let Some(domain) = params.domain else {
          return Err(self.invalid_usage("Domain log queries require `domain`.", None));
        };
        cadder_operator::LogsTarget::Domain(DomainSelector {
          domain,
          registration: params.registration_id,
        })
      }
    };

    let stream = self
      .operator
      .resolve_logs_target("cadder_get_logs", target)
      .await
      .map_err(|error| self.map_error(error))?;
    let view = self
      .operator
      .query_logs(
        "cadder_get_logs",
        "query retained logs",
        stream,
        limit,
        params.cursor,
        map_severity(params.minimum_severity),
      )
      .await
      .map_err(|error| self.map_error(error))?;
    Ok(Json(self.build_logs(view)))
  }

  #[tool(
    name = "cadder_start_daemon",
    description = "Explicitly start the local Cadder daemon for this runtime directory and return an updated overview."
  )]
  async fn cadder_start_daemon(
    &self,
  ) -> std::result::Result<Json<StartDaemonOutput>, CallToolResult> {
    let already_running = self.operator.query_state_response().await.is_ok();
    self
      .operator
      .ensure_daemon_running("cadder_start_daemon")
      .await
      .map_err(|error| self.map_error(error))?;
    let snapshot = self
      .operator
      .query_snapshot("cadder_start_daemon", "query daemon status after start")
      .await
      .map_err(|error| self.map_error(error))?;
    Ok(Json(StartDaemonOutput {
      started: !already_running,
      message: if already_running {
        "cadderd is already running.".to_string()
      } else {
        "cadderd started.".to_string()
      },
      overview: self.build_overview(&snapshot),
    }))
  }

  #[tool(
    name = "cadder_set_entrypoint_enabled",
    description = "Enable or disable one Cadder entrypoint by registration ID."
  )]
  async fn cadder_set_entrypoint_enabled(
    &self,
    Parameters(params): Parameters<SetEntrypointEnabledParams>,
  ) -> std::result::Result<Json<ToggleEntrypointOutput>, CallToolResult> {
    let response = self
      .operator
      .set_entrypoint_enabled(
        "cadder_set_entrypoint_enabled",
        params.registration_id.clone(),
        params.enabled,
      )
      .await
      .map_err(|error| self.map_error(error))?;
    Ok(Json(ToggleEntrypointOutput {
      registration_id: params.registration_id,
      enabled: params.enabled,
      message: self.redactor.redact_text(&response.message),
    }))
  }

  #[tool(
    name = "cadder_set_domain_enabled",
    description = "Enable or disable one Cadder domain with an optional registration filter for disambiguation."
  )]
  async fn cadder_set_domain_enabled(
    &self,
    Parameters(params): Parameters<SetDomainEnabledParams>,
  ) -> std::result::Result<Json<ToggleDomainOutput>, CallToolResult> {
    let response = self
      .operator
      .set_domain_enabled(
        "cadder_set_domain_enabled",
        &DomainSelector {
          domain: params.domain.clone(),
          registration: params.registration_id.clone(),
        },
        params.enabled,
      )
      .await
      .map_err(|error| self.map_error(error))?;
    Ok(Json(ToggleDomainOutput {
      canonical_domain: cadder_protocol::canonicalize_domain(&params.domain),
      registration_id: params.registration_id,
      enabled: params.enabled,
      message: self.redactor.redact_text(&response.message),
    }))
  }
}

#[tool_handler]
impl ServerHandler for CadderMcpServer {
  fn get_info(&self) -> ServerInfo {
    ServerInfo::default()
      .with_server_info(
        Implementation::new("cadder-mcp", env!("CARGO_PKG_VERSION"))
          .with_title("Cadder MCP")
          .with_description("Local stdio MCP server for safe Cadder inspection and bounded management."),
      )
      .with_instructions(
        "Cadder MCP is local-only over stdio. It talks to the same per-user Cadder daemon model as cadderctl. Only `cadder_start_daemon` may launch `cadderd`; all other tools are attach-only and return typed unavailable-daemon errors when the backend is absent. Prefer cadderctl for streaming watch/tail workflows and cadder-tui for interactive browsing."
      )
  }
}

fn map_severity(value: Option<SeverityParam>) -> Option<cadder_protocol::LogSeverity> {
  match value {
    None => None,
    Some(SeverityParam::Trace) => Some(cadder_protocol::LogSeverity::Trace),
    Some(SeverityParam::Debug) => Some(cadder_protocol::LogSeverity::Debug),
    Some(SeverityParam::Info) => Some(cadder_protocol::LogSeverity::Info),
    Some(SeverityParam::Warn) => Some(cadder_protocol::LogSeverity::Warn),
    Some(SeverityParam::Error) => Some(cadder_protocol::LogSeverity::Error),
    Some(SeverityParam::Fatal) => Some(cadder_protocol::LogSeverity::Fatal),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::redaction::McpRedactor;

  #[test]
  fn invalid_usage_response_marks_tool_error() {
    let server = CadderMcpServer {
      operator: OperatorContext::from_paths(
        cadder_daemon::RuntimePaths::resolve(Some(PathBuf::from("D:/runtime"))).unwrap(),
        DaemonLaunchOptions::default(),
      ),
      redactor: McpRedactor::new(None, PathBuf::from("D:/runtime")),
    };

    let error = server.invalid_usage("bad input", Some("fix it".to_string()));

    assert_eq!(error.is_error, Some(true));
    assert!(error.structured_content.is_some());
  }
}
