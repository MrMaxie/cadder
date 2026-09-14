use crate::{ProtocolErrorCode, ProtocolVersion, RequestId};
use serde::{Deserialize, Serialize};
use std::{fmt, ops::Deref};

pub type ProtocolResult<T> = Result<T, ProtocolError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProtocolErrorKind {
  IncompatibleProtocolVersion,
  UnsupportedOperation,
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
  pub const fn default_code(&self) -> &'static str {
    match self {
      Self::IncompatibleProtocolVersion => "incompatible_protocol",
      Self::UnsupportedOperation => "unsupported_operation",
      Self::PayloadDecodeFailed => "invalid_payload",
      Self::AccessDenied => "permission_denied",
      Self::InvalidInput => "invalid_input",
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ProtocolError(Box<ProtocolErrorData>);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtocolErrorData {
  pub kind: ProtocolErrorKind,
  pub code: ProtocolErrorCode,
  pub message: Box<str>,
  pub guidance: Option<Box<str>>,
  pub retryable: bool,
  pub request_id: Option<RequestId>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub denied_operation: Option<Box<str>>,
}

impl ProtocolError {
  pub fn new(
    kind: ProtocolErrorKind,
    code: ProtocolErrorCode,
    message: impl Into<Box<str>>,
    guidance: Option<Box<str>>,
    retryable: bool,
  ) -> Self {
    Self(Box::new(ProtocolErrorData {
      kind,
      code,
      message: message.into(),
      guidance,
      retryable,
      request_id: None,
      denied_operation: None,
    }))
  }

  pub fn incompatible_protocol_version(version: u16) -> Self {
    Self::new(
      ProtocolErrorKind::IncompatibleProtocolVersion,
      ProtocolErrorCode::known("incompatible_protocol"),
      format!("Cadder does not support protocol version {version}."),
      Some("Use the same Cadder version for cadder, cadderd, and caddy.".into()),
      false,
    )
  }

  pub fn incompatible_protocol_version_pair(
    offered: ProtocolVersion,
    supported: ProtocolVersion,
  ) -> Self {
    Self::new(
      ProtocolErrorKind::IncompatibleProtocolVersion,
      ProtocolErrorCode::known("incompatible_protocol"),
      format!(
        "Cadder protocol {} does not match the required protocol {}.",
        offered, supported
      ),
      Some("Use version-matched Cadder binaries from one portable archive.".into()),
      false,
    )
  }

  pub fn payload_decode_failed(error: impl fmt::Display) -> Self {
    Self::new(
      ProtocolErrorKind::PayloadDecodeFailed,
      ProtocolErrorCode::known("invalid_payload"),
      format!("Cadder could not decode the request payload: {error}"),
      Some("Send the payload defined for this exact Cadder protocol version.".into()),
      false,
    )
  }

  pub fn incompatible_payload_contract(path: Option<&str>) -> Self {
    let location = path.map_or(String::new(), |path| format!(" at `{path}`"));
    Self::new(
      ProtocolErrorKind::IncompatibleProtocolVersion,
      ProtocolErrorCode::known("incompatible_payload"),
      format!("The request uses an unsupported payload contract{location}."),
      Some("Use version-matched Cadder binaries from one portable archive.".into()),
      false,
    )
  }

  pub fn decoder_contract_mismatch(expected: &str, actual: &str) -> Self {
    Self::new(
      ProtocolErrorKind::ProtocolViolation,
      ProtocolErrorCode::known("decoder_contract_mismatch"),
      format!("The `{actual}` decoder cannot handle the `{expected}` operation."),
      Some("Report this Cadder dispatcher defect.".into()),
      false,
    )
  }

  pub fn unsupported_operation(operation: impl Into<String>, _supported: impl Sized) -> Self {
    let operation = operation.into();
    Self::new(
      ProtocolErrorKind::UnsupportedOperation,
      ProtocolErrorCode::known("unsupported_operation"),
      format!("Cadder does not support the `{operation}` operation."),
      Some("Use one of the eight operations supported by this Cadder version.".into()),
      false,
    )
  }

  pub fn access_denied(
    operation: impl Into<String>,
    message: impl Into<Box<str>>,
    guidance: Option<String>,
  ) -> Self {
    let operation = operation.into();
    let mut error = Self::new(
      ProtocolErrorKind::AccessDenied,
      ProtocolErrorCode::known("permission_denied"),
      message,
      guidance.map(String::into_boxed_str),
      false,
    );
    error.0.denied_operation = Some(operation.into_boxed_str());
    error
  }

  pub fn stale_instance() -> Self {
    Self::new(
      ProtocolErrorKind::StaleInstance,
      ProtocolErrorCode::known("stale_instance"),
      "The local endpoint belongs to a different Cadder runtime instance.",
      Some("Reconnect through the current local endpoint.".into()),
      true,
    )
  }

  pub fn with_request_id(mut self, request_id: RequestId) -> Self {
    self.0.request_id = Some(request_id);
    self
  }

  pub(crate) fn for_versioned_response(mut self, request_id: RequestId) -> Self {
    self.0.request_id = Some(request_id);
    self
  }
}

impl Deref for ProtocolError {
  type Target = ProtocolErrorData;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl fmt::Display for ProtocolError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.message)
  }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtocolErrorResponse {
  pub request_id: String,
  pub error: ProtocolError,
}

impl ProtocolErrorResponse {
  pub fn rejected(request_id: Option<RequestId>, error: ProtocolError) -> Self {
    let request_id = request_id.unwrap_or_else(|| {
      RequestId::parse("uncorrelated-error").expect("static request ID is valid")
    });
    Self {
      request_id: request_id.to_string(),
      error: error.for_versioned_response(request_id),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_error_kind_has_a_stable_machine_code() {
    let cases = [
      (
        ProtocolErrorKind::IncompatibleProtocolVersion,
        "incompatible_protocol",
      ),
      (
        ProtocolErrorKind::UnsupportedOperation,
        "unsupported_operation",
      ),
      (ProtocolErrorKind::PayloadDecodeFailed, "invalid_payload"),
      (ProtocolErrorKind::AccessDenied, "permission_denied"),
      (ProtocolErrorKind::InvalidInput, "invalid_input"),
      (ProtocolErrorKind::Conflict, "conflict"),
      (ProtocolErrorKind::Configuration, "configuration"),
      (ProtocolErrorKind::CaddyRuntime, "caddy_runtime"),
      (ProtocolErrorKind::Storage, "storage"),
      (ProtocolErrorKind::Busy, "busy"),
      (ProtocolErrorKind::Frame, "frame"),
      (ProtocolErrorKind::Timeout, "timeout"),
      (ProtocolErrorKind::ProtocolViolation, "protocol_violation"),
      (ProtocolErrorKind::ShuttingDown, "shutting_down"),
      (ProtocolErrorKind::StaleInstance, "stale_instance"),
      (ProtocolErrorKind::Internal, "internal"),
    ];
    for (kind, expected) in cases {
      assert_eq!(kind.default_code(), expected);
    }
  }

  #[test]
  fn specialized_errors_keep_actionable_protocol_context() {
    let errors = [
      ProtocolError::incompatible_protocol_version(7),
      ProtocolError::incompatible_protocol_version_pair(
        ProtocolVersion::new(7, 0).unwrap(),
        ProtocolVersion::new(1, 0).unwrap(),
      ),
      ProtocolError::payload_decode_failed("bad field"),
      ProtocolError::incompatible_payload_contract(None),
      ProtocolError::incompatible_payload_contract(Some("payload.mode")),
      ProtocolError::decoder_contract_mismatch("query-state", "query-logs"),
      ProtocolError::unsupported_operation("removed-operation", ()),
      ProtocolError::stale_instance(),
    ];
    for error in errors {
      assert!(!error.message.is_empty());
      assert!(error.guidance.is_some());
      assert_eq!(error.to_string(), error.message.as_ref());
      assert!(std::error::Error::source(&error).is_none());
    }

    let denied = ProtocolError::access_denied(
      "shutdown-daemon",
      "different local user",
      Some("Use the runtime owner.".to_string()),
    );
    assert_eq!(denied.kind, ProtocolErrorKind::AccessDenied);
    assert_eq!(denied.denied_operation.as_deref(), Some("shutdown-daemon"));
    assert!(!denied.retryable);
  }

  #[test]
  fn protocol_error_responses_always_have_exact_correlation() {
    let request_id = RequestId::parse("request-1").unwrap();
    let response =
      ProtocolErrorResponse::rejected(Some(request_id.clone()), ProtocolError::stale_instance());
    assert_eq!(response.request_id, request_id.as_str());
    assert_eq!(response.error.request_id.as_ref(), Some(&request_id));

    let uncorrelated =
      ProtocolErrorResponse::rejected(None, ProtocolError::payload_decode_failed("bad"));
    assert_eq!(uncorrelated.request_id, "uncorrelated-error");
    assert_eq!(
      uncorrelated
        .error
        .request_id
        .as_ref()
        .map(RequestId::as_str),
      Some("uncorrelated-error")
    );

    let explicit = ProtocolError::stale_instance().with_request_id(request_id.clone());
    let json = serde_json::to_string(&explicit).unwrap();
    assert_eq!(
      serde_json::from_str::<ProtocolError>(&json).unwrap(),
      explicit
    );
  }
}
