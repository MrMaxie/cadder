use anyhow::Error;
use cadder_daemon::{IpcClientError, IpcClientPhase, LocalIpcErrorCode, LocalIpcErrorKind};
use cadder_ipc::{ProtocolError, ProtocolErrorKind, RequestId};
use serde::Serialize;
use std::{
  path::Path,
  process::{ExitCode, Termination},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[repr(u8)]
#[serde(rename_all = "camelCase")]
pub enum AppExit {
  Success = 0,
  InvalidUsage = 2,
  DaemonUnavailable = 3,
  DaemonStartFailure = 4,
  TargetNotFound = 5,
  ConflictOrRejected = 6,
  PermissionOrElevation = 7,
  UnsupportedOperation = 8,
  IpcFailure = 9,
}

impl Termination for AppExit {
  fn report(self) -> ExitCode {
    ExitCode::from(self as u8)
  }
}

#[derive(Debug, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct OperatorError {
  pub kind: AppExit,
  pub message: String,
  pub guidance: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub daemon_error: Option<ProtocolError>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub local_error: Option<Box<OperatorLocalIpcError>>,
  #[serde(skip)]
  pub command: &'static str,
  #[serde(skip)]
  #[source]
  ipc_error: Option<Box<IpcClientError>>,
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
    kind: AppExit,
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

  pub fn invalid_usage(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(command, AppExit::InvalidUsage, message, guidance)
  }

  pub fn target_not_found(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(command, AppExit::TargetNotFound, message, guidance)
  }

  pub fn conflict_or_rejected(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(command, AppExit::ConflictOrRejected, message, guidance)
  }

  pub fn unsupported(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(command, AppExit::UnsupportedOperation, message, guidance)
  }

  pub fn daemon_request(
    command: &'static str,
    paths: &Path,
    action: &str,
    error: IpcClientError,
  ) -> Self {
    let kind = app_exit_for_ipc(&error);
    let message = if kind == AppExit::DaemonUnavailable {
      format!(
        "Cadder daemon is unavailable for runtime `{}`.",
        paths.display()
      )
    } else {
      format!("Could not {action}: {}", sentence(error.message()))
    };
    let guidance = if kind == AppExit::DaemonUnavailable {
      Some(start_guidance(paths))
    } else {
      error.guidance().map(ToOwned::to_owned).or_else(|| {
        if kind == AppExit::IpcFailure {
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
      AppExit::PermissionOrElevation
    } else {
      AppExit::DaemonStartFailure
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
    self.ipc_error = Some(Box::new(error));
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

fn app_exit_for_ipc(error: &IpcClientError) -> AppExit {
  if error.is_permission_denied() {
    return AppExit::PermissionOrElevation;
  }
  if daemon_error_indicates_unavailable(error) {
    return AppExit::DaemonUnavailable;
  }
  if error.is_protocol_incompatible() {
    return AppExit::UnsupportedOperation;
  }
  match error.daemon_error().map(|error| &error.kind) {
    Some(ProtocolErrorKind::Conflict) => AppExit::ConflictOrRejected,
    Some(
      ProtocolErrorKind::IncompatibleProtocolVersion | ProtocolErrorKind::UnsupportedOperation,
    ) => AppExit::UnsupportedOperation,
    _ => AppExit::IpcFailure,
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
  if error.chain().any(|cause| {
    cause
      .downcast_ref::<std::io::Error>()
      .is_some_and(|error| matches!(error.kind(), std::io::ErrorKind::PermissionDenied))
  }) {
    return true;
  }

  let error_chain = format_error_chain(error).to_ascii_lowercase();
  error_chain.contains("permission denied") || error_chain.contains("access is denied")
}

pub fn start_guidance(_paths: &Path) -> String {
  "Open Cadder and start it from Status, then retry.".to_string()
}

#[cfg(test)]
mod tests;
