use std::{error::Error, fmt, ops::Deref};

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
  MIN_COMPATIBLE_PROTOCOL_VERSION, PROTOCOL_VERSION, ProtocolErrorCode, ProtocolVersionRange,
  RequestId,
};

pub mod capabilities {
  pub const ACTIVATION_CONTROL: &str = "activation-control";
  pub const AUTOSTART: &str = "autostart";
  pub const DAEMON_LIFECYCLE: &str = "daemon-lifecycle";
  pub const ENTRYPOINT_REGISTRATION: &str = "entrypoint-registration";
  pub const HISTORY: &str = "history";
  pub const IIS_HANDOFF: &str = "iis-handoff";
  pub const LOGS: &str = "logs";
  pub const RUNTIME_STATE: &str = "runtime-state";
  pub const STATE_SUBSCRIPTION: &str = "state-subscription";

  /// Every capability advertised by this build in stable wire order.
  pub const ALL: &[&str] = &[
    ACTIVATION_CONTROL,
    AUTOSTART,
    DAEMON_LIFECYCLE,
    ENTRYPOINT_REGISTRATION,
    HISTORY,
    IIS_HANDOFF,
    LOGS,
    RUNTIME_STATE,
    STATE_SUBSCRIPTION,
  ];
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
  capabilities::ALL
    .iter()
    .map(|capability| (*capability).to_string())
    .collect()
}

pub fn current_capability_versions() -> Box<[ProtocolCapability]> {
  capabilities::ALL
    .iter()
    .copied()
    .map(|capability| ProtocolCapability::new(capability, 1, 1))
    .collect()
}

pub type ProtocolResult<T> = Result<T, ProtocolError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// A stable category used to map protocol failures to client behavior.
pub enum ProtocolErrorKind {
  IncompatibleProtocolVersion,
  UnsupportedCapability,
  PayloadDecodeFailed,
  AccessDenied,
  InvalidInput,
  Conflict,
  Configuration,
  CaddyRuntime,
  Storage,
  Busy,
  Frame,
  Timeout,
  ProtocolViolation,
  ShuttingDown,
  StaleInstance,
  Internal,
}

impl ProtocolErrorKind {
  /// Returns the stable default machine code for this error category.
  pub const fn default_code(&self) -> &'static str {
    match self {
      Self::IncompatibleProtocolVersion => "incompatible_protocol",
      Self::UnsupportedCapability => "unsupported_capability",
      Self::PayloadDecodeFailed => "invalid_payload",
      Self::InvalidInput => "invalid_input",
      Self::AccessDenied => "permission_denied",
      Self::Conflict => "conflict",
      Self::Configuration => "configuration",
      Self::CaddyRuntime => "caddy_runtime",
      Self::Storage => "storage",
      Self::Busy => "busy",
      Self::Frame => "frame",
      Self::Timeout => "timeout",
      Self::ProtocolViolation => "protocol_violation",
      Self::ShuttingDown => "shutting_down",
      Self::StaleInstance => "stale_instance",
      Self::Internal => "internal",
    }
  }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(transparent)]
/// A compact owning typed error carried by Cadder protocol responses.
///
/// Operation and handshake envelopes add the request correlation before serialization.
pub struct ProtocolError(Box<ProtocolErrorData>);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Read-only fields exposed by [`ProtocolError`].
///
/// This separate allocation keeps `ProtocolResult<T>` small without changing the JSON object.
pub struct ProtocolErrorData {
  pub kind: ProtocolErrorKind,
  pub code: ProtocolErrorCode,
  pub message: Box<str>,
  pub guidance: Option<Box<str>>,
  pub retryable: bool,
  pub request_id: Option<RequestId>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub protocol_version: Option<u16>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub minimum_compatible_protocol_version: Option<u16>,
  #[serde(skip_serializing_if = "Option::is_none")]
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
  #[serde(skip)]
  legacy_version_metadata_present: bool,
}

impl ProtocolError {
  fn from_data(data: ProtocolErrorData) -> Self {
    Self(Box::new(data))
  }

  /// Creates an uncorrelated typed error for a handler or transport boundary.
  ///
  /// A response constructor attaches the validated request ID before the error reaches the wire.
  pub fn new(
    kind: ProtocolErrorKind,
    code: ProtocolErrorCode,
    message: impl Into<Box<str>>,
    guidance: Option<Box<str>>,
    retryable: bool,
  ) -> Self {
    Self::from_data(ProtocolErrorData {
      kind,
      code,
      message: message.into(),
      guidance,
      retryable,
      request_id: None,
      protocol_version: None,
      minimum_compatible_protocol_version: None,
      current_protocol_version: None,
      required_capability: None,
      required_capability_version: None,
      denied_operation: None,
      supported_capabilities: Box::default(),
      supported_capability_versions: Box::default(),
      legacy_version_metadata_present: false,
    })
  }

