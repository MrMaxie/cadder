use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{LogStreamIdentity, StorageState};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ActivationState {
  Unknown,
  Registered,
  Activating,
  Active,
  Inactive,
  Faulted,
}

impl ActivationState {
  pub fn is_enabled(self) -> bool {
    matches!(self, Self::Registered | Self::Activating | Self::Active)
  }

  pub fn from_enabled(enabled: bool) -> Self {
    if enabled {
      Self::Active
    } else {
      Self::Inactive
    }
  }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeStatus {
  Unknown,
  NotResolved,
  Resolved,
  Running,
  Unhealthy,
  Idle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConfigApplyStatus {
  Unknown,
  NotApplied,
  Applied,
  Failed,
  Idle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourcePath {
  pub raw: String,
  pub canonical: Option<String>,
}

impl SourcePath {
  pub fn new(raw: impl Into<String>, canonical: Option<String>) -> Self {
    Self {
      raw: raw.into(),
      canonical,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DomainName {
  pub raw: String,
  pub canonical: String,
}

impl DomainName {
  pub fn parse(raw: impl Into<String>) -> Self {
    let raw = raw.into();
    Self {
      canonical: canonicalize_domain(&raw),
      raw,
    }
  }
}

pub fn canonicalize_domain(raw: &str) -> String {
  raw.trim().trim_end_matches('.').to_ascii_lowercase()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EntrypointInstanceIdentity {
  pub instance_id: String,
  pub started_at_utc: DateTime<Utc>,
  pub shim_session_nonce: String,
}

impl EntrypointInstanceIdentity {
  pub fn new(started_at_utc: DateTime<Utc>) -> Self {
    let id = format!("shim-{}", Uuid::new_v4().simple());
    Self {
      instance_id: id,
      started_at_utc,
      shim_session_nonce: Uuid::new_v4().simple().to_string(),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OwnerProcessIdentity {
  pub process_id: u32,
  pub process_start_time_utc: DateTime<Utc>,
  pub shim_session_nonce: String,
  pub executable_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredDomain {
  pub name: DomainName,
  pub activation_state: ActivationState,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub upstream: Option<String>,
  pub log_stream: LogStreamIdentity,
}

impl RegisteredDomain {
  pub fn active(raw: impl Into<String>) -> Self {
    let name = DomainName::parse(raw);
    Self {
      log_stream: LogStreamIdentity::domain(&name.canonical),
      name,
      activation_state: ActivationState::Active,
      upstream: None,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShimRunMetadata {
  pub adapter: Option<String>,
  pub raw_arguments: Vec<String>,
  pub command_line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EntrypointRegistration {
  pub registration_id: String,
  pub entrypoint_instance: EntrypointInstanceIdentity,
  pub source_working_directory: SourcePath,
  pub source_config_path: SourcePath,
  pub registered_domains: Vec<RegisteredDomain>,
  pub activation_state: ActivationState,
  pub owner_process: OwnerProcessIdentity,
  pub log_stream: LogStreamIdentity,
  pub shim_run: Option<ShimRunMetadata>,
  pub created_at_utc: DateTime<Utc>,
  pub last_heartbeat_utc: DateTime<Utc>,
}

impl EntrypointRegistration {
  pub fn validate_owner(&self) -> Result<(), String> {
    if self.registration_id.trim().is_empty() {
      return Err("registration_id is required".to_string());
    }
    if self.registration_id != self.entrypoint_instance.instance_id {
      return Err("registration_id must match instance_id".to_string());
    }
    if self
      .entrypoint_instance
      .shim_session_nonce
      .trim()
      .is_empty()
      || self.entrypoint_instance.shim_session_nonce != self.owner_process.shim_session_nonce
    {
      return Err("entrypoint and owner shim session nonce values must match".to_string());
    }
    Ok(())
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnostic {
  pub code: String,
  pub message: String,
  pub operation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
  pub status: RuntimeStatus,
  pub binary_path: Option<String>,
  pub version: Option<String>,
  pub process_id: Option<u32>,
  pub admin_endpoint: Option<String>,
  pub diagnostics: Vec<RuntimeDiagnostic>,
}

impl RuntimeState {
  pub fn idle() -> Self {
    Self {
      status: RuntimeStatus::Idle,
      binary_path: None,
      version: None,
      process_id: None,
      admin_endpoint: None,
      diagnostics: Vec::new(),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDiagnostic {
  pub code: String,
  pub message: String,
  pub domain_key: Option<String>,
  pub source_config_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigState {
  pub status: ConfigApplyStatus,
  pub last_attempted_at_utc: Option<DateTime<Utc>>,
  pub last_successful_reload_at_utc: Option<DateTime<Utc>>,
  pub effective_config_hash: Option<String>,
  pub diagnostics: Vec<ConfigDiagnostic>,
}

impl ConfigState {
  pub fn idle() -> Self {
    Self {
      status: ConfigApplyStatus::Idle,
      last_attempted_at_utc: None,
      last_successful_reload_at_utc: None,
      effective_config_hash: None,
      diagnostics: Vec::new(),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GuiStateSnapshot {
  pub captured_at_utc: DateTime<Utc>,
  pub registrations: Vec<EntrypointRegistration>,
  pub runtime: RuntimeState,
  pub config: ConfigState,
  pub storage: Option<StorageState>,
}
