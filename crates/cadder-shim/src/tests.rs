use super::*;
use cadder_api::{
  CaddyConfigAdapter, CaddyConfigCoordinator, DaemonServer, DaemonState, ProcessRuntime,
};
use cadder_ipc::{
  ProtocolError, ProtocolErrorCode, ProtocolErrorKind, QueryStatePayload, message_types,
};
use clap::CommandFactory;
use std::{fs, path::Path, sync::Mutex as StdMutex};
use tokio::{sync::watch, time::sleep};

static TEST_ENV_LOCK: StdMutex<()> = StdMutex::new(());

#[test]
fn command_metadata_matches_release_identity() {
  let command = ShimArgs::command();

  assert_eq!(command.get_name(), "caddy");
  assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
  assert_eq!(
    command.get_about().map(ToString::to_string),
    Some(env!("CARGO_PKG_DESCRIPTION").to_string())
  );
}

#[test]
fn short_help_uses_package_description() {
  let help = ShimArgs::command().render_help().to_string();

  assert!(
    help.contains(env!("CARGO_PKG_DESCRIPTION")),
    "short help output should include the package description: {help}"
  );
}

#[test]
fn long_help_describes_managed_and_delegated_commands() {
  let help = ShimArgs::command().render_long_help().to_string();

  assert!(
    help.contains("`run` is managed by Cadder"),
    "long help output should describe managed caddy run behavior: {help}"
  );
  assert!(
    help.contains("delegated to the safely resolved real Caddy binary or rejected"),
    "long help output should describe command policy behavior: {help}"
  );
}

#[test]
fn shim_privilege_warning_text_is_available_for_elevated_managed_runs() {
  let _guard = TEST_ENV_LOCK.lock().unwrap();
  unsafe { env::set_var("CADDER_TEST_ELEVATED_CONTEXT", "elevated") };

  let message = shim_privilege_warning_text().unwrap();

  unsafe { env::remove_var("CADDER_TEST_ELEVATED_CONTEXT") };

  assert!(message.contains("caddy shim is running with elevated privileges"));
  assert!(message.contains("user that owns the Cadder runtime"));
}

#[tokio::test]
async fn typed_error_managed_backend_unavailable_message_explains_manual_start() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let error = CadderSession::connect(&paths).await.unwrap_err();

  let message = managed_backend_unavailable_message(&paths, &error);

  assert!(message.contains("Cadder backend `cadderd` is not running"));
  assert!(message.contains("Next: Start `cadderd`"));
  assert!(message.contains("retry `caddy run`"));
  assert!(
    message
      .find("Next:")
      .expect("message should include recovery")
      < message
        .find("Details:")
        .expect("message should place diagnostics after recovery")
  );
}

#[tokio::test]
async fn typed_error_read_only_inspection_uses_matching_recovery() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let args = ["version".to_string()];
  let command = classify_caddy_command(&args);
  let unavailable = CadderSession::connect(&paths).await.unwrap_err();

  let message = read_only_real_caddy_inspection_message(&paths, command, &unavailable);

  assert!(message.contains("cadderd` is not running"));
  assert!(message.contains("read-only `caddy version`"));
  assert!(message.contains("real-Caddy inspection, not Cadder runtime state"));
  assert!(message.contains("Start `cadderd`"));
  assert!(!message.lines().next().unwrap().contains("os error"));
  assert!(
    message
      .find("Next:")
      .expect("message should include recovery")
      < message
        .find("Details:")
        .expect("message should place diagnostics after recovery")
  );

  let permission = IpcClientError::Daemon(ProtocolError::access_denied(
    message_types::QUERY_STATE_REQUEST,
    "Access is denied.",
    Some("Use the account that owns this runtime.".to_string()),
  ));
  let permission_message = read_only_real_caddy_inspection_message(&paths, command, &permission);
  assert!(permission_message.contains("Next: Use the account that owns this runtime."));
  assert!(!permission_message.contains("Start `cadderd`"));
  assert!(
    !permission_message
      .lines()
      .next()
      .unwrap()
      .contains("os error")
  );
}