  pub fn incompatible_protocol_version(protocol_version: u16) -> Self {
    Self::from_data(ProtocolErrorData {
      kind: ProtocolErrorKind::IncompatibleProtocolVersion,
      code: ProtocolErrorCode::known("incompatible_protocol"),
      message: format!(
        "unsupported Cadder IPC protocol version {protocol_version}; supported compatible range is {MIN_COMPATIBLE_PROTOCOL_VERSION}..={PROTOCOL_VERSION}"
      )
      .into_boxed_str(),
      guidance: Some(protocol_version_guidance(protocol_version).into_boxed_str()),
      retryable: false,
      request_id: None,
      protocol_version: Some(protocol_version),
      minimum_compatible_protocol_version: Some(MIN_COMPATIBLE_PROTOCOL_VERSION),
      current_protocol_version: Some(PROTOCOL_VERSION),
      required_capability: None,
      required_capability_version: None,
      denied_operation: None,
      supported_capabilities: current_capabilities(),
      supported_capability_versions: current_capability_versions(),
      legacy_version_metadata_present: true,
    })
  }

  /// Creates a versioned incompatibility error for two non-overlapping ranges.
  pub fn incompatible_protocol_range(
    offered: ProtocolVersionRange,
    supported: ProtocolVersionRange,
  ) -> Self {
    let (message, older_component) = if offered.maximum().major() != supported.maximum().major() {
      (
        format!(
          "Cadder IPC protocol major {} is incompatible with supported major {}.",
          offered.maximum().major(),
          supported.maximum().major()
        ),
        if offered.maximum() < supported.minimum() {
          "client"
        } else {
          "daemon"
        },
      )
    } else {
      (
        format!(
          "Cadder IPC version range {}..={} does not overlap the supported range {}..={}.",
          offered.minimum(),
          offered.maximum(),
          supported.minimum(),
          supported.maximum()
        ),
        if offered.maximum() < supported.minimum() {
          "client"
        } else {
          "daemon"
        },
      )
    };
    Self::from_data(ProtocolErrorData {
      kind: ProtocolErrorKind::IncompatibleProtocolVersion,
      code: ProtocolErrorCode::known("incompatible_protocol"),
      message: message.into_boxed_str(),
      guidance: Some(
        format!("Upgrade the older Cadder {older_component}, then retry the connection.")
          .into_boxed_str(),
      ),
      retryable: false,
      request_id: None,
      protocol_version: None,
      minimum_compatible_protocol_version: None,
      current_protocol_version: None,
      required_capability: None,
      required_capability_version: None,
      denied_operation: None,
      supported_capabilities: current_capabilities(),
      supported_capability_versions: current_capability_versions(),
      legacy_version_metadata_present: false,
    })
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

  /// Creates the legacy-compatible diagnostic for an unknown message type.
  pub fn unsupported_operation(
    operation: impl Into<String>,
    supported_capabilities: impl Into<Box<[String]>>,
  ) -> Self {
    let operation = operation.into();
    let supported_capabilities = supported_capabilities.into();
    let supported_capability_versions = supported_capabilities
      .iter()
      .map(|capability| ProtocolCapability::new(capability.clone(), 1, 1))
      .collect();
    Self::from_data(ProtocolErrorData {
      kind: ProtocolErrorKind::UnsupportedCapability,
      code: ProtocolErrorCode::known("unsupported_operation"),
      message: format!("Cadder does not support the `{operation}` operation.").into_boxed_str(),
      guidance: Some("Use an operation advertised by the connected Cadder daemon.".into()),
      retryable: false,
      request_id: None,
      protocol_version: None,
      minimum_compatible_protocol_version: None,
      current_protocol_version: None,
      required_capability: Some(format!("message-type:{operation}").into_boxed_str()),
      required_capability_version: None,
      denied_operation: None,
      supported_capabilities,
      supported_capability_versions,
      legacy_version_metadata_present: false,
    })
  }

  pub fn unsupported_capability_version(
    required_capability: impl Into<String>,
    required_capability_version: u16,
    supported_capabilities: impl Into<Box<[String]>>,
    supported_capability_versions: impl Into<Box<[ProtocolCapability]>>,
  ) -> Self {
    let required_capability = required_capability.into();
    Self::from_data(ProtocolErrorData {
      kind: ProtocolErrorKind::UnsupportedCapability,
      code: ProtocolErrorCode::known("unsupported_capability"),
      message: format!(
        "unsupported Cadder protocol capability `{required_capability}` version {required_capability_version}"
      )
      .into_boxed_str(),
      guidance: Some(format!(
        "Use one of the advertised Cadder capabilities, or upgrade the older Cadder node to a build that supports `{required_capability}` version {required_capability_version}."
      )
      .into_boxed_str()),
      retryable: false,
      request_id: None,
      protocol_version: Some(PROTOCOL_VERSION),
      minimum_compatible_protocol_version: Some(MIN_COMPATIBLE_PROTOCOL_VERSION),
      current_protocol_version: Some(PROTOCOL_VERSION),
      required_capability: Some(required_capability.into_boxed_str()),
      required_capability_version: Some(required_capability_version),
      denied_operation: None,
      supported_capabilities: supported_capabilities.into(),
      supported_capability_versions: supported_capability_versions.into(),
      legacy_version_metadata_present: true,
    })
  }

  pub fn access_denied(
    operation: impl Into<String>,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::from_data(ProtocolErrorData {
      kind: ProtocolErrorKind::AccessDenied,
      code: ProtocolErrorCode::known("permission_denied"),
      message: message.into().into_boxed_str(),
      guidance: guidance.map(String::into_boxed_str),
      retryable: false,
      request_id: None,
      protocol_version: Some(PROTOCOL_VERSION),
      minimum_compatible_protocol_version: Some(MIN_COMPATIBLE_PROTOCOL_VERSION),
      current_protocol_version: Some(PROTOCOL_VERSION),
      required_capability: None,
      required_capability_version: None,
      denied_operation: Some(operation.into().into_boxed_str()),
      supported_capabilities: current_capabilities(),
      supported_capability_versions: current_capability_versions(),
      legacy_version_metadata_present: true,
    })
  }

  pub fn payload_decode_failed(error: serde_json::Error) -> Self {
    Self::from_data(ProtocolErrorData {
      kind: ProtocolErrorKind::PayloadDecodeFailed,
      code: ProtocolErrorCode::known("invalid_payload"),
      message: format!("could not decode Cadder protocol payload: {error}").into_boxed_str(),
      guidance: Some(
        "Verify that the request payload matches the Cadder IPC schema for this message type."
          .into(),
      ),
      retryable: false,
      request_id: None,
      protocol_version: None,
      minimum_compatible_protocol_version: Some(MIN_COMPATIBLE_PROTOCOL_VERSION),
      current_protocol_version: Some(PROTOCOL_VERSION),
      required_capability: None,
      required_capability_version: None,
      denied_operation: None,
      supported_capabilities: current_capabilities(),
      supported_capability_versions: current_capability_versions(),
      legacy_version_metadata_present: true,
    })
  }

  /// Reports that a closed mutation payload uses a contract this protocol session cannot decode.
  pub fn incompatible_payload_contract(path: Option<&str>) -> Self {
    let message = path.filter(|path| safe_payload_path(path)).map_or_else(
      || "The mutation payload does not match the negotiated Cadder protocol contract.".into(),
      |path| {
        format!(
          "The mutation payload uses `{path}`, which is not part of the negotiated Cadder protocol contract."
        )
      },
    );
    Self::from_data(ProtocolErrorData {
      kind: ProtocolErrorKind::IncompatibleProtocolVersion,
      code: ProtocolErrorCode::known("incompatible_payload"),
      message: message.into_boxed_str(),
      guidance: Some(
        "Upgrade the older Cadder component or use only fields and variants supported by the negotiated capabilities."
          .into(),
      ),
      retryable: false,
      request_id: None,
      protocol_version: None,
      minimum_compatible_protocol_version: None,
      current_protocol_version: None,
      required_capability: None,
      required_capability_version: None,
      denied_operation: None,
      supported_capabilities: current_capabilities(),
      supported_capability_versions: current_capability_versions(),
      legacy_version_metadata_present: false,
    })
  }

  pub(crate) fn decoder_contract_mismatch(operation: &str, payload_operation: &str) -> Self {
    Self::new(
      ProtocolErrorKind::Internal,
      ProtocolErrorCode::known("internal"),
      format!(
        "Cadder selected the `{payload_operation}` payload decoder for the `{operation}` operation."
      ),
      Some("Report this Cadder protocol dispatcher error.".into()),
      false,
    )
  }

  pub fn with_request_id(mut self, request_id: RequestId) -> Self {
    self.0.request_id = Some(request_id);
    self
  }

  pub(crate) fn for_versioned_response(mut self, request_id: RequestId) -> Self {
    self.0.request_id = Some(request_id);
    self.0.protocol_version = None;
    self.0.minimum_compatible_protocol_version = None;
    self.0.current_protocol_version = None;
    self.0.legacy_version_metadata_present = false;
    self
  }

  pub(crate) fn has_legacy_version_metadata(&self) -> bool {
    self.0.legacy_version_metadata_present
      || self.protocol_version.is_some()
      || self.minimum_compatible_protocol_version.is_some()
      || self.current_protocol_version.is_some()
  }
}

fn safe_payload_path(path: &str) -> bool {
  !path.is_empty()
    && path.len() <= 160
    && path
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'[' | b']'))
}

