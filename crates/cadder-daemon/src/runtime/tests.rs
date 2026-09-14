use super::*;
use crate::logs::LogQuery;
use std::{fs as std_fs, path::Path};
use tokio::time::sleep;

#[derive(Debug, Clone, Copy)]
enum FakeRuntimeMode {
  LongRunning,
  DelayedConfigRead,
  ShortRun,
  FailRun,
  FailReload,
  NeverReady,
  FailStop,
  SlowStop,
}

struct RuntimeFixture {
  runtime: ProcessRuntime,
  logs: CaddyLogStore,
  command_log: PathBuf,
  run_exit_file: PathBuf,
  _temp: tempfile::TempDir,
}

fn short_timeouts() -> RuntimeTimeouts {
  RuntimeTimeouts {
    start_check: Duration::from_millis(250),
    reload: Duration::from_secs(8),
    graceful_stop: Duration::from_secs(3),
    stop_wait: Duration::from_secs(1),
    kill_wait: Duration::from_secs(1),
  }
}

fn runtime_fixture(mode: FakeRuntimeMode) -> RuntimeFixture {
  let temp = tempfile::tempdir().unwrap();
  let runtime_dir = temp.path().join("run");
  let paths = RuntimePaths::resolve(Some(runtime_dir)).unwrap();
  paths.ensure_dirs().unwrap();
  let command_log = temp.path().join("fake-caddy.log");
  let run_exit_file = temp.path().join("fake-caddy.exit");
  let fake_caddy = write_fake_caddy(temp.path(), &command_log, mode);
  let resolver = RealCaddyResolver::for_test_fixture(fake_caddy);
  let runtime = ProcessRuntime::with_timeouts(resolver, paths, short_timeouts());

  RuntimeFixture {
    runtime,
    logs: CaddyLogStore::default(),
    command_log,
    run_exit_file,
    _temp: temp,
  }
}

#[test]
fn default_timeouts_keep_runtime_operations_bounded() {
  let timeouts = RuntimeTimeouts::default();

  assert_eq!(timeouts.start_check, Duration::from_millis(250));
  assert_eq!(timeouts.reload, Duration::from_secs(30));
  assert_eq!(timeouts.graceful_stop, Duration::from_secs(3));
  assert_eq!(timeouts.stop_wait, Duration::from_secs(3));
  assert_eq!(timeouts.kill_wait, Duration::from_secs(4));
  assert_eq!(
    timeouts.graceful_stop + timeouts.stop_wait + timeouts.kill_wait,
    Duration::from_secs(10)
  );
  assert!(format!("{:?}", timeouts).contains("RuntimeTimeouts"));
}

#[test]
fn runtime_stop_deadlines_do_not_expand_short_phase_budgets_to_the_overall_deadline() {
  let started = Instant::now();
  let overall = started + Duration::from_secs(10);
  let deadlines = RuntimeStopDeadlines::new(overall, short_timeouts());

  let graceful = deadlines.graceful.saturating_duration_since(started);
  let stop = deadlines.stop.saturating_duration_since(started);
  let kill = deadlines.kill.saturating_duration_since(started);

  assert!(graceful <= Duration::from_millis(3_100));
  assert!(graceful >= Duration::from_millis(2_900));
  assert!(stop <= Duration::from_millis(4_100));
  assert!(stop >= Duration::from_millis(3_900));
  assert!(kill <= Duration::from_millis(5_100));
  assert!(kill >= Duration::from_millis(4_900));
  assert!(deadlines.kill < overall);
}

#[test]
fn process_runtime_clone_and_debug_keep_shared_child_contract() {
  let fixture = runtime_fixture(FakeRuntimeMode::LongRunning);
  let cloned = fixture.runtime.clone();

  assert!(format!("{:?}", fixture.runtime).contains("ProcessRuntime"));
  assert!(Arc::ptr_eq(&fixture.runtime.child, &cloned.child));
}

#[tokio::test]
async fn inspect_idle_runtime_has_no_stale_process_metadata() {
  let fixture = runtime_fixture(FakeRuntimeMode::LongRunning);

  let state = fixture.runtime.inspect().await;

  assert_eq!(state.status, RuntimeStatus::Idle);
  assert!(state.process_id.is_none());
  assert!(state.admin_endpoint.is_none());
  assert!(state.diagnostics.is_empty());
}

