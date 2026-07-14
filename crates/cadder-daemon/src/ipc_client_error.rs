use cadder_ipc::{ProtocolError, ProtocolErrorKind, RequestId};
use serde::Serialize;
use std::{error::Error as StdError, fmt};
use thiserror::Error;

pub(crate) type BoxError = Box<dyn StdError + Send + Sync + 'static>;

/// Result returned by every local Cadder IPC client API.
pub type IpcClientResult<T> = Result<T, IpcClientError>;

/// A failure observed by a Cadder IPC client.
///
/// Daemon rejections retain the complete wire [`ProtocolError`]. Local failures remain distinct
/// and identify the phase that failed before a protocol response was available.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum IpcClientError {
  #[error("{0}")]
  Daemon(#[source] ProtocolError),
  #[error("{0}")]
  Local(#[source] LocalIpcError),
}

impl IpcClientError {
  pub(crate) fn daemon(error: ProtocolError) -> Self {
    Self::Daemon(error)
  }

  pub(crate) fn local(context: LocalIpcErrorContext) -> Self {
    Self::Local(LocalIpcError {
      kind: context.kind,
      phase: context.phase,
      code: context.code,
      message: context.message,
      guidance: context.guidance,
      retryable: context.retryable,
      request_id: context.request_id,
      operation: context.operation,
      source: context.source,
    })
  }

  pub(crate) fn with_request_context(
    mut self,
    request_id: RequestId,
    operation: impl Into<Box<str>>,
  ) -> Self {
    if let Self::Local(error) = &mut self {
      error.request_id.get_or_insert(request_id);
      if error.operation.is_none() {
        error.operation = Some(operation.into());
      }
    }
    self
  }

  pub(crate) fn with_source(mut self, source: BoxError) -> Self {
    if let Self::Local(error) = &mut self {
      error.source = Some(source);
    }
    self
  }

  /// Returns the stable machine code without erasing whether the daemon or local client produced
  /// it.
  pub fn code(&self) -> &str {
    match self {
      Self::Daemon(error) => error.code.as_str(),
      Self::Local(error) => error.code().as_str(),
    }
  }

  /// Returns the primary human-readable outcome.
  pub fn message(&self) -> &str {
    match self {
      Self::Daemon(error) => &error.message,
      Self::Local(error) => error.message(),
    }
  }

  /// Returns the most relevant recovery action, when one is available.
  pub fn guidance(&self) -> Option<&str> {
    match self {
      Self::Daemon(error) => error.guidance.as_deref(),
      Self::Local(error) => error.guidance(),
    }
  }

  /// Reports whether the same operation is safe to retry under its bounded policy.
  pub fn retryable(&self) -> bool {
    match self {
      Self::Daemon(error) => error.retryable,
      Self::Local(error) => error.retryable(),
    }
  }

  /// Returns the request ID retained from either the daemon response or local request context.
  pub fn request_id(&self) -> Option<&RequestId> {
    match self {
      Self::Daemon(error) => error.request_id.as_ref(),
      Self::Local(error) => error.request_id(),
    }
  }

  /// Returns the locally correlated operation, or the denied operation explicitly reported by the
  /// daemon.
  pub fn operation(&self) -> Option<&str> {
    match self {
      Self::Daemon(error) => error.denied_operation.as_deref(),
      Self::Local(error) => error.operation(),
    }
  }

  /// Returns the exact daemon error without flattening its wire fields.
  pub const fn daemon_error(&self) -> Option<&ProtocolError> {
    match self {
      Self::Daemon(error) => Some(error),
      Self::Local(_) => None,
    }
  }

  /// Returns the typed local failure when no daemon error was received.
  pub const fn local_error(&self) -> Option<&LocalIpcError> {
    match self {
      Self::Daemon(_) => None,
      Self::Local(error) => Some(error),
    }
  }

  /// Reports whether access failed at either the local transport or daemon policy boundary.
  pub fn is_permission_denied(&self) -> bool {
    match self {
      Self::Daemon(error) => {
        error.kind == ProtocolErrorKind::AccessDenied || error.code.as_str() == "permission_denied"
      }
      Self::Local(error) => error.code() == LocalIpcErrorCode::PermissionDenied,
    }
  }

  /// Reports whether the selected runtime has no reachable daemon endpoint.
  pub fn is_daemon_unavailable(&self) -> bool {
    match self {
      Self::Daemon(_) => false,
      Self::Local(error) => matches!(
        error.code(),
        LocalIpcErrorCode::DaemonUnavailable | LocalIpcErrorCode::DiscoveryUnavailable
      ),
    }
  }

  /// Reports whether the client and daemon protocol contracts cannot interoperate unchanged.
  pub fn is_protocol_incompatible(&self) -> bool {
    match self {
      Self::Daemon(error) => error.kind == ProtocolErrorKind::IncompatibleProtocolVersion,
      Self::Local(error) => error.code() == LocalIpcErrorCode::IncompatibleProtocol,
    }
  }

  /// Reports that discovery and the connected daemon identified different live instances.
  pub fn is_stale_instance(&self) -> bool {
    match self {
      Self::Daemon(error) => error.kind == ProtocolErrorKind::StaleInstance,
      Self::Local(error) => error.code() == LocalIpcErrorCode::StaleInstance,
    }
  }
}

pub(crate) struct LocalIpcErrorContext {
  pub kind: LocalIpcErrorKind,
  pub phase: IpcClientPhase,
  pub code: LocalIpcErrorCode,
  pub message: Box<str>,
  pub guidance: Option<Box<str>>,
  pub retryable: bool,
  pub request_id: Option<RequestId>,
  pub operation: Option<Box<str>>,
  pub source: Option<BoxError>,
}

/// Stable machine code for a local IPC failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LocalIpcErrorCode {
  ConnectionClosed,
  DaemonNotFound,
  DaemonStartFailed,
  DaemonUnavailable,
  DiscoveryReadFailed,
  DiscoveryUnavailable,
  Frame,
  IncompatibleProtocol,
  InvalidDiscovery,
  InvalidEndpoint,
  InvalidInput,
  InvalidRequest,
  InvalidRuntime,
  PermissionDenied,
  ProtocolViolation,
  StaleInstance,
  Timeout,
  TransportConnect,
  TransportRead,
  TransportWrite,
  UnexpectedEof,
}