impl Deref for ProtocolError {
  type Target = ProtocolErrorData;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl<'de> Deserialize<'de> for ProtocolError {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let wire = ProtocolErrorWire::deserialize(deserializer)?;
    let (protocol_version, protocol_version_present) = wire.protocol_version.into_parts();
    let (minimum_compatible_protocol_version, minimum_version_present) =
      wire.minimum_compatible_protocol_version.into_parts();
    let (current_protocol_version, current_version_present) =
      wire.current_protocol_version.into_parts();
    Ok(Self::from_data(ProtocolErrorData {
      kind: wire.kind,
      code: wire.code,
      message: wire.message,
      guidance: wire.guidance,
      retryable: wire.retryable,
      request_id: wire.request_id,
      protocol_version: protocol_version.flatten(),
      minimum_compatible_protocol_version: minimum_compatible_protocol_version.flatten(),
      current_protocol_version: current_protocol_version.flatten(),
      required_capability: wire.required_capability,
      required_capability_version: wire.required_capability_version,
      denied_operation: wire.denied_operation,
      supported_capabilities: wire.supported_capabilities,
      supported_capability_versions: wire.supported_capability_versions,
      legacy_version_metadata_present: protocol_version_present
        || minimum_version_present
        || current_version_present,
    }))
  }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolErrorWire {
  kind: ProtocolErrorKind,
  code: ProtocolErrorCode,
  message: Box<str>,
  guidance: Option<Box<str>>,
  retryable: bool,
  #[serde(deserialize_with = "deserialize_required_request_id")]
  request_id: Option<RequestId>,
  #[serde(default)]
  protocol_version: WireField<Option<u16>>,
  #[serde(default)]
  minimum_compatible_protocol_version: WireField<Option<u16>>,
  #[serde(default)]
  current_protocol_version: WireField<Option<u16>>,
  required_capability: Option<Box<str>>,
  #[serde(default)]
  required_capability_version: Option<u16>,
  #[serde(default)]
  denied_operation: Option<Box<str>>,
  #[serde(default)]
  supported_capabilities: Box<[String]>,
  #[serde(default)]
  supported_capability_versions: Box<[ProtocolCapability]>,
}

#[derive(Default)]
enum WireField<T> {
  #[default]
  Missing,
  Present(T),
}

impl<T> WireField<T> {
  fn into_parts(self) -> (Option<T>, bool) {
    match self {
      Self::Missing => (None, false),
      Self::Present(value) => (Some(value), true),
    }
  }
}

impl<'de, T> Deserialize<'de> for WireField<T>
where
  T: Deserialize<'de>,
{
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    T::deserialize(deserializer).map(Self::Present)
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolErrorResponse {
  pub request_id: String,
  pub accepted: bool,
  pub error: ProtocolError,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub capabilities: Option<ProtocolCapabilities>,
}

impl ProtocolErrorResponse {
  pub fn rejected(request_id: Option<RequestId>, mut error: ProtocolError) -> Self {
    let response_request_id = request_id
      .as_ref()
      .map(ToString::to_string)
      .unwrap_or_else(|| "unknown".to_string());
    error.0.request_id = request_id;
    Self {
      request_id: response_request_id,
      accepted: false,
      error,
      capabilities: Some(ProtocolCapabilities::current()),
    }
  }
}

impl<'de> Deserialize<'de> for ProtocolErrorResponse {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WireResponse {
      request_id: String,
      accepted: bool,
      error: CompatibleProtocolError,
      #[serde(default)]
      capabilities: Option<ProtocolCapabilities>,
    }

    let response = WireResponse::deserialize(deserializer)?;
    if response.accepted {
      return Err(de::Error::custom(
        "a protocol error response cannot be accepted",
      ));
    }
    let request_id = if response.request_id == "unknown" {
      None
    } else {
      Some(RequestId::parse(response.request_id.clone()).map_err(de::Error::custom)?)
    };
    let error = response
      .error
      .into_current(request_id)
      .map_err(de::Error::custom)?;

    Ok(Self {
      request_id: response.request_id,
      accepted: false,
      error,
      capabilities: response.capabilities,
    })
  }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibleProtocolError {
  kind: ProtocolErrorKind,
  #[serde(default)]
  code: Option<ProtocolErrorCode>,
  message: Box<str>,
  guidance: Option<Box<str>>,
  #[serde(default)]
  retryable: Option<bool>,
  #[serde(default)]
  request_id: Option<RequestId>,
  protocol_version: Option<u16>,
  minimum_compatible_protocol_version: Option<u16>,
  current_protocol_version: Option<u16>,
  required_capability: Option<Box<str>>,
  #[serde(default)]
  required_capability_version: Option<u16>,
  #[serde(default)]
  denied_operation: Option<Box<str>>,
  #[serde(default)]
  supported_capabilities: Box<[String]>,
  #[serde(default)]
  supported_capability_versions: Box<[ProtocolCapability]>,
}

impl CompatibleProtocolError {
  fn into_current(
    self,
    outer_request_id: Option<RequestId>,
  ) -> Result<ProtocolError, &'static str> {
    let request_id = match (self.request_id, outer_request_id) {
      (Some(nested), Some(outer)) if nested == outer => Some(nested),
      (None, outer) => outer,
      _ => return Err("the response and protocol error request IDs must match"),
    };
    let legacy_version_metadata_present = self.protocol_version.is_some()
      || self.minimum_compatible_protocol_version.is_some()
      || self.current_protocol_version.is_some();
    Ok(ProtocolError::from_data(ProtocolErrorData {
      code: self
        .code
        .unwrap_or_else(|| ProtocolErrorCode::known(self.kind.default_code())),
      kind: self.kind,
      message: self.message,
      guidance: self.guidance,
      retryable: self.retryable.unwrap_or(false),
      request_id,
      protocol_version: self.protocol_version,
      minimum_compatible_protocol_version: self.minimum_compatible_protocol_version,
      current_protocol_version: self.current_protocol_version,
      required_capability: self.required_capability,
      required_capability_version: self.required_capability_version,
      denied_operation: self.denied_operation,
      supported_capabilities: self.supported_capabilities,
      supported_capability_versions: self.supported_capability_versions,
      legacy_version_metadata_present,
    }))
  }
}

fn deserialize_required_request_id<'de, D>(deserializer: D) -> Result<Option<RequestId>, D::Error>
where
  D: Deserializer<'de>,
{
  Option::<RequestId>::deserialize(deserializer)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn protocol_error_response_adapts_legacy_errors_with_outer_correlation() {
    let legacy = r#"{
      "requestId":"legacy-1",
      "accepted":false,
      "error":{
        "kind":"payloadDecodeFailed",
        "message":"Legacy daemon rejected the payload.",
        "guidance":"Check the payload.",
        "protocolVersion":2,
        "minimumCompatibleProtocolVersion":1,
        "currentProtocolVersion":2,
        "requiredCapability":null,
        "supportedCapabilities":[],
        "supportedCapabilityVersions":[]
      }
    }"#;

    let response: ProtocolErrorResponse = serde_json::from_str(legacy).unwrap();

    assert_eq!(response.error.code.as_ref(), "invalid_payload");
    assert_eq!(
      response.error.request_id.as_ref().map(RequestId::as_str),
      Some("legacy-1")
    );
    assert!(!response.error.retryable);
    assert_eq!(response.error.current_protocol_version, Some(2));
    let duplicate_message = legacy.replacen(
      "\"message\":\"Legacy daemon rejected the payload.\"",
      "\"message\":\"First value.\",\"message\":\"Legacy daemon rejected the payload.\"",
      1,
    );
    assert!(serde_json::from_str::<ProtocolErrorResponse>(&duplicate_message).is_err());

    let uncorrelated = ProtocolErrorResponse::rejected(
      None,
      ProtocolError::payload_decode_failed(
        serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
      ),
    );
    let decoded: ProtocolErrorResponse =
      serde_json::from_str(&serde_json::to_string(&uncorrelated).unwrap()).unwrap();
    assert_eq!(decoded.request_id, "unknown");
    assert_eq!(decoded.error.request_id, None);
  }
}
