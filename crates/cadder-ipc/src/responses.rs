use serde::{Deserialize, Serialize};

use crate::{
  AutostartDiagnostic, AutostartMode, AutostartStatus, GuiStateSnapshot, HistoryRecord,
  StorageState,
};

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
pub struct BasicResponse {
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
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
pub struct QueryHistoryResponse {
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
  pub records: Vec<HistoryRecord>,
  pub storage: Option<StorageState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryAutostartResponse {
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
  pub mode: AutostartMode,
  pub status: AutostartStatus,
  pub target: Option<String>,
  pub diagnostics: Vec<AutostartDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetAutostartResponse {
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
  pub mode: AutostartMode,
  pub status: AutostartStatus,
  pub target: Option<String>,
  pub diagnostics: Vec<AutostartDiagnostic>,
}
