use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListEntrypointsParams {
  pub registration_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDomainsParams {
  pub registration_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetLogsParams {
  pub target: LogTargetKind,
  pub registration_id: Option<String>,
  pub domain: Option<String>,
  pub limit: Option<usize>,
  pub cursor: Option<String>,
  pub minimum_severity: Option<SeverityParam>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetEntrypointEnabledParams {
  pub registration_id: String,
  pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetDomainEnabledParams {
  pub domain: String,
  pub registration_id: Option<String>,
  pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LogTargetKind {
  Runtime,
  Entrypoint,
  Domain,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SeverityParam {
  Trace,
  Debug,
  Info,
  Warn,
  Error,
  Fatal,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OverviewOutput {
  pub captured_at_utc: String,
  pub runtime_dir: String,
  pub trust_boundary: TrustBoundaryOutput,
  pub counts: OverviewCountsOutput,
  pub runtime: RuntimeSummaryOutput,
  pub config: ConfigSummaryOutput,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrustBoundaryOutput {
  pub transport: String,
  pub exposure: String,
  pub daemon_model: String,
  pub implicit_start: bool,
  pub explicit_start_tool: String,
  pub preferred_surfaces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OverviewCountsOutput {
  pub entrypoints: usize,
  pub domains: usize,
  pub active_domains: usize,
  pub runtime_diagnostic_count: usize,
  pub config_diagnostic_count: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSummaryOutput {
  pub status: String,
  pub version: Option<String>,
  pub binary_path: Option<String>,
  pub admin_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSummaryOutput {
  pub status: String,
  pub effective_config_hash: Option<String>,
  pub last_attempted_at_utc: Option<String>,
  pub last_successful_reload_at_utc: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntrypointListOutput {
  pub captured_at_utc: String,
  pub entrypoints: Vec<EntrypointOutput>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntrypointOutput {
  pub registration_id: String,
  pub activation_state: String,
  pub working_directory: String,
  pub config_path: String,
  pub started_at_utc: String,
  pub last_heartbeat_utc: String,
  pub executable_path: Option<String>,
  pub domain_count: usize,
  pub active_domain_count: usize,
  pub domains: Vec<String>,
  pub adapter: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DomainListOutput {
  pub captured_at_utc: String,
  pub domains: Vec<DomainOutput>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DomainOutput {
  pub registration_id: String,
  pub domain: String,
  pub canonical_domain: String,
  pub activation_state: String,
  pub entrypoint_activation_state: String,
  pub working_directory: String,
  pub config_path: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsOutput {
  pub captured_at_utc: String,
  pub runtime_status: String,
  pub config_status: String,
  pub runtime_diagnostics: Vec<RuntimeDiagnosticOutput>,
  pub config_diagnostics: Vec<ConfigDiagnosticOutput>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnosticOutput {
  pub code: String,
  pub message: String,
  pub operation: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDiagnosticOutput {
  pub code: String,
  pub message: String,
  pub domain_key: Option<String>,
  pub source_config_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogsOutput {
  pub stream_id: String,
  pub channel: String,
  pub domain_key: Option<String>,
  pub stream_status: String,
  pub entries: Vec<LogEntryOutput>,
  pub next_cursor: Option<String>,
  pub has_gap: bool,
  pub has_more_before: bool,
  pub truncated_by_retention: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogEntryOutput {
  pub sequence_number: u64,
  pub timestamp_utc: String,
  pub severity: String,
  pub attribution_kind: String,
  pub entry_kind: String,
  pub message: String,
  pub operation: Option<String>,
  pub source_registration_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartDaemonOutput {
  pub started: bool,
  pub message: String,
  pub overview: OverviewOutput,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToggleEntrypointOutput {
  pub registration_id: String,
  pub enabled: bool,
  pub message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToggleDomainOutput {
  pub canonical_domain: String,
  pub registration_id: Option<String>,
  pub enabled: bool,
  pub message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolErrorResponse {
  pub ok: bool,
  pub error: ToolErrorPayload,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolErrorPayload {
  pub kind: ToolErrorKind,
  pub message: String,
  pub guidance: Option<String>,
  pub retryable: bool,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ToolErrorKind {
  InvalidUsage,
  DaemonUnavailable,
  DaemonStartFailure,
  TargetNotFound,
  ConflictOrRejected,
  PermissionOrElevation,
  UnsupportedOperation,
  IpcFailure,
}
