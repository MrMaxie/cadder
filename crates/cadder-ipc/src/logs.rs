use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum LogSeverity {
  Unknown,
  Trace,
  Debug,
  Info,
  Warn,
  Error,
  Fatal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LogAttributionKind {
  Unknown,
  Runtime,
  RuntimeControl,
  Entrypoint,
  Domain,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LogEntryKind {
  Normal,
  Lifecycle,
  IngestionOverflow,
  RetentionGap,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LogStreamStatus {
  Unknown,
  Empty,
  Active,
  Stale,
  Removed,
  ReadError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogStreamIdentity {
  pub stream_id: String,
  pub domain_key: Option<String>,
  pub channel: String,
}

impl LogStreamIdentity {
  pub fn runtime_control() -> Self {
    Self {
      stream_id: "runtime-control".to_string(),
      domain_key: None,
      channel: "control".to_string(),
    }
  }

  pub fn entrypoint(registration_id: &str) -> Self {
    Self {
      stream_id: format!("entrypoint-{registration_id}"),
      domain_key: None,
      channel: "caddy".to_string(),
    }
  }

  pub fn domain(domain_key: &str) -> Self {
    Self {
      stream_id: format!("domain-{domain_key}"),
      domain_key: Some(domain_key.to_string()),
      channel: "caddy".to_string(),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
  pub sequence_number: u64,
  pub timestamp_utc: DateTime<Utc>,
  pub severity: LogSeverity,
  pub stream: LogStreamIdentity,
  pub attribution_kind: LogAttributionKind,
  pub entry_kind: LogEntryKind,
  pub raw_message: String,
  pub domain_key: Option<String>,
  pub source_registration_id: Option<String>,
  pub source_instance_id: Option<String>,
  pub operation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryLogsRequest {
  pub request_id: String,
  pub stream: LogStreamIdentity,
  pub limit: Option<usize>,
  pub cursor: Option<String>,
  pub minimum_severity: Option<LogSeverity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryLogsResponse {
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
  pub stream: LogStreamIdentity,
  pub stream_status: LogStreamStatus,
  pub entries: Vec<LogEntry>,
  pub next_cursor: Option<String>,
  pub has_gap: bool,
  pub has_more_before: bool,
  pub truncated_by_retention: bool,
}
