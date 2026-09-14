use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{OperationPayload, message_types, operation_payload_sealed};

/// Requests the current bounded daemon snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryStatePayload {}

impl operation_payload_sealed::Sealed for QueryStatePayload {}

impl OperationPayload for QueryStatePayload {
  const OPERATION: &'static str = message_types::QUERY_STATE_REQUEST;
}

pub fn new_request_id(prefix: &str) -> String {
  format!("{prefix}-{}", Uuid::new_v4().simple())
}
