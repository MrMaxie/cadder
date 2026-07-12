use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{error::Error, fmt, str::FromStr};

const MAX_CAPABILITY_ID_BYTES: usize = 64;
const MAX_ERROR_CODE_BYTES: usize = 64;
const MAX_REQUEST_ID_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Explains why a protocol identifier was rejected.
pub struct IdentifierError {
  kind: &'static str,
  reason: &'static str,
}

impl IdentifierError {
  fn new(kind: &'static str, reason: &'static str) -> Self {
    Self { kind, reason }
  }
}

impl fmt::Display for IdentifierError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(formatter, "invalid {}: {}", self.kind, self.reason)
  }
}

impl Error for IdentifierError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// A validated capability name such as `state-subscription`.
///
/// Capability IDs use lowercase ASCII letters, digits, and internal hyphens and are at most 64
/// bytes long.
pub struct CapabilityId(Box<str>);

impl CapabilityId {
  /// Validates and owns a capability ID.
  pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
    let value = value.into();
    validate_capability_id(&value)?;
    Ok(Self(value.into_boxed_str()))
  }

  /// Returns the validated wire value.
  pub fn as_str(&self) -> &str {
    &self.0
  }

  pub(crate) fn known(value: &'static str) -> Self {
    debug_assert!(validate_capability_id(value).is_ok());
    Self(value.into())
  }
}

impl AsRef<str> for CapabilityId {
  fn as_ref(&self) -> &str {
    self.as_str()
  }
}

impl fmt::Display for CapabilityId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for CapabilityId {
  type Err = IdentifierError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    Self::parse(value)
  }
}

impl TryFrom<String> for CapabilityId {
  type Error = IdentifierError;

  fn try_from(value: String) -> Result<Self, Self::Error> {
    Self::parse(value)
  }
}

impl Serialize for CapabilityId {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(self.as_str())
  }
}

impl<'de> Deserialize<'de> for CapabilityId {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let value = String::deserialize(deserializer)?;
    Self::parse(value).map_err(de::Error::custom)
  }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// A validated stable machine code carried by [`crate::ProtocolError`].
///
/// Error codes use lowercase ASCII letters, digits, and internal underscores and are at most 64
/// bytes long.
pub struct ProtocolErrorCode(Box<str>);

impl ProtocolErrorCode {
  /// Validates and owns a protocol error code.
  pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
    let value = value.into();
    validate_error_code(&value)?;
    Ok(Self(value.into_boxed_str()))
  }

  /// Returns the validated wire value.
  pub fn as_str(&self) -> &str {
    &self.0
  }

  pub(crate) fn known(value: &'static str) -> Self {
    debug_assert!(validate_error_code(value).is_ok());
    Self(value.into())
  }
}

impl AsRef<str> for ProtocolErrorCode {
  fn as_ref(&self) -> &str {
    self.as_str()
  }
}

impl fmt::Display for ProtocolErrorCode {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for ProtocolErrorCode {
  type Err = IdentifierError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    Self::parse(value)
  }
}

impl TryFrom<String> for ProtocolErrorCode {
  type Error = IdentifierError;

  fn try_from(value: String) -> Result<Self, Self::Error> {
    Self::parse(value)
  }
}

impl Serialize for ProtocolErrorCode {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(self.as_str())
  }
}

impl<'de> Deserialize<'de> for ProtocolErrorCode {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let value = String::deserialize(deserializer)?;
    Self::parse(value).map_err(de::Error::custom)
  }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// A validated request correlation ID.
///
/// IDs are ASCII, at most 128 bytes long, and cannot use the legacy `unknown` sentinel.
pub struct RequestId(Box<str>);

impl RequestId {
  /// Validates and owns a request ID.
  pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
    let value = value.into();
    validate_request_id(&value)?;
    Ok(Self(value.into_boxed_str()))
  }

  /// Returns the validated wire value.
  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl AsRef<str> for RequestId {
  fn as_ref(&self) -> &str {
    self.as_str()
  }
}

impl fmt::Display for RequestId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for RequestId {
  type Err = IdentifierError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    Self::parse(value)
  }
}

