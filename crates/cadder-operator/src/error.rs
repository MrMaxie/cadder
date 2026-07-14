use anyhow::Error;
use cadder_daemon::{IpcClientError, IpcClientPhase, LocalIpcErrorCode, LocalIpcErrorKind};
use cadder_protocol::{ProtocolError, ProtocolErrorKind, RequestId};
use serde::Serialize;
use std::{
  error::Error as StdError,
  fmt::{self, Display},
  path::Path,
  sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperatorExitCode {
  Success,
  InvalidUsage,
  DaemonUnavailable,
  DaemonStartFailure,
  TargetNotFound,
  ConflictOrRejected,
  PermissionOrElevation,
  UnsupportedOperation,
  IpcFailure,
}

impl OperatorExitCode {
  pub fn code(self) -> u8 {
    match self {
      Self::Success => 0,
      Self::InvalidUsage => 2,
      Self::DaemonUnavailable => 3,
      Self::DaemonStartFailure => 4,
      Self::TargetNotFound => 5,
      Self::ConflictOrRejected => 6,
      Self::PermissionOrElevation => 7,
      Self::UnsupportedOperation => 8,
      Self::IpcFailure => 9,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperatorErrorKind {
  InvalidUsage,
  DaemonUnavailable,
  DaemonStartFailure,
  TargetNotFound,
  ConflictOrRejected,
  PermissionOrElevation,
  UnsupportedOperation,
  IpcFailure,
}

impl OperatorErrorKind {
  pub fn exit_code(self) -> OperatorExitCode {
    match self {
      Self::InvalidUsage => OperatorExitCode::InvalidUsage,
      Self::DaemonUnavailable => OperatorExitCode::DaemonUnavailable,
      Self::DaemonStartFailure => OperatorExitCode::DaemonStartFailure,
      Self::TargetNotFound => OperatorExitCode::TargetNotFound,
      Self::ConflictOrRejected => OperatorExitCode::ConflictOrRejected,
      Self::PermissionOrElevation => OperatorExitCode::PermissionOrElevation,
      Self::UnsupportedOperation => OperatorExitCode::UnsupportedOperation,
      Self::IpcFailure => OperatorExitCode::IpcFailure,
    }
  }
}

#[derive(Debug, Clone, Serialize)]
pub struct OperatorError {
  pub kind: OperatorErrorKind,
  pub message: String,
  pub guidance: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub daemon_error: Option<ProtocolError>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub local_error: Option<Box<OperatorLocalIpcError>>,
  #[serde(skip)]
  pub command: &'static str,
  #[serde(skip)]
  ipc_error: Option<Arc<IpcClientError>>,
}

impl PartialEq for OperatorError {
  fn eq(&self, other: &Self) -> bool {
    self.kind == other.kind
      && self.message == other.message
      && self.guidance == other.guidance
      && self.daemon_error == other.daemon_error
      && self.local_error == other.local_error
      && self.command == other.command
  }
}

impl Eq for OperatorError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorLocalIpcError {
  pub kind: LocalIpcErrorKind,
  pub phase: IpcClientPhase,
  pub code: LocalIpcErrorCode,
  pub message: String,
  pub guidance: Option<String>,
  pub retryable: bool,
  pub request_id: Option<RequestId>,
  pub operation: Option<String>,
}

impl OperatorError {
  pub fn new(
    command: &'static str,
    kind: OperatorErrorKind,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self {
      kind,
      message: message.into(),
      guidance,
      daemon_error: None,
      local_error: None,
      command,
      ipc_error: None,
    }
  }

  pub fn exit_code(&self) -> OperatorExitCode {
    self.kind.exit_code()
  }

  pub fn invalid_usage(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(command, OperatorErrorKind::InvalidUsage, message, guidance)
  }

  pub fn target_not_found(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(
      command,
      OperatorErrorKind::TargetNotFound,
      message,
      guidance,
    )
  }

  pub fn conflict_or_rejected(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(
      command,
      OperatorErrorKind::ConflictOrRejected,
      message,
      guidance,
    )
  }

  pub fn unsupported(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(
      command,
      OperatorErrorKind::UnsupportedOperation,
      message,
      guidance,
    )
  }

  pub fn daemon_request(
    command: &'static str,
    paths: &Path,
    action: &str,
    error: IpcClientError,
  ) -> Self {
    let kind = operator_kind_for_ipc(&error);
    let message = if kind == OperatorErrorKind::DaemonUnavailable {
      format!(
        "Cadder daemon is unavailable for runtime `{}`.",
        paths.display()
      )
    } else {
      format!("Could not {action}: {}", sentence(error.message()))
    };
    let guidance = if kind == OperatorErrorKind::DaemonUnavailable {
      Some(start_guidance(paths))
    } else {
      error.guidance().map(ToOwned::to_owned).or_else(|| {
        if kind == OperatorErrorKind::IpcFailure {
          Some(format!(
            "Inspect cadderd diagnostics for runtime `{}`, correct the reported protocol or transport error, then retry.",
            paths.display()
          ))
        } else {
          None
        }
      })
    };
    Self::new(command, kind, message, guidance).with_ipc_error(error)
  }

  pub fn daemon_start(command: &'static str, _paths: &Path, error: IpcClientError) -> Self {
    let permission_denied = error.is_permission_denied();
    let kind = if permission_denied {
      OperatorErrorKind::PermissionOrElevation
    } else {
      OperatorErrorKind::DaemonStartFailure
    };
    let guidance = error.guidance().map(ToOwned::to_owned).or_else(|| {
      Some(if permission_denied {
        "Use the account that owns this Cadder runtime and verify access to the daemon executable and runtime directory."
          .to_string()
      } else {
        "Fix the Cadder startup problem, then open Cadder and start it from Status."
          .to_string()
      })
    });

    Self::new(
      command,
      kind,
      format!("Could not start cadderd: {}", sentence(error.message())),
      guidance,
    )
    .with_ipc_error(error)
  }

  fn with_ipc_error(mut self, error: IpcClientError) -> Self {
    self.daemon_error = error.daemon_error().cloned();
    self.local_error = error.local_error().map(|local| {
      Box::new(OperatorLocalIpcError {
        kind: local.kind(),
        phase: local.phase(),
        code: local.code(),
        message: local.message().to_string(),
        guidance: local.guidance().map(ToOwned::to_owned),
        retryable: local.retryable(),
        request_id: local.request_id().cloned(),
        operation: local.operation().map(ToOwned::to_owned),
      })
    });
    self.ipc_error = Some(Arc::new(error));
    self
  }
}

pub(crate) fn sentence(message: &str) -> String {
  let message = message.trim();
  if message.ends_with(['.', '!', '?']) {
    message.to_string()
  } else {
    format!("{message}.")
  }
}

fn operator_kind_for_ipc(error: &IpcClientError) -> OperatorErrorKind {
  if error.is_permission_denied() {
    return OperatorErrorKind::PermissionOrElevation;
  }
  if daemon_error_indicates_unavailable(error) {
    return OperatorErrorKind::DaemonUnavailable;
  }
  if error.is_protocol_incompatible() {
    return OperatorErrorKind::UnsupportedOperation;
  }
  match error.daemon_error().map(|error| &error.kind) {
    Some(ProtocolErrorKind::Conflict) => OperatorErrorKind::ConflictOrRejected,
    Some(
      ProtocolErrorKind::IncompatibleProtocolVersion | ProtocolErrorKind::UnsupportedCapability,
    ) => OperatorErrorKind::UnsupportedOperation,
    _ => OperatorErrorKind::IpcFailure,
  }
}

impl Display for OperatorError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.message)
  }
}

impl std::error::Error for OperatorError {
  fn source(&self) -> Option<&(dyn StdError + 'static)> {
    self
      .ipc_error
      .as_deref()
      .map(|error| error as &(dyn StdError + 'static))
  }
}

pub fn format_error_chain(error: &Error) -> String {
  let mut messages = error.chain().map(ToString::to_string).collect::<Vec<_>>();
  messages.dedup();
  messages.join(": ")
}

pub fn daemon_error_indicates_unavailable(error: &IpcClientError) -> bool {
  error.is_daemon_unavailable()
}

pub fn error_indicates_permission(error: &Error) -> bool {
  error.chain().any(|cause| {
    cause
      .downcast_ref::<std::io::Error>()
      .is_some_and(|error| matches!(error.kind(), std::io::ErrorKind::PermissionDenied))
  }) || format_error_chain(error)
    .to_ascii_lowercase()
    .contains("permission denied")
    || format_error_chain(error)
      .to_ascii_lowercase()
      .contains("access is denied")
}

pub fn start_guidance(_paths: &Path) -> String {
  "Open Cadder and start it from Status, then retry.".to_string()
}

#[cfg(test)]
mod tests {
  use super::*;
  use anyhow::{Context, anyhow};
  use cadder_daemon::{CadderSession, RuntimePaths};
  use cadder_protocol::ProtocolErrorCode;

  #[test]
  fn exit_codes_remain_stable_for_all_error_kinds() {
    assert_eq!(OperatorExitCode::Success.code(), 0);
    assert_eq!(OperatorExitCode::InvalidUsage.code(), 2);
    assert_eq!(OperatorExitCode::DaemonUnavailable.code(), 3);
    assert_eq!(OperatorExitCode::DaemonStartFailure.code(), 4);
    assert_eq!(OperatorExitCode::TargetNotFound.code(), 5);
    assert_eq!(OperatorExitCode::ConflictOrRejected.code(), 6);
    assert_eq!(OperatorExitCode::PermissionOrElevation.code(), 7);
    assert_eq!(OperatorExitCode::UnsupportedOperation.code(), 8);
    assert_eq!(OperatorExitCode::IpcFailure.code(), 9);
  }

  #[tokio::test]
  async fn typed_error_daemon_unavailable_maps_to_stable_exit_code() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let error = CadderSession::connect(&paths).await.unwrap_err();

    let mapped =
      OperatorError::daemon_request("domains list", Path::new("runtime"), "query domains", error);

    assert_eq!(mapped.kind, OperatorErrorKind::DaemonUnavailable);
    assert_eq!(mapped.exit_code().code(), 3);
    assert_eq!(
      mapped.guidance.as_deref(),
      Some("Open Cadder and start it from Status, then retry.")
    );
  }

  #[test]
  fn typed_error_permission_maps_to_permission_exit_code() {
    let error = IpcClientError::Daemon(ProtocolError::access_denied(
      "start-daemon",
      "Permission denied.",
      None,
    ));

    let mapped = OperatorError::daemon_start("daemon start", Path::new("runtime"), error);

    assert_eq!(mapped.kind, OperatorErrorKind::PermissionOrElevation);
    assert_eq!(mapped.exit_code().code(), 7);
  }

  #[test]
  fn typed_error_unknown_daemon_error_maps_to_ipc_failure() {
    let error = daemon_ipc_error(
      ProtocolErrorKind::Internal,
      "internal",
      "Unexpected daemon envelope.",
    );

    let mapped = OperatorError::daemon_request(
      "logs show",
      Path::new("runtime"),
      "query retained logs",
      error,
    );

    assert_eq!(mapped.kind, OperatorErrorKind::IpcFailure);
    assert!(mapped.message.contains("Unexpected daemon envelope"));
    assert!(
      mapped
        .guidance
        .as_deref()
        .is_some_and(|guidance| guidance.contains("Inspect cadderd diagnostics"))
    );
  }

  #[test]
  fn typed_error_incompatible_protocol_maps_to_unsupported_exit_code() {
    let error = IpcClientError::Daemon(ProtocolError::incompatible_protocol_version(0));

    let mapped =
      OperatorError::daemon_request("status", Path::new("runtime"), "query state", error);

    assert_eq!(mapped.kind, OperatorErrorKind::UnsupportedOperation);
    assert_eq!(mapped.exit_code().code(), 8);
    assert!(mapped.daemon_error.is_some());
  }

  #[test]
  fn typed_error_daemon_start_failure_keeps_stable_exit_code() {
    let error = daemon_ipc_error(
      ProtocolErrorKind::Internal,
      "daemon_start_failed",
      "Spawn failed.",
    );

    let mapped = OperatorError::daemon_start("daemon start", Path::new("runtime"), error);

    assert_eq!(mapped.kind, OperatorErrorKind::DaemonStartFailure);
    assert_eq!(mapped.exit_code().code(), 4);
    assert!(mapped.message.contains("Spawn failed."));
    assert!(!mapped.message.ends_with(".."));
  }

  #[test]
  fn operator_error_kinds_and_constructors_keep_stable_contracts() {
    let cases = [
      (
        OperatorErrorKind::InvalidUsage,
        OperatorExitCode::InvalidUsage,
      ),
      (
        OperatorErrorKind::DaemonUnavailable,
        OperatorExitCode::DaemonUnavailable,
      ),
      (
        OperatorErrorKind::DaemonStartFailure,
        OperatorExitCode::DaemonStartFailure,
      ),
      (
        OperatorErrorKind::TargetNotFound,
        OperatorExitCode::TargetNotFound,
      ),
      (
        OperatorErrorKind::ConflictOrRejected,
        OperatorExitCode::ConflictOrRejected,
      ),
      (
        OperatorErrorKind::PermissionOrElevation,
        OperatorExitCode::PermissionOrElevation,
      ),
      (
        OperatorErrorKind::UnsupportedOperation,
        OperatorExitCode::UnsupportedOperation,
      ),
      (OperatorErrorKind::IpcFailure, OperatorExitCode::IpcFailure),
    ];

    for (kind, exit_code) in cases {
      assert_eq!(kind.exit_code(), exit_code);
    }

    let unsupported = OperatorError::unsupported(
      "iis handoff",
      "not supported",
      Some("retry elsewhere".to_string()),
    );
    assert_eq!(unsupported.kind, OperatorErrorKind::UnsupportedOperation);
    assert_eq!(unsupported.to_string(), "not supported");
    assert_eq!(unsupported.exit_code().code(), 8);

    let target = OperatorError::target_not_found("domains enable", "missing", None);
    assert_eq!(target.kind, OperatorErrorKind::TargetNotFound);

    let conflict = OperatorError::conflict_or_rejected("domains enable", "busy", None);
    assert_eq!(conflict.kind, OperatorErrorKind::ConflictOrRejected);
  }

  #[test]
  fn daemon_request_and_permission_detection_cover_permission_and_unavailable_kinds() {
    let permission = Error::from(std::io::Error::new(
      std::io::ErrorKind::PermissionDenied,
      "access is denied",
    ));
    let permission_error = IpcClientError::Daemon(ProtocolError::access_denied(
      "query-history",
      "Access is denied.",
      None,
    ));
    let mapped = OperatorError::daemon_request(
      "history show",
      Path::new("runtime"),
      "query history",
      permission_error,
    );
    assert_eq!(mapped.kind, OperatorErrorKind::PermissionOrElevation);
    assert!(error_indicates_permission(&permission));

    let unavailable = daemon_ipc_error(
      ProtocolErrorKind::Internal,
      "daemon_unavailable",
      "The daemon is unavailable.",
    );
    assert!(!daemon_error_indicates_unavailable(&unavailable));

    let text_permission = anyhow!("Access is denied while opening pipe");
    assert!(error_indicates_permission(&text_permission));
    let chained = Err::<(), _>(anyhow!("outer")).context("outer").unwrap_err();
    assert_eq!(format_error_chain(&chained), "outer");
  }

  fn daemon_ipc_error(kind: ProtocolErrorKind, code: &str, message: &str) -> IpcClientError {
    IpcClientError::Daemon(ProtocolError::new(
      kind,
      ProtocolErrorCode::parse(code).unwrap(),
      message,
      None,
      false,
    ))
  }
}
