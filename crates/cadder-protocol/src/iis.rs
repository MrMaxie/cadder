use serde::{Deserialize, Serialize};

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