#[tokio::test]
async fn apply_config_starts_and_stops_running_runtime() {
  let fixture = runtime_fixture(FakeRuntimeMode::LongRunning);

  fixture
    .runtime
    .apply_config(br#"{"apps":{}}"#, &fixture.logs)
    .await
    .unwrap();
  wait_for_command_log(&fixture.command_log, "run").await;
  let state = fixture.runtime.inspect().await;

  assert_eq!(state.status, RuntimeStatus::Running);
  assert!(state.process_id.is_some());
  fixture.runtime.stop().await.unwrap();
  wait_for_command_log(&fixture.command_log, "stop").await;
}

#[tokio::test]
async fn apply_config_keeps_candidate_available_until_a_delayed_runtime_accepts_it() {
  let fixture = runtime_fixture(FakeRuntimeMode::DelayedConfigRead);
  let expected = br#"{"apps":{}}"#;

  fixture
    .runtime
    .apply_config(expected, &fixture.logs)
    .await
    .unwrap();

  wait_for_command_log(&fixture.command_log, "config-read").await;
  assert_eq!(
    std_fs::read(fixture.runtime.paths.effective_config_path()).unwrap(),
    expected
  );
  assert!(!has_runtime_config_candidate(
    fixture.runtime.paths.runtime_dir()
  ));
  fixture.runtime.stop().await.unwrap();
}

#[tokio::test]
async fn apply_config_reloads_running_runtime_and_records_control_log() {
  let fixture = runtime_fixture(FakeRuntimeMode::LongRunning);
  fixture
    .runtime
    .apply_config(br#"{"apps":{}}"#, &fixture.logs)
    .await
    .unwrap();
  wait_for_command_log(&fixture.command_log, "run").await;

  fixture
    .runtime
    .apply_config(br#"{"apps":{"http":{}}}"#, &fixture.logs)
    .await
    .unwrap();
  wait_for_command_log(&fixture.command_log, "reload").await;
  let logs = fixture
    .logs
    .query(
      LogQuery {
        stream: LogStreamIdentity::runtime_control(),
        limit: 20,
      },
      true,
    )
    .await;

  assert!(logs.entries.iter().any(|entry| {
    entry.operation.as_deref() == Some("reload")
      && entry.raw_message.contains("real Caddy runtime reloaded")
  }));
  fixture.runtime.stop().await.unwrap();
}

#[tokio::test]
async fn apply_config_reports_reload_failure_for_running_runtime() {
  let fixture = runtime_fixture(FakeRuntimeMode::FailReload);
  let initial_config = br#"{"apps":{}}"#;
  fixture
    .runtime
    .apply_config(initial_config, &fixture.logs)
    .await
    .unwrap();
  wait_for_command_log(&fixture.command_log, "run").await;

  let error = fixture
    .runtime
    .apply_config(br#"{"apps":{"http":{}}}"#, &fixture.logs)
    .await
    .unwrap_err();

  assert!(error.to_string().contains("caddy reload failed"));
  assert_eq!(
    std_fs::read(fixture.runtime.paths.effective_config_path()).unwrap(),
    initial_config
  );
  assert!(!has_runtime_config_candidate(
    fixture.runtime.paths.runtime_dir()
  ));
  wait_for_command_count(&fixture.command_log, "reload", 2).await;
  wait_for_command_log(&fixture.command_log, "stop").await;
  assert_eq!(fixture.runtime.inspect().await.status, RuntimeStatus::Idle);
}

#[tokio::test]
async fn apply_config_reports_start_failure_without_publishing_candidate() {
  let fixture = runtime_fixture(FakeRuntimeMode::FailRun);
  let effective_path = fixture.runtime.paths.effective_config_path();
  std_fs::write(&effective_path, b"previous").unwrap();

  let error = fixture
    .runtime
    .apply_config(br#"{"apps":{}}"#, &fixture.logs)
    .await
    .unwrap_err();

  assert!(error.to_string().contains("exited immediately"));
  assert_eq!(std_fs::read(effective_path).unwrap(), b"previous");
  assert!(!has_runtime_config_candidate(
    fixture.runtime.paths.runtime_dir()
  ));
}

#[tokio::test]
async fn apply_config_times_out_and_cleans_up_a_runtime_that_never_accepts_config() {
  let mut fixture = runtime_fixture(FakeRuntimeMode::NeverReady);
  fixture.runtime.timeouts.reload = Duration::from_millis(500);

  let error = fixture
    .runtime
    .apply_config(br#"{"apps":{}}"#, &fixture.logs)
    .await
    .unwrap_err();

  assert!(
    format!("{error:#}").contains("did not accept its initial configuration"),
    "{error:#}"
  );
  assert_eq!(fixture.runtime.inspect().await.status, RuntimeStatus::Idle);
  assert!(!has_runtime_config_candidate(
    fixture.runtime.paths.runtime_dir()
  ));
}

#[tokio::test]
async fn stop_receipt_accepts_only_at_finalization() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let runtime = CaddyRuntime::Mock(MockCaddyRuntime::new(paths.clone()));
  let logs = CaddyLogStore::default();
  let initial_config = br#"{"apps":{}}"#;
  runtime.apply_config(initial_config, &logs).await.unwrap();

  let attempt = runtime.begin_stop(&logs).await.unwrap();
  let (mut receipt, outcome) = attempt.into_parts();

  outcome.unwrap();
  assert_eq!(
    std_fs::read(paths.effective_config_path()).unwrap(),
    initial_config
  );
  receipt.accept().unwrap();
  assert_eq!(runtime.inspect().await.status, RuntimeStatus::Idle);
  assert!(!paths.effective_config_path().exists());
}

#[tokio::test]
async fn stop_receipt_rollback_restores_previous_mock_runtime() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let runtime = CaddyRuntime::Mock(MockCaddyRuntime::new(paths.clone()));
  let logs = CaddyLogStore::default();
  let initial_config = br#"{"apps":{"http":{}}}"#;
  runtime.apply_config(initial_config, &logs).await.unwrap();

  let attempt = runtime.begin_stop(&logs).await.unwrap();
  let (receipt, outcome) = attempt.into_parts();
  outcome.unwrap();
  receipt.rollback(&logs).await.unwrap();

  assert_eq!(runtime.inspect().await.status, RuntimeStatus::Running);
  assert_eq!(
    std_fs::read(paths.effective_config_path()).unwrap(),
    initial_config
  );
}

#[tokio::test]
async fn stop_receipt_rollback_restarts_previous_process_config() {
  let fixture = runtime_fixture(FakeRuntimeMode::LongRunning);
  let initial_config = br#"{"apps":{"http":{}}}"#;
  fixture
    .runtime
    .apply_config(initial_config, &fixture.logs)
    .await
    .unwrap();
  wait_for_command_log(&fixture.command_log, "run").await;

  let (receipt, outcome) = fixture.runtime.begin_stop(&fixture.logs).await.unwrap();
  outcome.unwrap();
  wait_for_command_log(&fixture.command_log, "stop").await;
  let _ = std_fs::remove_file(fixture._temp.path().join("fake-caddy.stop"));
  receipt.rollback(&fixture.logs).await.unwrap();

  wait_for_command_count(&fixture.command_log, "run", 2).await;
  assert_eq!(
    fixture.runtime.inspect().await.status,
    RuntimeStatus::Running
  );
  assert_eq!(
    std_fs::read(fixture.runtime.paths.effective_config_path()).unwrap(),
    initial_config
  );
  fixture.runtime.stop().await.unwrap();
}

#[tokio::test]
async fn begin_stop_keeps_running_process_without_effective_rollback_config() {
  let fixture = runtime_fixture(FakeRuntimeMode::LongRunning);
  let transient_config = fixture._temp.path().join("transient-config.json");
  std_fs::write(&transient_config, br#"{"apps":{}}"#).unwrap();
  fixture
    .runtime
    .start(&transient_config, &fixture.logs)
    .await
    .unwrap();
  wait_for_command_log(&fixture.command_log, "run").await;

  let error = fixture.runtime.begin_stop(&fixture.logs).await.unwrap_err();

  assert!(error.to_string().contains("effective config for rollback"));
  assert_eq!(
    fixture.runtime.inspect().await.status,
    RuntimeStatus::Running
  );
  assert!(!command_log_contains(&fixture.command_log, "stop"));
  fixture.runtime.force_stop().await.unwrap();
}

#[tokio::test]
async fn begin_stop_preserves_owned_child_when_liveness_is_unknown() {
  let fixture = runtime_fixture(FakeRuntimeMode::LongRunning);
  let initial_config = br#"{"apps":{}}"#;
  fixture
    .runtime
    .apply_config(initial_config, &fixture.logs)
    .await
    .unwrap();
  wait_for_command_log(&fixture.command_log, "run").await;
  fixture.runtime.force_inspect_failure_for_test(true);

  let inspected = fixture.runtime.inspect().await;
  assert_eq!(inspected.status, RuntimeStatus::Unhealthy);
  assert_eq!(inspected.diagnostics[0].code, "runtime-inspect-failed");
  let duplicate_start = fixture
    .runtime
    .start(
      &fixture.runtime.paths.effective_config_path(),
      &fixture.logs,
    )
    .await
    .unwrap_err();
  assert!(duplicate_start.to_string().contains("already owns"));

  let error = fixture.runtime.begin_stop(&fixture.logs).await.unwrap_err();

  assert!(error.to_string().contains("injected"));
  fixture.runtime.force_inspect_failure_for_test(false);
  assert_eq!(
    fixture.runtime.inspect().await.status,
    RuntimeStatus::Running
  );
  assert_eq!(
    std_fs::read(fixture.runtime.paths.effective_config_path()).unwrap(),
    initial_config
  );
  assert!(!command_log_contains(&fixture.command_log, "stop"));
  fixture.runtime.stop().await.unwrap();
}

#[tokio::test]
async fn promotion_failure_stops_a_newly_started_runtime_during_rollback() {
  let fixture = runtime_fixture(FakeRuntimeMode::LongRunning);
  let attempt = fixture
    .runtime
    .begin_apply_config(br#"{"apps":{}}"#, &fixture.logs)
    .await
    .unwrap();
  let (mut receipt, outcome) = attempt;
  outcome.unwrap();
  wait_for_command_log(&fixture.command_log, "run").await;
  std_fs::create_dir(fixture.runtime.paths.effective_config_path()).unwrap();

  let promotion_error = receipt.accept().unwrap_err();
  let rollback_error = receipt.rollback(&fixture.logs).await.unwrap_err();

  assert!(promotion_error.to_string().contains("promote staged"));
  assert!(
    rollback_error
      .to_string()
      .contains("effective Caddy config")
  );
  assert_eq!(fixture.runtime.inspect().await.status, RuntimeStatus::Idle);
  wait_for_command_log(&fixture.command_log, "stop").await;
}

#[tokio::test]
async fn apply_config_restarts_after_child_exit_without_prior_inspect() {
  let fixture = runtime_fixture(FakeRuntimeMode::ShortRun);
  fixture
    .runtime
    .apply_config(br#"{"apps":{}}"#, &fixture.logs)
    .await
    .unwrap();
  std_fs::write(&fixture.run_exit_file, b"exit").unwrap();
  wait_for_command_log(&fixture.command_log, "run-exited").await;
  wait_for_runtime_child_exit(&fixture.runtime).await;

  fixture
    .runtime
    .apply_config(br#"{"apps":{"tls":{}}}"#, &fixture.logs)
    .await
    .unwrap();

  wait_for_command_count(&fixture.command_log, "run", 2).await;
  fixture.runtime.stop().await.unwrap();
  wait_for_command_log(&fixture.command_log, "stop").await;
}

#[tokio::test]
async fn stop_times_out_slow_graceful_stop_and_kills_child() {
  let fixture = runtime_fixture(FakeRuntimeMode::SlowStop);
  fixture
    .runtime
    .apply_config(br#"{"apps":{}}"#, &fixture.logs)
    .await
    .unwrap();
  wait_for_command_log(&fixture.command_log, "run").await;

  let error = fixture.runtime.stop().await.unwrap_err();

  assert!(error.to_string().contains("caddy stop timed out"));
  wait_for_command_log(&fixture.command_log, "stop").await;
}

#[tokio::test]
async fn shutdown_coordinator_bounds_owned_runtime_stop_and_joins_the_child() {
  let mut fixture = runtime_fixture(FakeRuntimeMode::SlowStop);
  fixture.runtime.timeouts = RuntimeTimeouts {
    start_check: Duration::from_millis(250),
    reload: Duration::from_secs(30),
    graceful_stop: Duration::from_secs(60),
    stop_wait: Duration::from_secs(60),
    kill_wait: Duration::from_secs(60),
  };
  fixture
    .runtime
    .apply_config(br#"{"apps":{}}"#, &fixture.logs)
    .await
    .unwrap();
  wait_for_command_log(&fixture.command_log, "run").await;
  let budget = Duration::from_secs(4);
  let started = Instant::now();

  let outcome = fixture.runtime.stop_until(started + budget).await;
  let (result, quiescent) = outcome.into_parts();

  assert!(result.is_err());
  assert!(quiescent);
  assert!(started.elapsed() <= budget + Duration::from_millis(250));
  assert_eq!(fixture.runtime.inspect().await.status, RuntimeStatus::Idle);
  assert_eq!(fixture.runtime.log_tasks.len(), 0);
}

#[tokio::test]
async fn cancelling_stop_until_retains_child_for_explicit_force_stop() {
  let fixture = runtime_fixture(FakeRuntimeMode::SlowStop);
  fixture
    .runtime
    .apply_config(br#"{"apps":{}}"#, &fixture.logs)
    .await
    .unwrap();
  wait_for_command_log(&fixture.command_log, "run").await;
  let runtime = fixture.runtime.clone();
  let stop = tokio::spawn(async move {
    runtime
      .stop_until(Instant::now() + Duration::from_secs(10))
      .await
  });
  sleep(Duration::from_millis(100)).await;

  stop.abort();
  assert!(stop.await.unwrap_err().is_cancelled());
  assert!(fixture.runtime.child.lock().await.is_some());

  fixture.runtime.force_stop().await.unwrap();
  assert_eq!(fixture.runtime.inspect().await.status, RuntimeStatus::Idle);
  assert_eq!(fixture.runtime.log_tasks.len(), 0);
}

#[tokio::test]
async fn request_graceful_stop_times_out_the_stop_command() {
  let temp = tempfile::tempdir().unwrap();
  let command_log = temp.path().join("fake-caddy.log");
  let fake_caddy = write_fake_caddy(temp.path(), &command_log, FakeRuntimeMode::SlowStop);
  let image = RealCaddyResolver::for_test_fixture(fake_caddy)
    .verify_for_spawn()
    .await
    .unwrap();

  let started = Instant::now();
  let error = request_graceful_stop_until(
    image,
    started + Duration::from_secs(2),
    started + Duration::from_secs(3),
  )
  .await
  .unwrap_err();

  assert!(error.to_string().contains("caddy stop timed out"));
}

#[tokio::test]
async fn request_graceful_stop_reports_failed_stop_status() {
  let temp = tempfile::tempdir().unwrap();
  let command_log = temp.path().join("fake-caddy.log");
  let fake_caddy = write_fake_caddy(temp.path(), &command_log, FakeRuntimeMode::FailStop);
  let image = RealCaddyResolver::for_test_fixture(fake_caddy)
    .verify_for_spawn()
    .await
    .unwrap();

  let started = Instant::now();
  let error = request_graceful_stop_until(
    image,
    started + Duration::from_secs(10),
    started + Duration::from_secs(12),
  )
  .await
  .unwrap_err();

  assert!(error.to_string().contains("caddy stop failed"));
  wait_for_command_log(&command_log, "stop").await;
}

async fn wait_for_command_log(path: &Path, command: &str) {
  for _ in 0..500 {
    let log = std_fs::read_to_string(path).unwrap_or_default();
    if log.lines().any(|line| line.starts_with(command)) {
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  let log = std_fs::read_to_string(path).unwrap_or_default();
  panic!("expected command `{command}` in fake Caddy log:\n{log}");
}

fn command_log_contains(path: &Path, command: &str) -> bool {
  std_fs::read_to_string(path).is_ok_and(|log| log.lines().any(|line| line.trim() == command))
}

async fn wait_for_command_count(path: &Path, command: &str, expected: usize) {
  for _ in 0..500 {
    let log = std_fs::read_to_string(path).unwrap_or_default();
    let count = log
      .lines()
      .filter(|line| line.split_whitespace().next() == Some(command))
      .count();
    if count >= expected {
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  let log = std_fs::read_to_string(path).unwrap_or_default();
  panic!("expected {expected} `{command}` commands in fake Caddy log:\n{log}");
}

async fn wait_for_runtime_child_exit(runtime: &ProcessRuntime) {
  for _ in 0..500 {
    let has_exited = {
      let mut child = runtime.child.lock().await;
      child
        .as_mut()
        .is_none_or(|owned| owned.child.try_wait().unwrap().is_some())
    };
    if has_exited {
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  panic!("expected fake Caddy runtime child to exit");
}

fn has_runtime_config_candidate(runtime_dir: &Path) -> bool {
  std_fs::read_dir(runtime_dir)
    .unwrap()
    .map(|entry| entry.unwrap().path())
    .any(|path| {
      path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(".effective-caddy.") && name.ends_with(".tmp"))
    })
}

fn write_fake_caddy(dir: &Path, command_log: &Path, mode: FakeRuntimeMode) -> PathBuf {
  let stop_file = dir.join("fake-caddy.stop");
  let run_exit_file = dir.join("fake-caddy.exit");
  let config_read_file = dir.join("fake-caddy.config-read");
  let first_reload_file = dir.join("fake-caddy.first-reload");

  #[cfg(windows)]
  {
    let path = dir.join("fake-caddy.cmd");
    let reload_behavior = if matches!(mode, FakeRuntimeMode::FailReload) {
      "if not exist \"{first_reload_file}\" (\r\n    echo ready> \"{first_reload_file}\"\r\n    exit /b 0\r\n  )\r\n  echo reload failed 1>&2\r\n  exit /b 7"
    } else if matches!(mode, FakeRuntimeMode::DelayedConfigRead) {
      "\"%SystemRoot%\\System32\\ping.exe\" -n 3 127.0.0.1 >nul\r\n  if not exist \"{config_read_file}\" exit /b 7\r\n  exit /b 0"
    } else if matches!(mode, FakeRuntimeMode::NeverReady) {
      "echo runtime not ready 1>&2\r\n  exit /b 7"
    } else {
      "exit /b 0"
    };
    let stop_behavior = if matches!(mode, FakeRuntimeMode::SlowStop) {
      "\"%SystemRoot%\\System32\\ping.exe\" -n 8 127.0.0.1 >nul\r\n  echo stop> \"{stop_file}\"\r\n  exit /b 0"
    } else if matches!(mode, FakeRuntimeMode::FailStop) {
      "echo stop> \"{stop_file}\"\r\n  exit /b 7"
    } else {
      "echo stop> \"{stop_file}\"\r\n  exit /b 0"
    };
    let run_behavior = if matches!(mode, FakeRuntimeMode::FailRun) {
      "echo run failed 1>&2\r\n  exit /b 7"
    } else if matches!(mode, FakeRuntimeMode::ShortRun) {
      ":short_run_loop\r\n  if exist \"{stop_file}\" exit /b 0\r\n  if exist \"{run_exit_file}\" goto short_run_exit\r\n  \"%SystemRoot%\\System32\\ping.exe\" -n 2 127.0.0.1 >nul\r\n  goto short_run_loop\r\n  :short_run_exit\r\n  del /q \"{run_exit_file}\"\r\n  echo run-exited>> \"{command_log}\"\r\n  exit /b 0"
    } else if matches!(mode, FakeRuntimeMode::DelayedConfigRead) {
      "\"%SystemRoot%\\System32\\ping.exe\" -n 2 127.0.0.1 >nul\r\n  if not exist \"%3\" (\r\n    echo config missing 1>&2\r\n    exit /b 9\r\n  )\r\n  echo config-read>> \"{command_log}\"\r\n  echo ready> \"{config_read_file}\"\r\n  :run_loop\r\n  if exist \"{stop_file}\" exit /b 0\r\n  \"%SystemRoot%\\System32\\ping.exe\" -n 2 127.0.0.1 >nul\r\n  goto run_loop"
    } else {
      ":run_loop\r\n  if exist \"{stop_file}\" exit /b 0\r\n  \"%SystemRoot%\\System32\\ping.exe\" -n 2 127.0.0.1 >nul\r\n  goto run_loop"
    };
    std_fs::write(
      &path,
      format!(
        r#"@echo off
echo %*>> "{command_log}"
if "%1"=="reload" (
{reload_behavior}
)
if "%1"=="stop" (
{stop_behavior}
)
if "%1"=="run" (
echo fake runtime started
{run_behavior}
)
exit /b 0
"#,
        command_log = command_log.display(),
        reload_behavior = reload_behavior
          .replace(
            "{first_reload_file}",
            &first_reload_file.display().to_string()
          )
          .replace(
            "{config_read_file}",
            &config_read_file.display().to_string()
          ),
        stop_behavior = stop_behavior.replace("{stop_file}", &stop_file.display().to_string()),
        run_behavior = run_behavior
          .replace("{stop_file}", &stop_file.display().to_string())
          .replace("{run_exit_file}", &run_exit_file.display().to_string())
          .replace(
            "{config_read_file}",
            &config_read_file.display().to_string()
          )
          .replace("{command_log}", &command_log.display().to_string()),
      ),
    )
    .unwrap();
    path
  }

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-caddy");
    let reload_behavior = if matches!(mode, FakeRuntimeMode::FailReload) {
      "if [ ! -f '{first_reload_file}' ]; then : > '{first_reload_file}'; exit 0; fi\n  printf '%s\n' 'reload failed' >&2\n  exit 7"
    } else if matches!(mode, FakeRuntimeMode::DelayedConfigRead) {
      "/bin/sleep 0.5\n  [ -f '{config_read_file}' ] || exit 7\n  exit 0"
    } else if matches!(mode, FakeRuntimeMode::NeverReady) {
      "printf '%s\n' 'runtime not ready' >&2\n  exit 7"
    } else {
      "exit 0"
    };
    let stop_behavior = if matches!(mode, FakeRuntimeMode::SlowStop) {
      "sleep 6\n  : > '{stop_file}'\n  exit 0"
    } else if matches!(mode, FakeRuntimeMode::FailStop) {
      ": > '{stop_file}'\n  exit 7"
    } else {
      ": > '{stop_file}'\n  exit 0"
    };
    let run_behavior = if matches!(mode, FakeRuntimeMode::FailRun) {
      "printf '%s\n' 'run failed' >&2\n  exit 7"
    } else if matches!(mode, FakeRuntimeMode::ShortRun) {
      "while [ ! -f '{run_exit_file}' ] && [ ! -f '{stop_file}' ]; do /bin/sleep 0.02; done\n  if [ -f '{run_exit_file}' ]; then /bin/rm -f '{run_exit_file}'; printf '%s\n' 'run-exited' >> '{command_log}'; fi\n  exit 0"
    } else if matches!(mode, FakeRuntimeMode::DelayedConfigRead) {
      "/bin/sleep 0.35\n  [ -f \"$3\" ] || { printf '%s\n' 'config missing' >&2; exit 9; }\n  printf '%s\n' 'config-read' >> '{command_log}'\n  : > '{config_read_file}'\n  while [ ! -f '{stop_file}' ]; do /bin/sleep 0.2; done\n  exit 0"
    } else {
      "while [ ! -f '{stop_file}' ]; do /bin/sleep 0.2; done\n  exit 0"
    };
    std_fs::write(
      &path,
      format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> '{command_log}'
if [ "$1" = "reload" ]; then
{reload_behavior}
fi
if [ "$1" = "stop" ]; then
{stop_behavior}
fi
if [ "$1" = "run" ]; then
echo fake runtime started
{run_behavior}
fi
exit 0
        "#,
        command_log = command_log.display(),
        reload_behavior = reload_behavior
          .replace(
            "{first_reload_file}",
            &first_reload_file.display().to_string()
          )
          .replace(
            "{config_read_file}",
            &config_read_file.display().to_string()
          ),
        stop_behavior = stop_behavior.replace("{stop_file}", &stop_file.display().to_string()),
        run_behavior = run_behavior
          .replace("{stop_file}", &stop_file.display().to_string())
          .replace("{run_exit_file}", &run_exit_file.display().to_string())
          .replace(
            "{config_read_file}",
            &config_read_file.display().to_string()
          )
          .replace("{command_log}", &command_log.display().to_string()),
      ),
    )
    .unwrap();
    let mut permissions = std_fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std_fs::set_permissions(&path, permissions).unwrap();
    path
  }
}
