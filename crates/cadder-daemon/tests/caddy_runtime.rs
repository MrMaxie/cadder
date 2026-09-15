use cadder_daemon::{
  CaddyConfigAdapter, CaddyConfigCoordinator, CaddyLogStore, ProcessRuntime, RealCaddyResolver,
  RuntimePaths, RuntimeTimeouts,
};
use cadder_ipc::{
  ActivationState, EntrypointInstanceIdentity, EntrypointRegistration, LogStreamIdentity,
  OwnerProcessIdentity, RegisteredDomain, RuntimeStatus, ShimRunMetadata, SourcePath,
};
use chrono::Utc;
use std::{
  fs,
  path::{Path, PathBuf},
  time::Duration,
};
use tokio::time::sleep;

#[tokio::test]
async fn adapter_uses_raw_config_path_and_shim_adapter_metadata() {
  let temp = test_tempdir();
  let command_log = temp.path().join("fake-caddy.log");
  let fake_caddy = write_fake_caddy(temp.path(), FakeMode::LongRunning);
  let config_path = temp.path().join("Caddyfile.json");
  fs::write(&config_path, r#"{"apps":{}}"#).unwrap();
  let adapter = CaddyConfigAdapter::new(RealCaddyResolver::for_test_fixture(fake_caddy));
  let mut registration = registration("adapter", "nonce", &config_path);
  registration.source_working_directory = SourcePath::new(temp.path().display().to_string(), None);
  registration.source_config_path = SourcePath::new(config_path.display().to_string(), None);
  registration.shim_run = Some(ShimRunMetadata {
    adapter: Some("json".to_string()),
    raw_arguments: vec![
      "run".to_string(),
      "--adapter".to_string(),
      "json".to_string(),
    ],
    command_line: "run --adapter json".to_string(),
  });

  let prepared = adapter.prepare(registration).await;
  let command_log = fs::read_to_string(&command_log).unwrap();

  assert!(
    prepared.diagnostics.is_empty(),
    "{:?}",
    prepared.diagnostics
  );
  assert_eq!(
    prepared.registration.registered_domains[0].name.canonical,
    "adapter.localhost"
  );
  assert!(
    command_log.contains("adapt")
      && command_log.contains("--config")
      && command_log.contains(config_path.to_string_lossy().as_ref())
      && command_log.contains("--adapter")
      && command_log.contains("json"),
    "{command_log}"
  );
}

#[tokio::test]
async fn adapter_reports_nonzero_adapt_failures() {
  let temp = test_tempdir();
  let fake_caddy = write_fake_caddy(temp.path(), FakeMode::FailAdapt);
  let config_path = temp.path().join("Caddyfile");
  fs::write(&config_path, "broken.localhost { respond ok }").unwrap();
  let adapter = CaddyConfigAdapter::new(RealCaddyResolver::for_test_fixture(fake_caddy));

  let prepared = adapter
    .prepare(registration("broken", "nonce", &config_path))
    .await;

  assert_eq!(prepared.diagnostics[0].code, "adapt-failed");
  assert!(
    prepared.diagnostics[0]
      .message
      .contains("caddy adapt failed"),
    "{:?}",
    prepared.diagnostics
  );
}

#[tokio::test]
async fn adapter_reports_invalid_adapt_json() {
  let temp = test_tempdir();
  let fake_caddy = write_fake_caddy(temp.path(), FakeMode::InvalidAdaptJson);
  let config_path = temp.path().join("Caddyfile");
  fs::write(&config_path, "invalid-json.localhost { respond ok }").unwrap();
  let adapter = CaddyConfigAdapter::new(RealCaddyResolver::for_test_fixture(fake_caddy));

  let prepared = adapter
    .prepare(registration("invalid-json", "nonce", &config_path))
    .await;

  assert_eq!(prepared.diagnostics[0].code, "adapt-failed");
  assert!(
    prepared.diagnostics[0]
      .message
      .contains("parse adapted Caddy JSON"),
    "{:?}",
    prepared.diagnostics
  );
}

#[tokio::test]
async fn adapter_reports_adapt_timeout() {
  let temp = test_tempdir();
  let fake_caddy = write_fake_caddy(temp.path(), FakeMode::SlowAdapt);
  let config_path = temp.path().join("Caddyfile");
  fs::write(&config_path, "slow.localhost { respond ok }").unwrap();
  let adapter = CaddyConfigAdapter::with_command_timeout(
    RealCaddyResolver::for_test_fixture(fake_caddy),
    Duration::from_millis(50),
  );

  let prepared = adapter
    .prepare(registration("slow", "nonce", &config_path))
    .await;

  assert_eq!(prepared.diagnostics[0].code, "adapt-failed");
  assert!(
    prepared.diagnostics[0].message.contains("timed out"),
    "{:?}",
    prepared.diagnostics
  );
}

#[tokio::test]
async fn process_runtime_starts_reports_running_reloads_and_stops() {
  let temp = test_tempdir();
  let command_log = temp.path().join("fake-caddy.log");
  let fake_caddy = write_fake_caddy(temp.path(), FakeMode::LongRunning);
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  paths.ensure_dirs().unwrap();
  let runtime = ProcessRuntime::with_timeouts(
    RealCaddyResolver::for_test_fixture(fake_caddy),
    paths,
    RuntimeTimeouts {
      start_check: Duration::from_millis(150),
      reload: Duration::from_secs(3),
      graceful_stop: Duration::from_secs(1),
      stop_wait: Duration::from_secs(1),
      kill_wait: Duration::from_secs(1),
    },
  );
  let logs = CaddyLogStore::default();

  runtime
    .apply_config(br#"{"apps":{}}"#, &logs)
    .await
    .unwrap();
  wait_for_command_log(&command_log, "run").await;
  let running = runtime.inspect().await;
  runtime
    .apply_config(br#"{"apps":{"http":{}}}"#, &logs)
    .await
    .unwrap();
  wait_for_command_log(&command_log, "reload").await;
  runtime.stop().await.unwrap();
  wait_for_command_log(&command_log, "stop").await;

  assert_eq!(running.status, RuntimeStatus::Running);
  assert!(running.process_id.is_some());
}

#[tokio::test]
async fn coordinator_apply_tracks_runtime_success_failure_and_idle_stop() {
  let temp = test_tempdir();
  let fake_caddy = write_fake_caddy(temp.path(), FakeMode::FailReload);
  let config_path = temp.path().join("Caddyfile");
  fs::write(&config_path, "coordinator.localhost { respond ok }").unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  paths.ensure_dirs().unwrap();
  let resolver = RealCaddyResolver::for_test_fixture(fake_caddy);
  let runtime = ProcessRuntime::with_timeouts(
    resolver.clone(),
    paths,
    RuntimeTimeouts {
      start_check: Duration::from_millis(150),
      reload: Duration::from_secs(3),
      graceful_stop: Duration::from_secs(1),
      stop_wait: Duration::from_secs(1),
      kill_wait: Duration::from_secs(1),
    },
  );
  let mut coordinator =
    CaddyConfigCoordinator::new(CaddyConfigAdapter::new(resolver), runtime.clone());
  let logs = CaddyLogStore::default();
  let mut active = registration("coordinator", "nonce", &config_path);

  active = coordinator.prepare_registration(active).await;
  let applied = coordinator.apply(&[active.clone()], &logs).await;
  let failed = coordinator.apply(&[active.clone()], &logs).await;
  active.activation_state = ActivationState::Inactive;
  let idle = coordinator.apply(&[active], &logs).await;
  let _ = runtime.stop().await;

  assert_eq!(applied.status, cadder_ipc::ConfigApplyStatus::Applied);
  assert_eq!(failed.status, cadder_ipc::ConfigApplyStatus::Failed);
  assert_eq!(failed.diagnostics[0].code, "runtime-apply-failed");
  assert_eq!(idle.status, cadder_ipc::ConfigApplyStatus::Idle);
}

fn registration(id: &str, nonce: &str, config_path: &Path) -> EntrypointRegistration {
  let now = Utc::now();
  let identity = EntrypointInstanceIdentity {
    instance_id: id.to_string(),
    started_at_utc: now,
    shim_session_nonce: nonce.to_string(),
  };
  EntrypointRegistration {
    registration_id: id.to_string(),
    entrypoint_instance: identity,
    source_working_directory: SourcePath::new(".", None),
    source_config_path: SourcePath::new(
      config_path.display().to_string(),
      Some(config_path.display().to_string()),
    ),
    registered_domains: vec![RegisteredDomain::active(format!("{id}.localhost"))],
    activation_state: ActivationState::Active,
    owner_process: OwnerProcessIdentity {
      process_id: 1,
      process_start_time_utc: now,
      shim_session_nonce: nonce.to_string(),
      executable_path: None,
    },
    log_stream: LogStreamIdentity::entrypoint(id),
    shim_run: Some(ShimRunMetadata {
      adapter: Some("caddyfile".to_string()),
      raw_arguments: vec!["run".to_string()],
      command_line: "run".to_string(),
    }),
    created_at_utc: now,
    last_heartbeat_utc: now,
  }
}

#[derive(Clone, Copy)]
enum FakeMode {
  LongRunning,
  FailReload,
  FailAdapt,
  InvalidAdaptJson,
  SlowAdapt,
}

fn test_tempdir() -> tempfile::TempDir {
  tempfile::Builder::new()
    .prefix("caddy-runtime-")
    .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
    .unwrap()
}

fn write_fake_caddy(dir: &Path, mode: FakeMode) -> PathBuf {
  fs::write(dir.join("cadder-test.mode"), mode.as_str()).unwrap();

  let source = Path::new(env!("CARGO_BIN_EXE_cadder-test-process"));
  let path = dir.join(if cfg!(windows) {
    "fake-caddy.exe"
  } else {
    "fake-caddy"
  });
  fs::hard_link(source, &path).unwrap();
  path
}

impl FakeMode {
  fn as_str(self) -> &'static str {
    match self {
      Self::LongRunning => "adapter-long-running",
      Self::FailReload => "adapter-fail-reload",
      Self::FailAdapt => "adapter-fail-adapt",
      Self::InvalidAdaptJson => "adapter-invalid-adapt-json",
      Self::SlowAdapt => "adapter-slow-adapt",
    }
  }
}

async fn wait_for_command_log(path: &Path, command: &str) {
  for _ in 0..200 {
    let log = fs::read_to_string(path).unwrap_or_default();
    if log.lines().any(|line| line.starts_with(command)) {
      return;
    }
    sleep(Duration::from_millis(20)).await;
  }
  let log = fs::read_to_string(path).unwrap_or_default();
  panic!("expected command `{command}` in fake Caddy log:\n{log}");
}