impl TryFrom<String> for RequestId {
  type Error = IdentifierError;

  fn try_from(value: String) -> Result<Self, Self::Error> {
    Self::parse(value)
  }
}

impl From<RequestId> for String {
  fn from(value: RequestId) -> Self {
    value.0.into()
  }
}

impl Serialize for RequestId {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(self.as_str())
  }
}

impl<'de> Deserialize<'de> for RequestId {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let value = String::deserialize(deserializer)?;
    Self::parse(value).map_err(de::Error::custom)
  }
}

fn validate_capability_id(value: &str) -> Result<(), IdentifierError> {
  if value.is_empty() || value.len() > MAX_CAPABILITY_ID_BYTES {
    return Err(IdentifierError::new(
      "capability ID",
      "length must be between 1 and 64 bytes",
    ));
  }
  let bytes = value.as_bytes();
  if !bytes[0].is_ascii_lowercase() || !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
    return Err(IdentifierError::new(
      "capability ID",
      "must start with a lowercase letter and end with a lowercase letter or digit",
    ));
  }
  if bytes
    .iter()
    .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-'))
  {
    return Err(IdentifierError::new(
      "capability ID",
      "may contain only lowercase ASCII letters, digits, and hyphens",
    ));
  }
  Ok(())
}

fn validate_request_id(value: &str) -> Result<(), IdentifierError> {
  if value.is_empty() || value.len() > MAX_REQUEST_ID_BYTES {
    return Err(IdentifierError::new(
      "request ID",
      "length must be between 1 and 128 bytes",
    ));
  }
  if value == "unknown" {
    return Err(IdentifierError::new(
      "request ID",
      "`unknown` is reserved for uncorrelated legacy responses",
    ));
  }
  if !value.as_bytes()[0].is_ascii_alphanumeric()
    || value
      .as_bytes()
      .iter()
      .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b':')))
  {
    return Err(IdentifierError::new(
      "request ID",
      "must start with an ASCII letter or digit and contain only ASCII letters, digits, '-', '_', '.', or ':'",
    ));
  }
  Ok(())
}

fn validate_error_code(value: &str) -> Result<(), IdentifierError> {
  if value.is_empty() || value.len() > MAX_ERROR_CODE_BYTES {
    return Err(IdentifierError::new(
      "protocol error code",
      "length must be between 1 and 64 bytes",
    ));
  }
  let bytes = value.as_bytes();
  if !bytes[0].is_ascii_lowercase() || !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
    return Err(IdentifierError::new(
      "protocol error code",
      "must start with a lowercase letter and end with a lowercase letter or digit",
    ));
  }
  if bytes
    .iter()
    .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_'))
  {
    return Err(IdentifierError::new(
      "protocol error code",
      "may contain only lowercase ASCII letters, digits, and underscores",
    ));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn protocol_identifiers_validate_text_and_serde_input() {
    let capability = CapabilityId::parse("state-subscription").unwrap();
    let error_code = ProtocolErrorCode::parse("incompatible_protocol").unwrap();
    let request = RequestId::parse("request:01.test_value").unwrap();

    assert_eq!(
      serde_json::to_string(&capability).unwrap(),
      "\"state-subscription\""
    );
    assert_eq!(
      serde_json::to_string(&request).unwrap(),
      "\"request:01.test_value\""
    );
    assert_eq!(
      serde_json::to_string(&error_code).unwrap(),
      "\"incompatible_protocol\""
    );
    assert!(CapabilityId::parse("State").is_err());
    assert!(CapabilityId::parse("state-").is_err());
    assert!(RequestId::parse("request value").is_err());
    assert!(RequestId::parse("unknown").is_err());
    assert!(ProtocolErrorCode::parse("Invalid-Code").is_err());
    assert!(serde_json::from_str::<CapabilityId>("\"bad/value\"").is_err());
    assert!(serde_json::from_str::<RequestId>("\"\"").is_err());
    assert!(serde_json::from_str::<ProtocolErrorCode>("\"bad-code\"").is_err());
  }
}
