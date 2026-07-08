use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::{MIN_COMPATIBLE_PROTOCOL_VERSION, PROTOCOL_VERSION};

pub mod capabilities {
  pub const AUTOSTART: &str = "autostart";
  pub const HISTORY: &str = "history";
  pub const IIS_HANDOFF: &str = "iis-handoff";
  pub const LOGS: &str = "logs";
  pub const STATE_SUBSCRIPTION: &str = "state-subscription";
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolCapability {
  pub name: String,
  pub version: u16,
  pub minimum_compatible_version: u16,
}

impl ProtocolCapability {
  pub fn new(name: impl Into<String>, version: u16, minimum_compatible_version: u16) -> Self {
    Self {
      name: name.into(),
      version,
      minimum_compatible_version,
    }
  }

  pub fn supports_version(&self, required_version: u16) -> bool {
    (self.minimum_compatible_version..=self.version).contains(&required_version)
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolCapabilities {
  pub protocol_version: u16,
  pub minimum_compatible_protocol_version: u16,
  #[serde(default)]
  pub supported_capabilities: Box<[String]>,
  #[serde(default)]
  pub supported_capability_versions: Box<[ProtocolCapability]>,
}

impl ProtocolCapabilities {
  pub fn current() -> Self {
    Self {
      protocol_version: PROTOCOL_VERSION,
      minimum_compatible_protocol_version: MIN_COMPATIBLE_PROTOCOL_VERSION,
      supported_capabilities: current_capabilities(),
      supported_capability_versions: current_capability_versions(),
    }
  }

  pub fn supports(&self, capability: &str) -> bool {
    self
      .supported_capability_versions
      .iter()
      .any(|supported| supported.name == capability)
      || self
        .supported_capabilities
        .iter()
        .any(|supported| supported == capability)
  }

  pub fn require(&self, capability: impl Into<String>) -> ProtocolResult<()> {
    self.require_version(capability, 1)
  }

  pub fn supports_version(&self, capability: &str, required_version: u16) -> bool {
    if let Some(supported) = self
      .supported_capability_versions
      .iter()
      .find(|supported| supported.name == capability)
    {
      return supported.supports_version(required_version);
    }

    required_version == 1
      && self
        .supported_capabilities
        .iter()
        .any(|supported| supported == capability)
  }

  pub fn require_version(
    &self,
    capability: impl Into<String>,
    required_version: u16,
  ) -> ProtocolResult<()> {
    let capability = capability.into();
    if self.supports_version(&capability, required_version) {
      return Ok(());
    }

    Err(ProtocolError::unsupported_capability_version(
      capability,
      required_version,
      self.supported_capabilities.clone(),
      self.supported_capability_versions.clone(),
    ))
  }
}

pub fn current_capabilities() -> Box<[String]> {
  [
    capabilities::AUTOSTART,
    capabilities::HISTORY,
    capabilities::IIS_HANDOFF,
    capabilities::LOGS,
    capabilities::STATE_SUBSCRIPTION,
  ]
  .iter()
  .map(|capability| (*capability).to_string())
  .collect()
}

pub fn current_capability_versions() -> Box<[ProtocolCapability]> {
  [
    capabilities::AUTOSTART,
    capabilities::HISTORY,
    capabilities::IIS_HANDOFF,
    capabilities::LOGS,
    capabilities::STATE_SUBSCRIPTION,
  ]
  .into_iter()
  .map(|capability| ProtocolCapability::new(capability, 1, 1))
  .collect()
}

pub type ProtocolResult<T> = Result<T, ProtocolError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProtocolErrorKind {
  IncompatibleProtocolVersion,
  UnsupportedCapability,
  PayloadDecodeFailed,
  AccessDenied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolError {
  pub kind: ProtocolErrorKind,
  pub message: Box<str>,
  pub guidance: Option<Box<str>>,
  pub protocol_version: Option<u16>,
  pub minimum_compatible_protocol_version: Option<u16>,
  pub current_protocol_version: Option<u16>,
  pub required_capability: Option<Box<str>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub required_capability_version: Option<u16>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub denied_operation: Option<Box<str>>,
  #[serde(default)]
  pub supported_capabilities: Box<[String]>,
  #[serde(default)]
  pub supported_capability_versions: Box<[ProtocolCapability]>,
}

impl ProtocolError {
  pub fn incompatible_protocol_version(protocol_version: u16) -> Self {
    Self {
      kind: ProtocolErrorKind::IncompatibleProtocolVersion,
      message: format!(
        "unsupported Cadder IPC protocol version {protocol_version}; supported compatible range is {MIN_COMPATIBLE_PROTOCOL_VERSION}..={PROTOCOL_VERSION}"
      )
      .into_boxed_str(),
      guidance: Some(protocol_version_guidance(protocol_version).into_boxed_str()),
      protocol_version: Some(protocol_version),
      minimum_compatible_protocol_version: Some(MIN_COMPATIBLE_PROTOCOL_VERSION),
      current_protocol_version: Some(PROTOCOL_VERSION),
      required_capability: None,
      required_capability_version: None,
      denied_operation: None,
      supported_capabilities: current_capabilities(),
      supported_capability_versions: current_capability_versions(),
    }
  }

  pub fn unsupported_capability(
    required_capability: impl Into<String>,
    supported_capabilities: impl Into<Box<[String]>>,
  ) -> Self {
    let supported_capabilities = supported_capabilities.into();
    let supported_capability_versions = supported_capabilities
      .iter()
      .map(|capability| ProtocolCapability::new(capability.clone(), 1, 1))
      .collect::<Box<_>>();
    Self::unsupported_capability_version(
      required_capability,
      1,
      supported_capabilities,
      supported_capability_versions,
    )
  }

  pub fn unsupported_capability_version(
    required_capability: impl Into<String>,
    required_capability_version: u16,
    supported_capabilities: impl Into<Box<[String]>>,
    supported_capability_versions: impl Into<Box<[ProtocolCapability]>>,
  ) -> Self {
    let required_capability = required_capability.into();
    Self {
      kind: ProtocolErrorKind::UnsupportedCapability,
      message: format!(
        "unsupported Cadder protocol capability `{required_capability}` version {required_capability_version}"
      )
      .into_boxed_str(),
      guidance: Some(format!(
        "Use one of the advertised Cadder capabilities, or upgrade the older Cadder node to a build that supports `{required_capability}` version {required_capability_version}."
      )
      .into_boxed_str()),
      protocol_version: Some(PROTOCOL_VERSION),
      minimum_compatible_protocol_version: Some(MIN_COMPATIBLE_PROTOCOL_VERSION),
      current_protocol_version: Some(PROTOCOL_VERSION),
      required_capability: Some(required_capability.into_boxed_str()),
      required_capability_version: Some(required_capability_version),
      denied_operation: None,
      supported_capabilities: supported_capabilities.into(),
      supported_capability_versions: supported_capability_versions.into(),
    }
  }

  pub fn access_denied(
    operation: impl Into<String>,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self {
      kind: ProtocolErrorKind::AccessDenied,
      message: message.into().into_boxed_str(),
      guidance: guidance.map(String::into_boxed_str),
      protocol_version: Some(PROTOCOL_VERSION),
      minimum_compatible_protocol_version: Some(MIN_COMPATIBLE_PROTOCOL_VERSION),
      current_protocol_version: Some(PROTOCOL_VERSION),
      required_capability: None,
      required_capability_version: None,
      denied_operation: Some(operation.into().into_boxed_str()),
      supported_capabilities: current_capabilities(),
      supported_capability_versions: current_capability_versions(),
    }
  }

  pub fn payload_decode_failed(error: serde_json::Error) -> Self {
    Self {
      kind: ProtocolErrorKind::PayloadDecodeFailed,
      message: format!("could not decode Cadder protocol payload: {error}").into_boxed_str(),
      guidance: Some(
        "Verify that the request payload matches the Cadder IPC schema for this message type."
          .into(),
      ),
      protocol_version: None,
      minimum_compatible_protocol_version: Some(MIN_COMPATIBLE_PROTOCOL_VERSION),
      current_protocol_version: Some(PROTOCOL_VERSION),
      required_capability: None,
      required_capability_version: None,
      denied_operation: None,
      supported_capabilities: current_capabilities(),
      supported_capability_versions: current_capability_versions(),
    }
  }
}

impl fmt::Display for ProtocolError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.message)
  }
}

impl Error for ProtocolError {}

fn protocol_version_guidance(protocol_version: u16) -> String {
  if protocol_version < MIN_COMPATIBLE_PROTOCOL_VERSION {
    return format!(
      "Upgrade the Cadder client to protocol version {MIN_COMPATIBLE_PROTOCOL_VERSION} or newer."
    );
  }

  format!(
    "Upgrade cadderd to a build that supports protocol version {protocol_version}, or downgrade the client to protocol version {PROTOCOL_VERSION}."
  )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolErrorResponse {
  pub request_id: String,
  pub accepted: bool,
  pub error: ProtocolError,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub capabilities: Option<ProtocolCapabilities>,
}

impl ProtocolErrorResponse {
  pub fn rejected(request_id: impl Into<String>, error: ProtocolError) -> Self {
    Self {
      request_id: request_id.into(),
      accepted: false,
      error,
      capabilities: Some(ProtocolCapabilities::current()),
    }
  }
}
