use super::*;
use anyhow::{Context, anyhow};
use cadder_daemon::{CadderSession, RuntimePaths};
use cadder_ipc::ProtocolErrorCode;

#[test]
fn app_exit_values_remain_stable() {
  assert_eq!(AppExit::Success as u8, 0);
  assert_eq!(AppExit::InvalidUsage as u8, 2);
  assert_eq!(AppExit::DaemonUnavailable as u8, 3);
  assert_eq!(AppExit::DaemonStartFailure as u8, 4);
  assert_eq!(AppExit::TargetNotFound as u8, 5);
  assert_eq!(AppExit::ConflictOrRejected as u8, 6);
  assert_eq!(AppExit::PermissionOrElevation as u8, 7);
  assert_eq!(AppExit::UnsupportedOperation as u8, 8);
  assert_eq!(AppExit::IpcFailure as u8, 9);
  assert_eq!(AppExit::Success.report(), ExitCode::SUCCESS);
  assert_eq!(AppExit::IpcFailure.report(), ExitCode::from(9));
}

#[tokio::test]
async fn typed_error_daemon_unavailable_maps_to_stable_exit_code() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let error = CadderSession::connect(&paths).await.unwrap_err();

  let mapped =
    OperatorError::daemon_request("domains list", Path::new("runtime"), "query domains", error);

  assert_eq!(mapped.kind, AppExit::DaemonUnavailable);
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

  assert_eq!(mapped.kind, AppExit::PermissionOrElevation);
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

  assert_eq!(mapped.kind, AppExit::IpcFailure);
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

  let mapped = OperatorError::daemon_request("status", Path::new("runtime"), "query state", error);

  assert_eq!(mapped.kind, AppExit::UnsupportedOperation);
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

  assert_eq!(mapped.kind, AppExit::DaemonStartFailure);
  assert!(mapped.message.contains("Spawn failed."));
  assert!(!mapped.message.ends_with(".."));
}

#[test]
fn operator_error_constructors_keep_stable_contracts() {
  let unsupported = OperatorError::unsupported(
    "unsupported operation",
    "not supported",
    Some("retry elsewhere".to_string()),
  );
  assert_eq!(unsupported.kind, AppExit::UnsupportedOperation);
  assert_eq!(unsupported.to_string(), "not supported");

  let target = OperatorError::target_not_found("domains enable", "missing", None);
  assert_eq!(target.kind, AppExit::TargetNotFound);

  let conflict = OperatorError::conflict_or_rejected("domains enable", "busy", None);
  assert_eq!(conflict.kind, AppExit::ConflictOrRejected);
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
  assert_eq!(mapped.kind, AppExit::PermissionOrElevation);
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
