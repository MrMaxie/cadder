use crate::{
  error::CliError,
  view::{
    ActionResultView, DaemonStartView, DaemonStatusView, DiagnosticsView, DomainListView,
    EntrypointListView, LogsView,
  },
};
use chrono::Utc;
use serde::Serialize;
use std::io::{self, Write};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SuccessEnvelope<'a, T> {
  ok: bool,
  command: &'a str,
  generated_at_utc: chrono::DateTime<Utc>,
  data: &'a T,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEnvelope<'a> {
  ok: bool,
  command: &'a str,
  generated_at_utc: chrono::DateTime<Utc>,
  error: ErrorPayload<'a>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorPayload<'a> {
  kind: crate::error::CliErrorKind,
  exit_code: u8,
  message: &'a str,
  guidance: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchEnvelope<'a, T> {
  pub event: &'a str,
  pub command: &'a str,
  pub generated_at_utc: chrono::DateTime<Utc>,
  pub sequence_number: Option<u64>,
  pub change_kind: Option<String>,
  pub registration_id: Option<&'a str>,
  pub data: &'a T,
}

pub fn write_json_success<T: Serialize>(
  command: &'static str,
  data: &T,
  writer: &mut dyn Write,
) -> io::Result<()> {
  let envelope = SuccessEnvelope {
    ok: true,
    command,
    generated_at_utc: Utc::now(),
    data,
  };
  serde_json::to_writer_pretty(&mut *writer, &envelope)?;
  writeln!(writer)
}

pub fn write_json_error(error: &CliError, writer: &mut dyn Write) -> io::Result<()> {
  let envelope = ErrorEnvelope {
    ok: false,
    command: error.command,
    generated_at_utc: Utc::now(),
    error: ErrorPayload {
      kind: error.kind,
      exit_code: error.exit_code().code(),
      message: &error.message,
      guidance: error.guidance.as_deref(),
    },
  };
  serde_json::to_writer_pretty(&mut *writer, &envelope)?;
  writeln!(writer)
}

pub fn write_jsonl<T: Serialize>(value: &T, writer: &mut dyn Write) -> io::Result<()> {
  serde_json::to_writer(&mut *writer, value)?;
  writeln!(writer)
}

pub fn write_human_error(error: &CliError, writer: &mut dyn Write) -> io::Result<()> {
  writeln!(writer, "Error: {}", error.message)?;
  if let Some(guidance) = &error.guidance {
    writeln!(writer, "Guidance: {guidance}")?;
  }
  Ok(())
}

pub fn write_human_daemon_status(
  view: &DaemonStatusView,
  writer: &mut dyn Write,
) -> io::Result<()> {
  writeln!(writer, "Runtime directory: {}", view.runtime_dir)?;
  writeln!(writer, "Connection: {:?}", view.connection_state)?;
  writeln!(writer, "Message: {}", view.message)?;
  if let Some(guidance) = &view.guidance {
    writeln!(writer, "Guidance: {guidance}")?;
  }
  if let Some(captured) = view.captured_at_utc {
    writeln!(writer, "Captured at: {captured}")?;
  }
  writeln!(writer, "Entrypoints: {}", view.counts.entrypoints)?;
  writeln!(writer, "Domains: {}", view.counts.domains)?;
  writeln!(writer, "Active domains: {}", view.counts.active_domains)?;
  if let Some(runtime) = &view.runtime {
    writeln!(writer, "Runtime status: {}", runtime.status)?;
    if let Some(process_id) = runtime.process_id {
      writeln!(writer, "Runtime process ID: {process_id}")?;
    }
    if let Some(version) = &runtime.version {
      writeln!(writer, "Runtime version: {version}")?;
    }
  }
  if let Some(config) = &view.config {
    writeln!(writer, "Config status: {}", config.status)?;
  }
  Ok(())
}

pub fn write_human_daemon_start(view: &DaemonStartView, writer: &mut dyn Write) -> io::Result<()> {
  writeln!(writer, "{}", view.message)?;
  write_human_daemon_status(&view.status, writer)
}

pub fn write_human_action(view: &ActionResultView, writer: &mut dyn Write) -> io::Result<()> {
  writeln!(writer, "{}", view.message)
}

pub fn write_human_entrypoints(
  view: &EntrypointListView,
  writer: &mut dyn Write,
) -> io::Result<()> {
  writeln!(writer, "Captured at: {}", view.captured_at_utc)?;
  if view.entrypoints.is_empty() {
    return writeln!(writer, "No entrypoints registered.");
  }

  for entrypoint in &view.entrypoints {
    writeln!(writer, "Registration: {}", entrypoint.registration_id)?;
    writeln!(writer, "  Activation: {:?}", entrypoint.activation_state)?;
    writeln!(
      writer,
      "  Working directory: {}",
      entrypoint.working_directory
    )?;
    writeln!(writer, "  Config path: {}", entrypoint.config_path)?;
    writeln!(
      writer,
      "  Domains: {} total, {} active",
      entrypoint.domain_count, entrypoint.active_domain_count
    )?;
    writeln!(writer, "  Process ID: {}", entrypoint.process_id)?;
    if let Some(adapter) = &entrypoint.adapter {
      writeln!(writer, "  Adapter: {adapter}")?;
    }
    if let Some(command_line) = &entrypoint.command_line {
      writeln!(writer, "  Command line: {command_line}")?;
    }
  }
  Ok(())
}

pub fn write_human_domains(view: &DomainListView, writer: &mut dyn Write) -> io::Result<()> {
  writeln!(writer, "Captured at: {}", view.captured_at_utc)?;
  if view.domains.is_empty() {
    return writeln!(writer, "No domains registered.");
  }

  for domain in &view.domains {
    writeln!(writer, "Domain: {}", domain.domain)?;
    writeln!(writer, "  Canonical: {}", domain.canonical_domain)?;
    writeln!(writer, "  Registration: {}", domain.registration_id)?;
    writeln!(writer, "  Activation: {:?}", domain.activation_state)?;
    writeln!(
      writer,
      "  Entrypoint activation: {:?}",
      domain.entrypoint_activation_state
    )?;
    writeln!(writer, "  Working directory: {}", domain.working_directory)?;
    writeln!(writer, "  Config path: {}", domain.config_path)?;
  }
  Ok(())
}

pub fn write_human_diagnostics(view: &DiagnosticsView, writer: &mut dyn Write) -> io::Result<()> {
  writeln!(writer, "Captured at: {}", view.captured_at_utc)?;
  writeln!(writer, "Runtime status: {}", view.runtime.status)?;
  writeln!(writer, "Config status: {}", view.config.status)?;
  if view.runtime_diagnostics.is_empty() && view.config_diagnostics.is_empty() {
    return writeln!(writer, "No diagnostics.");
  }

  for diagnostic in &view.runtime_diagnostics {
    write_runtime_diagnostic(diagnostic, writer)?;
  }
  for diagnostic in &view.config_diagnostics {
    write_config_diagnostic(diagnostic, writer)?;
  }
  Ok(())
}

pub fn write_human_logs(view: &LogsView, writer: &mut dyn Write) -> io::Result<()> {
  writeln!(writer, "Stream: {}", view.stream.stream_id)?;
  writeln!(writer, "Status: {:?}", view.stream_status)?;
  if let Some(cursor) = &view.next_cursor {
    writeln!(writer, "Next cursor: {cursor}")?;
  }
  if view.has_gap {
    writeln!(
      writer,
      "Notice: retained log history has a gap before the newest entries."
    )?;
  }
  if view.truncated_by_retention {
    writeln!(
      writer,
      "Notice: older log entries were dropped by retention."
    )?;
  }
  if view.entries.is_empty() {
    return writeln!(writer, "No log entries matched the request.");
  }

  for entry in &view.entries {
    writeln!(
      writer,
      "[{}] {:?} {}",
      entry.timestamp_utc, entry.severity, entry.raw_message
    )?;
  }
  Ok(())
}

fn write_runtime_diagnostic(
  diagnostic: &cadder_protocol::RuntimeDiagnostic,
  writer: &mut dyn Write,
) -> io::Result<()> {
  writeln!(writer, "Runtime diagnostic: {}", diagnostic.code)?;
  writeln!(writer, "  Message: {}", diagnostic.message)?;
  if let Some(operation) = &diagnostic.operation {
    writeln!(writer, "  Operation: {operation}")?;
  }
  Ok(())
}

fn write_config_diagnostic(
  diagnostic: &cadder_protocol::ConfigDiagnostic,
  writer: &mut dyn Write,
) -> io::Result<()> {
  writeln!(writer, "Config diagnostic: {}", diagnostic.code)?;
  writeln!(writer, "  Message: {}", diagnostic.message)?;
  if let Some(domain) = &diagnostic.domain_key {
    writeln!(writer, "  Domain: {domain}")?;
  }
  if !diagnostic.source_config_paths.is_empty() {
    writeln!(
      writer,
      "  Source paths: {}",
      diagnostic.source_config_paths.join(", ")
    )?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    error::{CliError, CliErrorKind},
    view::{ConfigView, ConnectionStateView, CountsView, DomainView, EntrypointView, RuntimeView},
  };
  use cadder_protocol::{
    ActivationState, ConfigDiagnostic, LogAttributionKind, LogEntry, LogEntryKind, LogSeverity,
    LogStreamIdentity, LogStreamStatus, RuntimeDiagnostic,
  };
  use chrono::{TimeZone, Utc};
  use serde_json::Value;

  fn timestamp() -> chrono::DateTime<Utc> {
    Utc
      .with_ymd_and_hms(2026, 6, 16, 18, 45, 0)
      .single()
      .unwrap()
  }

  fn status_view() -> DaemonStatusView {
    DaemonStatusView {
      runtime_dir: "C:/runtime".to_string(),
      connection_state: ConnectionStateView::Connected,
      message: "Attached to cadderd.".to_string(),
      guidance: Some("No action required.".to_string()),
      captured_at_utc: Some(timestamp()),
      runtime: Some(RuntimeView {
        status: "Running".to_string(),
        binary_path: Some("caddy.exe".to_string()),
        version: Some("2.9.1".to_string()),
        process_id: Some(42),
        admin_endpoint: Some("http://127.0.0.1:2019".to_string()),
        diagnostics: Vec::new(),
      }),
      config: Some(ConfigView {
        status: "Applied".to_string(),
        last_attempted_at_utc: Some(timestamp()),
        last_successful_reload_at_utc: Some(timestamp()),
        effective_config_hash: Some("abc123".to_string()),
        diagnostics: Vec::new(),
      }),
      counts: CountsView {
        entrypoints: 2,
        domains: 4,
        active_domains: 3,
      },
    }
  }

  fn entrypoint_list() -> EntrypointListView {
    EntrypointListView {
      captured_at_utc: timestamp(),
      entrypoints: vec![EntrypointView {
        registration_id: "shim-1".to_string(),
        activation_state: ActivationState::Active,
        working_directory: "D:/Projects/App".to_string(),
        config_path: "D:/Projects/App/Caddyfile".to_string(),
        started_at_utc: timestamp(),
        last_heartbeat_utc: timestamp(),
        process_id: 17,
        executable_path: Some("caddy.exe".to_string()),
        domain_count: 2,
        active_domain_count: 1,
        adapter: Some("caddyfile".to_string()),
        command_line: Some("run --adapter caddyfile".to_string()),
      }],
    }
  }

  fn domain_list() -> DomainListView {
    DomainListView {
      captured_at_utc: timestamp(),
      domains: vec![DomainView {
        registration_id: "shim-1".to_string(),
        domain: "App.Localhost".to_string(),
        canonical_domain: "app.localhost".to_string(),
        activation_state: ActivationState::Inactive,
        entrypoint_activation_state: ActivationState::Active,
        working_directory: "D:/Projects/App".to_string(),
        config_path: "D:/Projects/App/Caddyfile".to_string(),
        log_stream: LogStreamIdentity::domain("app.localhost"),
      }],
    }
  }

  fn diagnostics_view_with_entries() -> DiagnosticsView {
    DiagnosticsView {
      captured_at_utc: timestamp(),
      runtime: RuntimeView {
        status: "Running".to_string(),
        binary_path: None,
        version: None,
        process_id: None,
        admin_endpoint: None,
        diagnostics: vec![RuntimeDiagnostic {
          code: "runtime-warning".to_string(),
          message: "Backend restart required".to_string(),
          operation: Some("reload".to_string()),
        }],
      },
      config: ConfigView {
        status: "Failed".to_string(),
        last_attempted_at_utc: Some(timestamp()),
        last_successful_reload_at_utc: None,
        effective_config_hash: None,
        diagnostics: vec![ConfigDiagnostic {
          code: "config-error".to_string(),
          message: "Duplicate host".to_string(),
          domain_key: Some("app.localhost".to_string()),
          source_config_paths: vec!["D:/Projects/App/Caddyfile".to_string()],
        }],
      },
      runtime_diagnostics: vec![RuntimeDiagnostic {
        code: "runtime-warning".to_string(),
        message: "Backend restart required".to_string(),
        operation: Some("reload".to_string()),
      }],
      config_diagnostics: vec![ConfigDiagnostic {
        code: "config-error".to_string(),
        message: "Duplicate host".to_string(),
        domain_key: Some("app.localhost".to_string()),
        source_config_paths: vec!["D:/Projects/App/Caddyfile".to_string()],
      }],
    }
  }

  fn logs_view_with_entry() -> LogsView {
    LogsView {
      stream: LogStreamIdentity::domain("app.localhost"),
      stream_status: LogStreamStatus::Active,
      entries: vec![LogEntry {
        sequence_number: 7,
        timestamp_utc: timestamp(),
        severity: LogSeverity::Error,
        stream: LogStreamIdentity::domain("app.localhost"),
        attribution_kind: LogAttributionKind::Domain,
        entry_kind: LogEntryKind::Normal,
        raw_message: "backend failed".to_string(),
        domain_key: Some("app.localhost".to_string()),
        source_registration_id: Some("shim-1".to_string()),
        source_instance_id: Some("instance-1".to_string()),
        operation: Some("reload".to_string()),
      }],
      next_cursor: Some("seq:7".to_string()),
      has_gap: true,
      has_more_before: false,
      truncated_by_retention: true,
    }
  }

  fn parse_json(text: &[u8]) -> Value {
    serde_json::from_slice(text).unwrap()
  }

  #[test]
  fn json_helpers_write_expected_envelopes() {
    let mut stdout = Vec::new();
    write_json_success("daemon status", &status_view(), &mut stdout).unwrap();
    let json = parse_json(&stdout);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "daemon status");
    assert_eq!(json["data"]["runtimeDir"], "C:/runtime");

    let error = CliError::new(
      "domains list",
      CliErrorKind::DaemonUnavailable,
      "daemon unavailable",
      Some("start the daemon".to_string()),
    );
    let mut stderr = Vec::new();
    write_json_error(&error, &mut stderr).unwrap();
    let json = parse_json(&stderr);
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["kind"], "daemonUnavailable");
    assert_eq!(json["error"]["exitCode"], 3);

    let mut jsonl = Vec::new();
    write_jsonl(&serde_json::json!({ "event": "tick" }), &mut jsonl).unwrap();
    assert_eq!(String::from_utf8(jsonl).unwrap(), "{\"event\":\"tick\"}\n");
  }

  #[test]
  fn human_status_and_actions_include_key_fields() {
    let error = CliError::new(
      "daemon start",
      CliErrorKind::DaemonStartFailure,
      "Could not start cadderd.",
      Some("Check the path.".to_string()),
    );
    let mut output = Vec::new();
    write_human_error(&error, &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Error: Could not start cadderd."));
    assert!(text.contains("Guidance: Check the path."));

    let mut output = Vec::new();
    write_human_daemon_status(&status_view(), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Runtime directory: C:/runtime"));
    assert!(text.contains("Connection: Connected"));
    assert!(text.contains("Runtime process ID: 42"));
    assert!(text.contains("Config status: Applied"));

    let start_view = DaemonStartView {
      started: false,
      message: "cadderd is already running.".to_string(),
      status: status_view(),
    };
    let mut output = Vec::new();
    write_human_daemon_start(&start_view, &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("cadderd is already running."));

    let mut output = Vec::new();
    write_human_action(
      &ActionResultView {
        message: "Domain activation updated.".to_string(),
      },
      &mut output,
    )
    .unwrap();
    assert_eq!(
      String::from_utf8(output).unwrap(),
      "Domain activation updated.\n"
    );
  }

  #[test]
  fn human_lists_cover_empty_and_populated_states() {
    let mut output = Vec::new();
    write_human_entrypoints(
      &EntrypointListView {
        captured_at_utc: timestamp(),
        entrypoints: Vec::new(),
      },
      &mut output,
    )
    .unwrap();
    assert!(
      String::from_utf8(output)
        .unwrap()
        .contains("No entrypoints registered.")
    );

    let mut output = Vec::new();
    write_human_entrypoints(&entrypoint_list(), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Registration: shim-1"));
    assert!(text.contains("Adapter: caddyfile"));
    assert!(text.contains("Command line: run --adapter caddyfile"));

    let mut output = Vec::new();
    write_human_domains(
      &DomainListView {
        captured_at_utc: timestamp(),
        domains: Vec::new(),
      },
      &mut output,
    )
    .unwrap();
    assert!(
      String::from_utf8(output)
        .unwrap()
        .contains("No domains registered.")
    );

    let mut output = Vec::new();
    write_human_domains(&domain_list(), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Domain: App.Localhost"));
    assert!(text.contains("Canonical: app.localhost"));
    assert!(text.contains("Entrypoint activation: Active"));
  }

  #[test]
  fn human_diagnostics_cover_empty_and_populated_states() {
    let empty = DiagnosticsView {
      captured_at_utc: timestamp(),
      runtime: RuntimeView {
        status: "Idle".to_string(),
        binary_path: None,
        version: None,
        process_id: None,
        admin_endpoint: None,
        diagnostics: Vec::new(),
      },
      config: ConfigView {
        status: "Idle".to_string(),
        last_attempted_at_utc: None,
        last_successful_reload_at_utc: None,
        effective_config_hash: None,
        diagnostics: Vec::new(),
      },
      runtime_diagnostics: Vec::new(),
      config_diagnostics: Vec::new(),
    };

    let mut output = Vec::new();
    write_human_diagnostics(&empty, &mut output).unwrap();
    assert!(
      String::from_utf8(output)
        .unwrap()
        .contains("No diagnostics.")
    );

    let mut output = Vec::new();
    write_human_diagnostics(&diagnostics_view_with_entries(), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Runtime diagnostic: runtime-warning"));
    assert!(text.contains("Operation: reload"));
    assert!(text.contains("Config diagnostic: config-error"));
    assert!(text.contains("Domain: app.localhost"));
    assert!(text.contains("Source paths: D:/Projects/App/Caddyfile"));
  }

  #[test]
  fn human_logs_cover_empty_and_populated_states() {
    let mut output = Vec::new();
    write_human_logs(
      &LogsView {
        stream: LogStreamIdentity::runtime_control(),
        stream_status: LogStreamStatus::Empty,
        entries: Vec::new(),
        next_cursor: Some("seq:0".to_string()),
        has_gap: true,
        has_more_before: false,
        truncated_by_retention: true,
      },
      &mut output,
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Stream: runtime-control"));
    assert!(text.contains("Notice: retained log history has a gap"));
    assert!(text.contains("Notice: older log entries were dropped by retention."));
    assert!(text.contains("No log entries matched the request."));

    let mut output = Vec::new();
    write_human_logs(&logs_view_with_entry(), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Next cursor: seq:7"));
    assert!(text.contains("backend failed"));
    assert!(text.contains("Error"));
  }
}
