use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;

pub mod message_types {
  pub const REGISTER_ENTRYPOINT_REQUEST: &str = "register-entrypoint-request";
  pub const REGISTER_ENTRYPOINT_RESPONSE: &str = "register-entrypoint-response";
  pub const UNREGISTER_ENTRYPOINT_REQUEST: &str = "unregister-entrypoint-request";
  pub const UNREGISTER_ENTRYPOINT_RESPONSE: &str = "unregister-entrypoint-response";
  pub const HEARTBEAT_ENTRYPOINT_REQUEST: &str = "heartbeat-entrypoint-request";
  pub const HEARTBEAT_ENTRYPOINT_RESPONSE: &str = "heartbeat-entrypoint-response";
  pub const QUERY_STATE_REQUEST: &str = "query-state-request";
  pub const QUERY_STATE_RESPONSE: &str = "query-state-response";
  pub const SUBSCRIBE_STATE_REQUEST: &str = "subscribe-state-request";
  pub const STATE_CHANGED_EVENT: &str = "state-changed-event";
  pub const SET_ENTRYPOINT_ENABLED_REQUEST: &str = "set-entrypoint-enabled-request";
  pub const SET_ENTRYPOINT_ENABLED_RESPONSE: &str = "set-entrypoint-enabled-response";
  pub const SET_DOMAIN_ENABLED_REQUEST: &str = "set-domain-enabled-request";
  pub const SET_DOMAIN_ENABLED_RESPONSE: &str = "set-domain-enabled-response";
  pub const QUERY_IIS_BINDINGS_REQUEST: &str = "query-iis-bindings-request";
  pub const QUERY_IIS_BINDINGS_RESPONSE: &str = "query-iis-bindings-response";
  pub const SET_IIS_HANDOFF_REQUEST: &str = "set-iis-handoff-request";
  pub const SET_IIS_HANDOFF_RESPONSE: &str = "set-iis-handoff-response";
  pub const QUERY_LOGS_REQUEST: &str = "query-logs-request";
  pub const QUERY_LOGS_RESPONSE: &str = "query-logs-response";
  pub const SHUTDOWN_DAEMON_REQUEST: &str = "shutdown-daemon-request";
  pub const SHUTDOWN_DAEMON_RESPONSE: &str = "shutdown-daemon-response";
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IpcEnvelope {
  pub protocol_version: u16,
  #[serde(rename = "type")]
  pub message_type: String,
  pub payload: Value,
}

impl IpcEnvelope {
  pub fn new<T: Serialize>(
    message_type: impl Into<String>,
    payload: &T,
  ) -> serde_json::Result<Self> {
    Ok(Self {
      protocol_version: PROTOCOL_VERSION,
      message_type: message_type.into(),
      payload: serde_json::to_value(payload)?,
    })
  }

