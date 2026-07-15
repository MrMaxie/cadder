use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::RuntimeDiagnostic;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StorageState {
  pub backend: String,
  pub path: Option<String>,
  pub schema_version: u32,
  pub diagnostics: Vec<RuntimeDiagnostic>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HistoryKind {
  Registration,
  Runtime,
  Config,
  Autostart,
  Log,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
  pub sequence_number: i64,
  pub timestamp_utc: DateTime<Utc>,
  pub kind: HistoryKind,
  pub summary: String,
  pub registration_id: Option<String>,
  pub domain_key: Option<String>,
  pub payload: Value,
}
