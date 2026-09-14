use serde::{Deserialize, Serialize};

use crate::RuntimeDiagnostic;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StorageState {
  pub backend: String,
  pub path: Option<String>,
  pub schema_version: u32,
  pub diagnostics: Vec<RuntimeDiagnostic>,
}
