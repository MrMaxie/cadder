use crate::cli::SeverityArg;
use cadder_protocol::{
  ActivationState, ConfigDiagnostic, ConfigState, EntrypointRegistration, GuiStateSnapshot,
  LogEntry, LogSeverity, LogStreamIdentity, LogStreamStatus, QueryLogsResponse, RuntimeDiagnostic,
  RuntimeState, canonicalize_domain,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionStateView {
  Connected,
  NotRunning,
  ConnectionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatusView {
  pub runtime_dir: String,
  pub connection_state: ConnectionStateView,
  pub message: String,
  pub guidance: Option<String>,
  pub captured_at_utc: Option<DateTime<Utc>>,
  pub runtime: Option<RuntimeView>,
  pub config: Option<ConfigView>,
  pub counts: CountsView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStartView {
  pub started: bool,
  pub message: String,
  pub status: DaemonStatusView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResultView {
  pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CountsView {
  pub entrypoints: usize,
  pub domains: usize,
  pub active_domains: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeView {
  pub status: String,
  pub binary_path: Option<String>,
  pub version: Option<String>,
  pub process_id: Option<u32>,
  pub admin_endpoint: Option<String>,
  pub diagnostics: Vec<RuntimeDiagnostic>,
}

impl From<&RuntimeState> for RuntimeView {
  fn from(value: &RuntimeState) -> Self {
    Self {
      status: format!("{:?}", value.status),
      binary_path: value.binary_path.clone(),
      version: value.version.clone(),
      process_id: value.process_id,
      admin_endpoint: value.admin_endpoint.clone(),
      diagnostics: value.diagnostics.clone(),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
  pub status: String,
  pub last_attempted_at_utc: Option<DateTime<Utc>>,
  pub last_successful_reload_at_utc: Option<DateTime<Utc>>,
  pub effective_config_hash: Option<String>,
  pub diagnostics: Vec<ConfigDiagnostic>,
}

impl From<&ConfigState> for ConfigView {
  fn from(value: &ConfigState) -> Self {
    Self {
      status: format!("{:?}", value.status),
      last_attempted_at_utc: value.last_attempted_at_utc,
      last_successful_reload_at_utc: value.last_successful_reload_at_utc,
      effective_config_hash: value.effective_config_hash.clone(),
      diagnostics: value.diagnostics.clone(),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrypointView {
  pub registration_id: String,
  pub activation_state: ActivationState,
  pub working_directory: String,
  pub config_path: String,
  pub started_at_utc: DateTime<Utc>,
  pub last_heartbeat_utc: DateTime<Utc>,
  pub process_id: u32,
  pub executable_path: Option<String>,
  pub domain_count: usize,
  pub active_domain_count: usize,
  pub adapter: Option<String>,
  pub command_line: Option<String>,
}

impl From<&EntrypointRegistration> for EntrypointView {
  fn from(value: &EntrypointRegistration) -> Self {
    Self {
      registration_id: value.registration_id.clone(),
      activation_state: value.activation_state,
      working_directory: value.source_working_directory.raw.clone(),
      config_path: value.source_config_path.raw.clone(),
      started_at_utc: value.entrypoint_instance.started_at_utc,
      last_heartbeat_utc: value.last_heartbeat_utc,
      process_id: value.owner_process.process_id,
      executable_path: value.owner_process.executable_path.clone(),
      domain_count: value.registered_domains.len(),
      active_domain_count: value
        .registered_domains
        .iter()
        .filter(|domain| domain.activation_state.is_enabled())
        .count(),
      adapter: value.shim_run.as_ref().and_then(|run| run.adapter.clone()),
      command_line: value.shim_run.as_ref().map(|run| run.command_line.clone()),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrypointListView {
  pub captured_at_utc: DateTime<Utc>,
  pub entrypoints: Vec<EntrypointView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainView {
  pub registration_id: String,
  pub domain: String,
  pub canonical_domain: String,
  pub activation_state: ActivationState,
  pub entrypoint_activation_state: ActivationState,
  pub working_directory: String,
  pub config_path: String,
  pub log_stream: LogStreamIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainListView {
  pub captured_at_utc: DateTime<Utc>,
  pub domains: Vec<DomainView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsView {
  pub captured_at_utc: DateTime<Utc>,
  pub runtime: RuntimeView,
  pub config: ConfigView,
  pub runtime_diagnostics: Vec<RuntimeDiagnostic>,
  pub config_diagnostics: Vec<ConfigDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsView {
  pub stream: LogStreamIdentity,
  pub stream_status: LogStreamStatus,
  pub entries: Vec<LogEntry>,
  pub next_cursor: Option<String>,
  pub has_gap: bool,
  pub has_more_before: bool,
  pub truncated_by_retention: bool,
}

impl From<QueryLogsResponse> for LogsView {
  fn from(value: QueryLogsResponse) -> Self {
    Self {
      stream: value.stream,
      stream_status: value.stream_status,
      entries: value.entries,
      next_cursor: value.next_cursor,
      has_gap: value.has_gap,
      has_more_before: value.has_more_before,
      truncated_by_retention: value.truncated_by_retention,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedDomain {
  pub registration_id: String,
  pub canonical_domain: String,
}

pub fn daemon_status_connected(
  runtime_dir: &Path,
  snapshot: &GuiStateSnapshot,
) -> DaemonStatusView {
  DaemonStatusView {
    runtime_dir: runtime_dir.display().to_string(),
    connection_state: ConnectionStateView::Connected,
    message: "Attached to cadderd.".to_string(),
    guidance: None,
    captured_at_utc: Some(snapshot.captured_at_utc),
    runtime: Some(RuntimeView::from(&snapshot.runtime)),
    config: Some(ConfigView::from(&snapshot.config)),
    counts: counts(snapshot),
  }
}

pub fn daemon_status_unavailable(
  runtime_dir: &Path,
  state: ConnectionStateView,
  message: String,
  guidance: Option<String>,
) -> DaemonStatusView {
  DaemonStatusView {
    runtime_dir: runtime_dir.display().to_string(),
    connection_state: state,
    message,
    guidance,
    captured_at_utc: None,
    runtime: None,
    config: None,
    counts: CountsView::default(),
  }
}

pub fn entrypoints_view(snapshot: &GuiStateSnapshot) -> EntrypointListView {
  EntrypointListView {
    captured_at_utc: snapshot.captured_at_utc,
    entrypoints: snapshot
      .registrations
      .iter()
      .map(EntrypointView::from)
      .collect(),
  }
}

pub fn domains_view(
  snapshot: &GuiStateSnapshot,
  registration_filter: Option<&str>,
) -> DomainListView {
  let domains = snapshot
    .registrations
    .iter()
    .filter(|registration| {
      registration_filter.is_none_or(|filter| registration.registration_id == filter)
    })
    .flat_map(|registration| {
      registration
        .registered_domains
        .iter()
        .map(move |domain| DomainView {
          registration_id: registration.registration_id.clone(),
          domain: domain.name.raw.clone(),
          canonical_domain: domain.name.canonical.clone(),
          activation_state: domain.activation_state,
          entrypoint_activation_state: registration.activation_state,
          working_directory: registration.source_working_directory.raw.clone(),
          config_path: registration.source_config_path.raw.clone(),
          log_stream: domain.log_stream.clone(),
        })
    })
    .collect();

  DomainListView {
    captured_at_utc: snapshot.captured_at_utc,
    domains,
  }
}

pub fn diagnostics_view(snapshot: &GuiStateSnapshot) -> DiagnosticsView {
  DiagnosticsView {
    captured_at_utc: snapshot.captured_at_utc,
    runtime: RuntimeView::from(&snapshot.runtime),
    config: ConfigView::from(&snapshot.config),
    runtime_diagnostics: snapshot.runtime.diagnostics.clone(),
    config_diagnostics: snapshot.config.diagnostics.clone(),
  }
}

pub fn counts(snapshot: &GuiStateSnapshot) -> CountsView {
  CountsView {
    entrypoints: snapshot.registrations.len(),
    domains: snapshot
      .registrations
      .iter()
      .map(|registration| registration.registered_domains.len())
      .sum(),
    active_domains: snapshot
      .registrations
      .iter()
      .flat_map(|registration| registration.registered_domains.iter())
      .filter(|domain| domain.activation_state.is_enabled())
      .count(),
  }
}

pub fn resolve_domain(
  snapshot: &GuiStateSnapshot,
  domain: &str,
  registration: Option<&str>,
) -> Result<SelectedDomain, DomainResolveError> {
  let canonical = canonicalize_domain(domain);
  let mut matches = Vec::new();
  for entrypoint in &snapshot.registrations {
    if registration.is_some_and(|registration_id| entrypoint.registration_id != registration_id) {
      continue;
    }

    if entrypoint
      .registered_domains
      .iter()
      .any(|candidate| candidate.name.canonical == canonical)
    {
      matches.push(SelectedDomain {
        registration_id: entrypoint.registration_id.clone(),
        canonical_domain: canonical.clone(),
      });
    }
  }

  match matches.as_slice() {
    [] => Err(DomainResolveError::NotFound {
      canonical_domain: canonical,
      requested_registration: registration.map(ToString::to_string),
    }),
    [selected] => Ok(selected.clone()),
    _ => Err(DomainResolveError::Ambiguous {
      canonical_domain: canonical,
      matches: matches
        .into_iter()
        .map(|entry| entry.registration_id)
        .collect(),
    }),
  }
}

pub fn map_severity(value: Option<SeverityArg>) -> Option<LogSeverity> {
  match value {
    None => None,
    Some(SeverityArg::Trace) => Some(LogSeverity::Trace),
    Some(SeverityArg::Debug) => Some(LogSeverity::Debug),
    Some(SeverityArg::Info) => Some(LogSeverity::Info),
    Some(SeverityArg::Warn) => Some(LogSeverity::Warn),
    Some(SeverityArg::Error) => Some(LogSeverity::Error),
    Some(SeverityArg::Fatal) => Some(LogSeverity::Fatal),
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainResolveError {
  NotFound {
    canonical_domain: String,
    requested_registration: Option<String>,
  },
  Ambiguous {
    canonical_domain: String,
    matches: Vec<String>,
  },
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_protocol::{
    DomainName, EntrypointInstanceIdentity, OwnerProcessIdentity, RegisteredDomain, SourcePath,
  };

  fn registration(id: &str, domains: &[&str]) -> EntrypointRegistration {
    let now = Utc::now();
    let identity = EntrypointInstanceIdentity {
      instance_id: id.to_string(),
      started_at_utc: now,
      shim_session_nonce: format!("{id}-nonce"),
    };
    EntrypointRegistration {
      registration_id: id.to_string(),
      entrypoint_instance: identity.clone(),
      source_working_directory: SourcePath::new("/work", None),
      source_config_path: SourcePath::new("/work/Caddyfile", None),
      registered_domains: domains
        .iter()
        .map(|domain| RegisteredDomain {
          name: DomainName::parse(*domain),
          activation_state: ActivationState::Active,
          log_stream: LogStreamIdentity::domain(domain),
        })
        .collect(),
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 1,
        process_start_time_utc: now,
        shim_session_nonce: identity.shim_session_nonce.clone(),
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint(id),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
    }
  }

  fn snapshot(registrations: Vec<EntrypointRegistration>) -> GuiStateSnapshot {
    GuiStateSnapshot {
      captured_at_utc: Utc::now(),
      registrations,
      runtime: RuntimeState::idle(),
      config: ConfigState::idle(),
    }
  }

  #[test]
  fn resolve_domain_finds_unique_match() {
    let snapshot = snapshot(vec![registration("shim-1", &["app.localhost"])]);

    let selected = resolve_domain(&snapshot, "APP.localhost", None).unwrap();

    assert_eq!(selected.registration_id, "shim-1");
    assert_eq!(selected.canonical_domain, "app.localhost");
  }

  #[test]
  fn resolve_domain_reports_ambiguity_without_registration() {
    let snapshot = snapshot(vec![
      registration("shim-1", &["app.localhost"]),
      registration("shim-2", &["app.localhost"]),
    ]);

    let error = resolve_domain(&snapshot, "app.localhost", None).unwrap_err();

    assert_eq!(
      error,
      DomainResolveError::Ambiguous {
        canonical_domain: "app.localhost".to_string(),
        matches: vec!["shim-1".to_string(), "shim-2".to_string()],
      }
    );
  }
}