#[test]
fn typed_error_backend_helpers_classify_transport_and_protocol_failures() {
  let paths =
    RuntimePaths::resolve(Some(std::env::temp_dir().join("cadder-shim-protocol-test"))).unwrap();
  let protocol_error = ipc_error(
    ProtocolErrorKind::IncompatibleProtocolVersion,
    "incompatible_protocol",
    "protocol mismatch",
  );
  let message = managed_backend_unavailable_message(&paths, &protocol_error);

  assert!(message.contains("could not attach `caddy run`"));
  assert!(message.contains("protocol mismatch"));
  assert!(message.contains("\nNext: Inspect the Cadder daemon diagnostics"));
  assert!(!message.contains("Start `cadderd`"));

  let permission = IpcClientError::Daemon(ProtocolError::access_denied(
    message_types::QUERY_STATE_REQUEST,
    "Access is denied.",
    Some("Use the account that owns this runtime.".to_string()),
  ));
  let permission_message = managed_backend_unavailable_message(&paths, &permission);
  assert!(permission_message.contains("Next: Use the account that owns this runtime."));
  assert!(!permission_message.contains("Start `cadderd`"));
  let unavailable = ipc_error(
    ProtocolErrorKind::Internal,
    "daemon_unavailable",
    "backend unavailable",
  );
  assert!(!daemon_error_indicates_not_running(&unavailable));

  let duplicated = Err::<(), _>(anyhow!("same")).context("same").unwrap_err();
  assert_eq!(format_error_chain(&duplicated), "same");
  let nested = Err::<(), _>(anyhow!("inner")).context("outer").unwrap_err();
  assert_eq!(format_error_chain(&nested), "outer: inner");
}

#[tokio::test]
async fn typed_error_recovery_message_uses_the_recovery_failure_guidance() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let attach_error = CadderSession::connect(&paths).await.unwrap_err();
  let start_error = IpcClientError::Daemon(ProtocolError::new(
    ProtocolErrorKind::InvalidInput,
    ProtocolErrorCode::parse("invalid_input").unwrap(),
    "The daemon launch configuration is invalid.",
    Some("Correct the daemon launch configuration, then retry.".into()),
    false,
  ));

  let message = managed_recovery_failed_message(
    &paths,
    &attach_error,
    ManagedRecoveryStage::DaemonStart,
    &start_error,
  );

  assert!(message.contains("Next: Correct the daemon launch configuration, then retry."));
  assert!(!message.contains("cadder daemon start"));

  let post_start_message = managed_recovery_failed_message(
    &paths,
    &attach_error,
    ManagedRecoveryStage::PostStartAttach,
    &attach_error,
  );
  assert!(post_start_message.contains("foreground diagnostic mode"));
}

fn ipc_error(kind: ProtocolErrorKind, code: &str, message: &str) -> IpcClientError {
  IpcClientError::Daemon(ProtocolError::new(
    kind,
    ProtocolErrorCode::parse(code).unwrap(),
    message,
    None,
    false,
  ))
}

#[tokio::test]
async fn run_managed_returns_failure_when_backend_is_unavailable() {
  let runtime_dir = std::env::temp_dir().join(format!(
    "cadder-shim-missing-backend-{}",
    std::process::id()
  ));
  let missing_daemon = runtime_dir.join(fake_daemon_name_for_test());
  let code = run_managed(ShimArgs {
    daemon_path: Some(missing_daemon),
    rejected_real_caddy_selector: None,
    caddy_backend: Some(CaddyBackendMode::Mock),
    caddy_args: vec!["run".to_string()],
    test_runtime_dir: Some(runtime_dir),
  })
  .await
  .unwrap();

  assert_eq!(code, ExitCode::FAILURE);
}

#[tokio::test]
async fn run_managed_does_not_delegate_to_real_caddy_when_backend_is_missing() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let fake_caddy = temp.path().join(fake_caddy_name_for_test());
  write_fake_caddy(&fake_caddy);

  let code = run_managed(ShimArgs {
    daemon_path: Some(temp.path().join(fake_daemon_name_for_test())),
    rejected_real_caddy_selector: Some(fake_caddy.display().to_string()),
    caddy_backend: None,
    caddy_args: vec!["run".to_string()],
    test_runtime_dir: Some(paths.runtime_dir().to_path_buf()),
  })
  .await
  .unwrap();

  assert_eq!(code, ExitCode::FAILURE);
}