impl LocalIpcErrorCode {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::ConnectionClosed => "connection_closed",
      Self::DaemonNotFound => "daemon_not_found",
      Self::DaemonStartFailed => "daemon_start_failed",
      Self::DaemonUnavailable => "daemon_unavailable",
      Self::DiscoveryReadFailed => "discovery_read_failed",
      Self::DiscoveryUnavailable => "discovery_unavailable",
      Self::Frame => "frame",
      Self::IncompatibleProtocol => "incompatible_protocol",
      Self::InvalidDiscovery => "invalid_discovery",
      Self::InvalidEndpoint => "invalid_endpoint",
      Self::InvalidInput => "invalid_input",
      Self::InvalidRequest => "invalid_request",
      Self::InvalidRuntime => "invalid_runtime",
      Self::PermissionDenied => "permission_denied",
      Self::ProtocolViolation => "protocol_violation",
      Self::StaleInstance => "stale_instance",
      Self::Timeout => "timeout",
      Self::TransportConnect => "transport_connect",
      Self::TransportRead => "transport_read",
      Self::TransportWrite => "transport_write",
      Self::UnexpectedEof => "unexpected_eof",
    }
  }
}

impl fmt::Display for LocalIpcErrorCode {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

/// High-level class for a failure produced by the local client rather than the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum LocalIpcErrorKind {
  Discovery,
  Transport,
  Timeout,
}

/// The local client phase that failed before a valid daemon outcome arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum IpcClientPhase {
  DiscoveryRead,
  DiscoveryDecode,
  EndpointResolve,
  Connect,
  DaemonLaunch,
  DaemonReadiness,
  RequestEncode,
  RequestWrite,
  ResponseRead,
  ResponseDecode,
  ResponseValidate,
}

/// Structured details for a local IPC failure.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct LocalIpcError {
  kind: LocalIpcErrorKind,
  phase: IpcClientPhase,
  code: LocalIpcErrorCode,
  message: Box<str>,
  guidance: Option<Box<str>>,
  retryable: bool,
  request_id: Option<RequestId>,
  operation: Option<Box<str>>,
  #[source]
  source: Option<BoxError>,
}

impl LocalIpcError {
  pub const fn kind(&self) -> LocalIpcErrorKind {
    self.kind
  }

  pub const fn phase(&self) -> IpcClientPhase {
    self.phase
  }

  pub const fn code(&self) -> LocalIpcErrorCode {
    self.code
  }

  pub fn message(&self) -> &str {
    &self.message
  }

  pub fn guidance(&self) -> Option<&str> {
    self.guidance.as_deref()
  }

  pub const fn retryable(&self) -> bool {
    self.retryable
  }

  pub fn request_id(&self) -> Option<&RequestId> {
    self.request_id.as_ref()
  }

  pub fn operation(&self) -> Option<&str> {
    self.operation.as_deref()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::BTreeSet;

  #[test]
  fn typed_error_local_codes_are_unique_and_match_the_serialized_contract() {
    let cases = [
      (LocalIpcErrorCode::ConnectionClosed, "connection_closed"),
      (LocalIpcErrorCode::DaemonNotFound, "daemon_not_found"),
      (LocalIpcErrorCode::DaemonStartFailed, "daemon_start_failed"),
      (LocalIpcErrorCode::DaemonUnavailable, "daemon_unavailable"),
      (
        LocalIpcErrorCode::DiscoveryReadFailed,
        "discovery_read_failed",
      ),
      (
        LocalIpcErrorCode::DiscoveryUnavailable,
        "discovery_unavailable",
      ),
      (LocalIpcErrorCode::Frame, "frame"),
      (
        LocalIpcErrorCode::IncompatibleProtocol,
        "incompatible_protocol",
      ),
      (LocalIpcErrorCode::InvalidDiscovery, "invalid_discovery"),
      (LocalIpcErrorCode::InvalidEndpoint, "invalid_endpoint"),
      (LocalIpcErrorCode::InvalidInput, "invalid_input"),
      (LocalIpcErrorCode::InvalidRequest, "invalid_request"),
      (LocalIpcErrorCode::InvalidRuntime, "invalid_runtime"),
      (LocalIpcErrorCode::PermissionDenied, "permission_denied"),
      (LocalIpcErrorCode::ProtocolViolation, "protocol_violation"),
      (LocalIpcErrorCode::StaleInstance, "stale_instance"),
      (LocalIpcErrorCode::Timeout, "timeout"),
      (LocalIpcErrorCode::TransportConnect, "transport_connect"),
      (LocalIpcErrorCode::TransportRead, "transport_read"),
      (LocalIpcErrorCode::TransportWrite, "transport_write"),
      (LocalIpcErrorCode::UnexpectedEof, "unexpected_eof"),
    ];
    let mut unique = BTreeSet::new();

    for (code, expected) in cases {
      assert_eq!(code.as_str(), expected);
      assert_eq!(code.to_string(), expected);
      assert_eq!(
        serde_json::to_string(&code).unwrap(),
        format!("\"{expected}\"")
      );
      assert!(unique.insert(expected));
    }
  }
}