  pub fn decode<T: DeserializeOwned>(&self) -> serde_json::Result<T> {
    serde_json::from_value(self.payload.clone())
  }
}

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
pub struct RegisteredDomain {
  pub name: DomainName,
  pub activation_state: ActivationState,
  pub log_stream: LogStreamIdentity,
}

impl RegisteredDomain {
  pub fn active(raw: impl Into<String>) -> Self {
    let name = DomainName::parse(raw);
    Self {
      log_stream: LogStreamIdentity::domain(&name.canonical),
      name,
      activation_state: ActivationState::Active,
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
pub struct GuiStateSnapshot {
  pub captured_at_utc: DateTime<Utc>,
  pub registrations: Vec<EntrypointRegistration>,
  pub runtime: RuntimeState,
  pub config: ConfigState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegisterEntrypointRequest {
  pub request_id: String,
  pub registration: EntrypointRegistration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegisterEntrypointResponse {
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
  pub registration_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnregisterEntrypointRequest {
  pub request_id: String,
  pub registration_id: String,
  pub shim_session_nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BasicResponse {
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatEntrypointRequest {
  pub request_id: String,
  pub registration_id: String,
  pub shim_session_nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryStateRequest {
  pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryStateResponse {
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
  pub snapshot: Option<GuiStateSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeStateRequest {
  pub request_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StateChangeKind {
  Snapshot,
  RegistrationsChanged,
  RuntimeChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StateChangedEvent {
  pub request_id: String,
  pub sequence_number: u64,
  pub change_kind: StateChangeKind,
  pub snapshot: GuiStateSnapshot,
  pub registration_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetEntrypointEnabledRequest {
  pub request_id: String,
  pub registration_id: String,
  pub shim_session_nonce: Option<String>,
  pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetDomainEnabledRequest {
  pub request_id: String,
  pub registration_id: String,
  pub domain_key: String,
  pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IisBindingIdentity {
  pub binding_id: String,
  pub site_name: String,
  pub protocol: String,
  pub binding_information: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IisHandoffState {
  Available,
  HandedOff,
  Unsupported,
  Conflict,
  MissingRoute,
  Unavailable,
  Busy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IisIssueKind {
  IisUnavailable,
  InsufficientPrivileges,
  ElevationRequired,
  ElevationDenied,
  ElevationUnsupported,
  UnsupportedBindingShape,
  MissingTlsCertificate,
  Conflict,
  MissingBinding,
  MissingRoute,
  RollbackSucceeded,
  RollbackFailed,
  RestoreFailed,
  Busy,
  ProviderError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IisIssue {
  pub kind: IisIssueKind,
  pub message: String,
}

impl IisIssue {
  pub fn new(kind: IisIssueKind, message: impl Into<String>) -> Self {
    Self {
      kind,
      message: message.into(),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IisRestoreMetadataSummary {
  pub site_name: String,
  pub protocol: String,
  pub ip_address: String,
  pub port: u16,
  pub host_header: String,
  pub binding_information: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IisPrivilegeLevel {
  User,
  Administrator,
  Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IisOperationStepStatus {
  Pending,
  Succeeded,
  RequiresElevation,
  Approved,
  Denied,
  Failed,
  Skipped,
  Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IisElevationApproval {
  NotRequired,
  Required,
  Approved,
  Denied,
  Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IisOperationStep {
  pub step_id: String,
  pub label: String,
  pub privilege_level: IisPrivilegeLevel,
  pub status: IisOperationStepStatus,
  pub approval: IisElevationApproval,
  pub issue: Option<IisIssue>,
}

impl IisOperationStep {
  pub fn user(step_id: impl Into<String>, label: impl Into<String>) -> Self {
    Self {
      step_id: step_id.into(),
      label: label.into(),
      privilege_level: IisPrivilegeLevel::User,
      status: IisOperationStepStatus::Pending,
      approval: IisElevationApproval::NotRequired,
      issue: None,
    }
  }

  pub fn administrator(step_id: impl Into<String>, label: impl Into<String>) -> Self {
    Self {
      step_id: step_id.into(),
      label: label.into(),
      privilege_level: IisPrivilegeLevel::Administrator,
      status: IisOperationStepStatus::RequiresElevation,
      approval: IisElevationApproval::Required,
      issue: None,
    }
  }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IisFollowUpAction {
  RetryElevation,
  RollbackHandoff,
  RetryRestore,
  RemoveLoopbackBinding,
  ClearRestoreMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IisBinding {
  pub identity: IisBindingIdentity,
  pub ip_address: String,
  pub port: u16,
  pub host_header: String,
  pub domain_key: Option<String>,
  pub handoff_state: IisHandoffState,
  pub issue: Option<IisIssue>,
  pub restore_metadata: Option<IisRestoreMetadataSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryIisBindingsRequest {
  pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryIisBindingsResponse {
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
  pub bindings: Vec<IisBinding>,
  pub issue: Option<IisIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetIisHandoffRequest {
  pub request_id: String,
  pub binding_id: String,
  pub enabled: bool,
  pub route_host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetIisHandoffResponse {
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
  pub binding: Option<IisBinding>,
  pub issue: Option<IisIssue>,
  pub steps: Vec<IisOperationStep>,
  pub follow_up_actions: Vec<IisFollowUpAction>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownDaemonRequest {
  pub request_id: String,
}

pub fn new_request_id(prefix: &str) -> String {
  format!("{prefix}-{}", Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn canonicalizes_domains() {
    assert_eq!(
      canonicalize_domain("  WWW.Example.Localhost. "),
      "www.example.localhost"
    );
  }

  #[test]
  fn serializes_envelope_with_camel_case_payload() {
    let request = QueryStateRequest {
      request_id: "request-1".to_string(),
    };
    let envelope = IpcEnvelope::new(message_types::QUERY_STATE_REQUEST, &request).unwrap();
    let json = serde_json::to_string(&envelope).unwrap();

    assert!(json.contains("\"protocolVersion\":1"));
    assert!(json.contains("\"type\":\"query-state-request\""));
    assert!(json.contains("\"requestId\":\"request-1\""));

    let decoded: QueryStateRequest = envelope.decode().unwrap();
    assert_eq!(decoded, request);
  }

  #[test]
  fn serializes_iis_handoff_contracts() {
    let response = QueryIisBindingsResponse {
      request_id: "iis-1".to_string(),
      accepted: true,
      message: "ok".to_string(),
      bindings: vec![IisBinding {
        identity: IisBindingIdentity {
          binding_id: "Default Web Site|http|*:80:app.localhost".to_string(),
          site_name: "Default Web Site".to_string(),
          protocol: "http".to_string(),
          binding_information: "*:80:app.localhost".to_string(),
        },
        ip_address: "*".to_string(),
        port: 80,
        host_header: "app.localhost".to_string(),
        domain_key: Some("app.localhost".to_string()),
        handoff_state: IisHandoffState::Available,
        issue: Some(IisIssue::new(
          IisIssueKind::MissingRoute,
          "Cadder has no route.",
        )),
        restore_metadata: None,
      }],
      issue: None,
    };

    let envelope = IpcEnvelope::new(message_types::QUERY_IIS_BINDINGS_RESPONSE, &response).unwrap();
    let json = serde_json::to_string(&envelope).unwrap();
    let decoded: QueryIisBindingsResponse = envelope.decode().unwrap();

    assert!(json.contains("\"query-iis-bindings-response\""));
    assert!(json.contains("\"handoffState\":\"available\""));
    assert!(json.contains("\"missingRoute\""));
    assert_eq!(decoded, response);

    let handoff_response = SetIisHandoffResponse {
      request_id: "iis-on".to_string(),
      accepted: false,
      message: "Administrator approval was denied.".to_string(),
      binding: None,
      issue: Some(IisIssue::new(
        IisIssueKind::ElevationDenied,
        "Administrator approval was denied.",
      )),
      steps: vec![IisOperationStep {
        step_id: "iis-remove-public-binding".to_string(),
        label: "Remove IIS public binding.".to_string(),
        privilege_level: IisPrivilegeLevel::Administrator,
        status: IisOperationStepStatus::Denied,
        approval: IisElevationApproval::Denied,
        issue: None,
      }],
      follow_up_actions: vec![IisFollowUpAction::RetryElevation],
    };
    let envelope =
      IpcEnvelope::new(message_types::SET_IIS_HANDOFF_RESPONSE, &handoff_response).unwrap();
    let json = serde_json::to_string(&envelope).unwrap();
    let decoded: SetIisHandoffResponse = envelope.decode().unwrap();

    assert!(json.contains("\"privilegeLevel\":\"administrator\""));
    assert!(json.contains("\"approval\":\"denied\""));
    assert!(json.contains("\"retryElevation\""));
    assert_eq!(decoded, handoff_response);

    let missing_tls = IisIssue::new(
      IisIssueKind::MissingTlsCertificate,
      "Missing certificate metadata.",
    );
    let json = serde_json::to_string(&missing_tls).unwrap();
    assert!(json.contains("\"kind\":\"missingTlsCertificate\""));

    let request = SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: "Default Web Site|https|*:443:".to_string(),
      enabled: true,
      route_host: Some("iis-app.localhost".to_string()),
    };
    let envelope = IpcEnvelope::new(message_types::SET_IIS_HANDOFF_REQUEST, &request).unwrap();
    let json = serde_json::to_string(&envelope).unwrap();
    let decoded: SetIisHandoffRequest = envelope.decode().unwrap();

    assert!(json.contains("\"set-iis-handoff-request\""));
    assert!(json.contains("\"routeHost\":\"iis-app.localhost\""));
    assert_eq!(decoded, request);
  }

  #[test]
  fn validates_owner_session_nonce() {
    let now = Utc::now();
    let identity = EntrypointInstanceIdentity::new(now);
    let registration = EntrypointRegistration {
      registration_id: identity.instance_id.clone(),
      source_working_directory: SourcePath::new(".", Some("/tmp/project".to_string())),
      source_config_path: SourcePath::new("Caddyfile", Some("/tmp/project/Caddyfile".to_string())),
      registered_domains: vec![RegisteredDomain::active("app.localhost")],
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 42,
        process_start_time_utc: now,
        shim_session_nonce: identity.shim_session_nonce.clone(),
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint(&identity.instance_id),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
      entrypoint_instance: identity,
    };

    assert_eq!(registration.validate_owner(), Ok(()));
  }
}