#[tokio::test]
async fn open_managed_run_target_starts_missing_daemon_when_fallback_is_skipped() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  paths.ensure_dirs().unwrap();
  let state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths.clone()));
  let server = DaemonServer::new(paths.clone(), state);
  let (shutdown_tx, shutdown_rx) = watch::channel(false);
  let args = ShimArgs {
    daemon_path: Some(temp.path().join(fake_daemon_name_for_test())),
    rejected_real_caddy_selector: None,
    caddy_backend: Some(CaddyBackendMode::Mock),
    caddy_args: vec!["run".to_string()],
    test_runtime_dir: Some(paths.runtime_dir().to_path_buf()),
  };
  let starter = move |_args: ShimArgs, paths: RuntimePaths| async move {
    tokio::spawn(async move {
      let _ = server.run_until(shutdown_rx).await;
    });
    wait_for_backend(&paths).await;
    Ok(())
  };

  let target = open_managed_run_target_with_starter(&args, &paths, starter)
    .await
    .unwrap();

  match target {
    ManagedRunTarget::Cadder(session) => {
      let response: cadder_ipc::QueryStateResponse = session
        .lock()
        .await
        .request(new_request_id("test-query"), &QueryStatePayload::default())
        .await
        .unwrap();
      assert!(response.accepted);
    }
    ManagedRunTarget::Exit(code) => panic!("expected Cadder session, got exit code {code:?}"),
  }
  let _ = shutdown_tx.send(true);
}

#[tokio::test]
async fn typed_error_managed_run_reports_complete_recovery_failure() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let args = ShimArgs {
    daemon_path: Some(temp.path().join(fake_daemon_name_for_test())),
    rejected_real_caddy_selector: Some("definitely-missing-caddy-binary".to_string()),
    caddy_backend: None,
    caddy_args: vec!["run".to_string()],
    test_runtime_dir: Some(paths.runtime_dir().to_path_buf()),
  };
  let starter = |_args: ShimArgs, _paths: RuntimePaths| async {
    Err(ipc_error(
      ProtocolErrorKind::Internal,
      "daemon_start_failed",
      "Test daemon start failed.",
    ))
  };

  let target = open_managed_run_target_with_starter(&args, &paths, starter)
    .await
    .unwrap();

  match target {
    ManagedRunTarget::Exit(code) => assert_eq!(code, ExitCode::FAILURE),
    ManagedRunTarget::Cadder(_) => panic!("expected recovery failure"),
  }
}

#[tokio::test]
async fn mock_caddy_command_handles_adapt_without_delegating_to_real_caddy() {
  let code = run_mock_caddy_command(&["adapt".to_string()])
    .await
    .unwrap();

  assert_eq!(code, ExitCode::SUCCESS);
}

#[tokio::test]
async fn heartbeat_stop_waits_for_in_flight_request_before_unregister() {
  let events = Arc::new(Mutex::new(Vec::new()));
  let (heartbeat_started_tx, heartbeat_started_rx) = oneshot::channel();
  let (heartbeat_release_tx, heartbeat_release_rx) = oneshot::channel();
  let mut heartbeat_started_tx = Some(heartbeat_started_tx);
  let mut heartbeat_release_rx = Some(heartbeat_release_rx);
  let heartbeat_events = events.clone();
  let (heartbeat_stop_tx, heartbeat_stop_rx) = oneshot::channel();
  let heartbeat = tokio::spawn(run_heartbeat_loop(heartbeat_stop_rx, move || {
    let started = heartbeat_started_tx
      .take()
      .expect("the test heartbeat should start once");
    let release = heartbeat_release_rx
      .take()
      .expect("the test heartbeat should finish once");
    let events = heartbeat_events.clone();
    async move {
      events.lock().await.push("heartbeat-started");
      started.send(()).unwrap();
      release.await.unwrap();
      events.lock().await.push("heartbeat-finished");
    }
  }));

  heartbeat_started_rx.await.unwrap();
  let unregister_events = events.clone();
  let shutdown = tokio::spawn(async move {
    stop_heartbeat(heartbeat_stop_tx, heartbeat).await.unwrap();
    unregister_events.lock().await.push("unregister");
  });
  tokio::task::yield_now().await;

  assert!(!shutdown.is_finished());
  assert_eq!(*events.lock().await, ["heartbeat-started"]);

  heartbeat_release_tx.send(()).unwrap();
  shutdown.await.unwrap();

  assert_eq!(
    *events.lock().await,
    ["heartbeat-started", "heartbeat-finished", "unregister"]
  );
}

