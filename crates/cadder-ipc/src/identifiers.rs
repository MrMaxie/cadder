use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{error::Error, fmt, str::FromStr};

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
    let error_code = ProtocolErrorCode::parse("incompatible_protocol").unwrap();
    let request = RequestId::parse("request:01.test_value").unwrap();

    assert_eq!(
      serde_json::to_string(&request).unwrap(),
      "\"request:01.test_value\""
    );
    assert_eq!(
      serde_json::to_string(&error_code).unwrap(),
      "\"incompatible_protocol\""
    );
    assert!(RequestId::parse("request value").is_err());
    assert!(RequestId::parse("unknown").is_err());
    assert!(ProtocolErrorCode::parse("Invalid-Code").is_err());
    assert!(serde_json::from_str::<RequestId>("\"\"").is_err());
    assert!(serde_json::from_str::<ProtocolErrorCode>("\"bad-code\"").is_err());
  }

  #[test]
  fn identifier_traits_and_all_validation_boundaries_are_explicit() {
    let request = "request-1".parse::<RequestId>().unwrap();
    assert_eq!(request.as_ref(), "request-1");
    assert_eq!(request.to_string(), "request-1");
    assert_eq!(
      RequestId::try_from("request-2".to_string())
        .unwrap()
        .as_str(),
      "request-2"
    );
    assert_eq!(String::from(request.clone()), "request-1");
    assert_eq!(
      serde_json::from_str::<RequestId>("\"request-1\"").unwrap(),
      request
    );

    for invalid in [
      "".to_string(),
      "x".repeat(MAX_REQUEST_ID_BYTES + 1),
      "unknown".to_string(),
      "_leading".to_string(),
      "request/value".to_string(),
      "żądanie".to_string(),
    ] {
      let error = RequestId::parse(invalid).unwrap_err();
      assert!(error.to_string().starts_with("invalid request ID:"));
      assert!(std::error::Error::source(&error).is_none());
    }

    let code = "storage_error".parse::<ProtocolErrorCode>().unwrap();
    assert_eq!(code.as_ref(), "storage_error");
    assert_eq!(code.to_string(), "storage_error");
    assert_eq!(
      ProtocolErrorCode::try_from("error2".to_string()).unwrap(),
      "error2".parse().unwrap()
    );
    assert_eq!(
      serde_json::from_str::<ProtocolErrorCode>("\"storage_error\"").unwrap(),
      code
    );

    for invalid in [
      "".to_string(),
      "x".repeat(MAX_ERROR_CODE_BYTES + 1),
      "_leading".to_string(),
      "trailing_".to_string(),
      "Uppercase".to_string(),
      "bad-code".to_string(),
    ] {
      let error = ProtocolErrorCode::parse(invalid).unwrap_err();
      assert!(
        error
          .to_string()
          .starts_with("invalid protocol error code:")
      );
    }
  }
}
