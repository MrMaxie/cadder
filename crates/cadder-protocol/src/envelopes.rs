use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
  MIN_COMPATIBLE_PROTOCOL_VERSION, PROTOCOL_VERSION, ProtocolCapabilities, ProtocolError,
  ProtocolResult,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IpcEnvelope {
  pub protocol_version: u16,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub capabilities: Option<ProtocolCapabilities>,
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
      capabilities: Some(ProtocolCapabilities::current()),
      message_type: message_type.into(),
      payload: serde_json::to_value(payload)?,
    })
  }

  /// Decodes a legacy flat-envelope payload without reducing compatibility failures to text.
  pub fn decode<T: DeserializeOwned>(&self) -> ProtocolResult<T> {
    ensure_compatible_protocol_version(self.protocol_version)?;
    serde_json::from_value(self.payload.clone()).map_err(ProtocolError::payload_decode_failed)
  }
}

pub fn ensure_compatible_protocol_version(protocol_version: u16) -> ProtocolResult<()> {
  if protocol_version >= MIN_COMPATIBLE_PROTOCOL_VERSION {
    return Ok(());
  }

  Err(ProtocolError::incompatible_protocol_version(
    protocol_version,
  ))
}
