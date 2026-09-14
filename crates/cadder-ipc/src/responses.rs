use serde::{Deserialize, Serialize};

use crate::GuiStateSnapshot;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegisterEntrypointResponse {
  #[serde(default)]
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
  pub registration_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BasicResponse {
  #[serde(default)]
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryStateResponse {
  #[serde(default)]
  pub request_id: String,
  pub accepted: bool,
  pub message: String,
  pub snapshot: Option<GuiStateSnapshot>,
}
