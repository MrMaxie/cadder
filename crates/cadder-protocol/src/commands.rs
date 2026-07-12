use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AutostartMode, EntrypointRegistration, HistoryKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterEntrypointRequest {
  pub request_id: String,
  pub registration: EntrypointRegistration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnregisterEntrypointRequest {
  pub request_id: String,
  pub registration_id: String,
  pub shim_session_nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
pub struct SubscribeStateRequest {
  pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetEntrypointEnabledRequest {
  pub request_id: String,
  pub registration_id: String,
  pub shim_session_nonce: Option<String>,
  pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetDomainEnabledRequest {
  pub request_id: String,
  pub registration_id: String,
  pub domain_key: String,
  pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryHistoryRequest {
  pub request_id: String,
  pub kind: Option<HistoryKind>,
  pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryAutostartRequest {
  pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetAutostartRequest {
  pub request_id: String,
  pub mode: AutostartMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShutdownDaemonRequest {
  pub request_id: String,
}

pub fn new_request_id(prefix: &str) -> String {
  format!("{prefix}-{}", Uuid::new_v4().simple())
}