#[tokio::test]
async fn run_managed_registers_heartbeats_and_unregisters_on_shutdown() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("run"))).unwrap();
  paths.ensure_dirs().unwrap();
  let fake_caddy = temp.path().join(fake_caddy_name_for_test());
  write_fake_caddy(&fake_caddy);
  fs::write(
    temp.path().join("Caddyfile"),
    "app.localhost { respond ok }",
  )
  .unwrap();
  let resolver = RealCaddyResolver::for_test_fixture(fake_caddy);
  let state = DaemonState::new(CaddyConfigCoordinator::new(
    CaddyConfigAdapter::new(resolver.clone()),
    ProcessRuntime::new(resolver, paths.clone()),
  ));
  let server = DaemonServer::new(paths.clone(), state.clone());
  let (shutdown_tx, shutdown_rx) = watch::channel(false);
  tokio::spawn(async move {
    let _ = server.run_until(shutdown_rx).await;
  });
  wait_for_backend(&paths).await;
  let config_path = temp.path().join("Caddyfile");

  let code = run_managed_until(
    ShimArgs {
      daemon_path: None,
      rejected_real_caddy_selector: None,
      caddy_backend: None,
      caddy_args: vec![
        "run".to_string(),
        "--config".to_string(),
        config_path.display().to_string(),
        "--adapter".to_string(),
        "caddyfile".to_string(),
      ],
      test_runtime_dir: Some(paths.runtime_dir().to_path_buf()),
    },
    async { Ok(()) },
  )
  .await
  .unwrap();
  let snapshot = state.snapshot().await;

  assert_eq!(code, ExitCode::SUCCESS);
  assert!(snapshot.registrations.is_empty());
  assert_eq!(snapshot.config.status, cadder_ipc::ConfigApplyStatus::Idle);
  let _ = shutdown_tx.send(true);
}

async fn wait_for_backend(paths: &RuntimePaths) {
  for _ in 0..50 {
    if CadderSession::connect(paths).await.is_ok() {
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  panic!("backend did not become ready");
}

#[cfg(windows)]
fn fake_caddy_name_for_test() -> &'static str {
  "fake-caddy.cmd"
}

#[cfg(not(windows))]
fn fake_caddy_name_for_test() -> &'static str {
  "fake-caddy"
}

#[cfg(windows)]
fn fake_daemon_name_for_test() -> &'static str {
  "missing-cadderd.exe"
}

#[cfg(not(windows))]
fn fake_daemon_name_for_test() -> &'static str {
  "missing-cadderd"
}

fn write_fake_caddy(path: &Path) {
  #[cfg(windows)]
  fs::write(
    path,
    r#"@echo off
if "%1"=="adapt" (
echo {"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["app.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}
exit /b 0
)
if "%1"=="run" (
ping -n 2 127.0.0.1 >nul
exit /b 0
)
if "%1"=="stop" (
exit /b 0
)
if "%1"=="reload" (
exit /b 0
)
exit /b 0
"#,
  )
  .unwrap();

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::write(
      path,
      r#"#!/usr/bin/env sh
if [ "$1" = "adapt" ]; then
printf '%s\n' '{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["app.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}'
exit 0
fi
if [ "$1" = "run" ]; then
sleep 1
exit 0
fi
exit 0
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
  }
}
