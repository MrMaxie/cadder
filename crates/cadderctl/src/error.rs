use anyhow::Error;
use serde::Serialize;
use std::{
  fmt::{self, Display},
  path::Path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CliExitCode {
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

impl CliExitCode {
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
pub enum CliErrorKind {
  InvalidUsage,
  DaemonUnavailable,
  DaemonStartFailure,
  TargetNotFound,
  ConflictOrRejected,
  PermissionOrElevation,
  UnsupportedOperation,
  IpcFailure,
}

impl CliErrorKind {
  pub fn exit_code(self) -> CliExitCode {
    match self {
      Self::InvalidUsage => CliExitCode::InvalidUsage,
      Self::DaemonUnavailable => CliExitCode::DaemonUnavailable,
      Self::DaemonStartFailure => CliExitCode::DaemonStartFailure,
      Self::TargetNotFound => CliExitCode::TargetNotFound,
      Self::ConflictOrRejected => CliExitCode::ConflictOrRejected,
      Self::PermissionOrElevation => CliExitCode::PermissionOrElevation,
      Self::UnsupportedOperation => CliExitCode::UnsupportedOperation,
      Self::IpcFailure => CliExitCode::IpcFailure,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CliError {
  pub kind: CliErrorKind,
  pub message: String,
  pub guidance: Option<String>,
  #[serde(skip)]
  pub command: &'static str,
}

impl CliError {
  pub fn new(
    command: &'static str,
    kind: CliErrorKind,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self {
      kind,
      message: message.into(),
      guidance,
      command,
    }
  }

  pub fn exit_code(&self) -> CliExitCode {
    self.kind.exit_code()
  }

  pub fn invalid_usage(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(command, CliErrorKind::InvalidUsage, message, guidance)
  }

  pub fn target_not_found(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(command, CliErrorKind::TargetNotFound, message, guidance)
  }

  pub fn conflict_or_rejected(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(command, CliErrorKind::ConflictOrRejected, message, guidance)
  }

  pub fn unsupported(
    command: &'static str,
    message: impl Into<String>,
    guidance: Option<String>,
  ) -> Self {
    Self::new(
      command,
      CliErrorKind::UnsupportedOperation,
      message,
      guidance,
    )
  }

  pub fn daemon_request(command: &'static str, paths: &Path, action: &str, error: &Error) -> Self {
    if error_indicates_permission(error) {
      return Self::new(
        command,
        CliErrorKind::PermissionOrElevation,
        format!("Could not {action}: {}.", format_error_chain(error)),
        Some("Check file-system and local IPC permissions, then retry the command.".to_string()),
      );
    }

    if daemon_error_indicates_unavailable(error) {
      return Self::new(
        command,
        CliErrorKind::DaemonUnavailable,
        format!(
          "Cadder daemon is unavailable for runtime `{}`.",
          paths.display()
        ),
        Some(start_guidance(paths)),
      );
    }

    Self::new(
      command,
      CliErrorKind::IpcFailure,
      format!("Could not {action}: {}.", format_error_chain(error)),
      Some(
        "The daemon responded unexpectedly or the IPC exchange failed before completion."
          .to_string(),
      ),
    )
  }

  pub fn daemon_start(command: &'static str, paths: &Path, error: &Error) -> Self {
    if error_indicates_permission(error) {
      return Self::new(
        command,
        CliErrorKind::PermissionOrElevation,
        format!("Could not start cadderd: {}.", format_error_chain(error)),
        Some(
          "Check the cadderd path, executable permissions, and runtime directory permissions before retrying."
            .to_string(),
        ),
      );
    }

    Self::new(
      command,
      CliErrorKind::DaemonStartFailure,
      format!("Could not start cadderd: {}.", format_error_chain(error)),
      Some(format!(
        "Retry `cadderctl daemon start --runtime-dir \"{}\"` after fixing the daemon path or startup problem.",
        paths.display()
      )),
    )
  }
}

impl Display for CliError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.message)
  }
}

impl std::error::Error for CliError {}

pub fn format_error_chain(error: &Error) -> String {
  let mut messages = error.chain().map(ToString::to_string).collect::<Vec<_>>();
  messages.dedup();
  messages.join(": ")
}

pub fn daemon_error_indicates_unavailable(error: &Error) -> bool {
  error.chain().any(|cause| {
    cause.downcast_ref::<std::io::Error>().is_some_and(|error| {
      matches!(
        error.kind(),
        std::io::ErrorKind::NotFound
          | std::io::ErrorKind::ConnectionRefused
          | std::io::ErrorKind::ConnectionAborted
          | std::io::ErrorKind::ConnectionReset
          | std::io::ErrorKind::UnexpectedEof
      )
    })
  })
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

pub fn start_guidance(paths: &Path) -> String {
  format!(
    "Start `cadderd --runtime-dir \"{}\"` or run `cadderctl daemon start --runtime-dir \"{}\"`, then retry.",
    paths.display(),
    paths.display()
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use anyhow::anyhow;

  #[test]
  fn exit_codes_remain_stable_for_all_error_kinds() {
    assert_eq!(CliExitCode::Success.code(), 0);
    assert_eq!(CliExitCode::InvalidUsage.code(), 2);
    assert_eq!(CliExitCode::DaemonUnavailable.code(), 3);
    assert_eq!(CliExitCode::DaemonStartFailure.code(), 4);
    assert_eq!(CliExitCode::TargetNotFound.code(), 5);
    assert_eq!(CliExitCode::ConflictOrRejected.code(), 6);
    assert_eq!(CliExitCode::PermissionOrElevation.code(), 7);
    assert_eq!(CliExitCode::UnsupportedOperation.code(), 8);
    assert_eq!(CliExitCode::IpcFailure.code(), 9);
  }

  #[test]
  fn daemon_unavailable_maps_to_stable_exit_code() {
    let error = Error::from(std::io::Error::new(
      std::io::ErrorKind::ConnectionRefused,
      "connection refused",
    ));

    let mapped = CliError::daemon_request(
      "domains list",
      Path::new("runtime"),
      "query domains",
      &error,
    );

    assert_eq!(mapped.kind, CliErrorKind::DaemonUnavailable);
    assert_eq!(mapped.exit_code().code(), 3);
    assert!(mapped.guidance.unwrap().contains("cadderctl daemon start"));
  }

  #[test]
  fn permission_errors_map_to_permission_exit_code() {
    let error = Error::from(std::io::Error::new(
      std::io::ErrorKind::PermissionDenied,
      "permission denied",
    ));

    let mapped = CliError::daemon_start("daemon start", Path::new("runtime"), &error);

    assert_eq!(mapped.kind, CliErrorKind::PermissionOrElevation);
    assert_eq!(mapped.exit_code().code(), 7);
  }

  #[test]
  fn daemon_request_maps_unknown_errors_to_ipc_failure() {
    let error = anyhow!("unexpected daemon envelope");

    let mapped = CliError::daemon_request(
      "logs show",
      Path::new("runtime"),
      "query retained logs",
      &error,
    );

    assert_eq!(mapped.kind, CliErrorKind::IpcFailure);
    assert!(mapped.message.contains("unexpected daemon envelope"));
    assert!(
      mapped
        .guidance
        .as_deref()
        .is_some_and(|guidance| guidance.contains("IPC exchange failed"))
    );
  }

  #[test]
  fn daemon_start_maps_non_permission_errors_to_start_failure() {
    let error = anyhow!("spawn failed");

    let mapped = CliError::daemon_start("daemon start", Path::new("runtime"), &error);

    assert_eq!(mapped.kind, CliErrorKind::DaemonStartFailure);
    assert_eq!(mapped.exit_code().code(), 4);
    assert!(mapped.message.contains("spawn failed"));
  }

  #[test]
  fn helper_constructors_set_expected_error_kinds() {
    assert_eq!(
      CliError::invalid_usage("cmd", "bad", None).kind,
      CliErrorKind::InvalidUsage
    );
    assert_eq!(
      CliError::target_not_found("cmd", "missing", None).kind,
      CliErrorKind::TargetNotFound
    );
    assert_eq!(
      CliError::conflict_or_rejected("cmd", "busy", None).kind,
      CliErrorKind::ConflictOrRejected
    );
    assert_eq!(
      CliError::unsupported("cmd", "unsupported", None).kind,
      CliErrorKind::UnsupportedOperation
    );
  }

  #[test]
  fn permission_detection_accepts_access_denied_text() {
    let error = anyhow!("Access is denied while opening the socket");

    assert!(error_indicates_permission(&error));
  }

  #[test]
  fn format_error_chain_deduplicates_repeated_messages() {
    let error = anyhow!("broken").context("broken").context("top level");

    assert_eq!(format_error_chain(&error), "top level: broken");
  }

  #[test]
  fn daemon_unavailable_detection_accepts_connection_reset_variants() {
    let error = Error::from(std::io::Error::new(
      std::io::ErrorKind::ConnectionReset,
      "connection reset",
    ));

    assert!(daemon_error_indicates_unavailable(&error));
  }
}
