use crate::{logs::CaddyLogStore, paths::RuntimePaths, process_tree::ProcessTreeChild};
use anyhow::{Context, Result};
use cadder_protocol::{
  LogAttributionKind, LogSeverity, LogStreamIdentity, RuntimeState, RuntimeStatus,
};
use std::{path::PathBuf, process::Stdio, sync::Arc, time::Duration};
use tokio::{
  fs,
  io::{AsyncBufReadExt, BufReader},
  process::Command,
  sync::Mutex,
  task::yield_now,
  time::timeout,
};

use crate::caddy::RealCaddyResolver;

#[derive(Debug, Clone)]
pub enum CaddyRuntime {
  Real(ProcessRuntime),
  Mock(MockCaddyRuntime),
}

impl CaddyRuntime {
  pub fn real(resolver: RealCaddyResolver, paths: RuntimePaths) -> Self {
    Self::Real(ProcessRuntime::new(resolver, paths))
  }

  pub fn mock(paths: RuntimePaths) -> Self {
    Self::Mock(MockCaddyRuntime::new(paths))
  }

  pub async fn inspect(&self) -> RuntimeState {
    match self {
      Self::Real(runtime) => runtime.inspect().await,
      Self::Mock(runtime) => runtime.inspect().await,
    }
  }

  pub async fn apply_config(&self, rendered: &[u8], logs: &CaddyLogStore) -> Result<()> {
    match self {
      Self::Real(runtime) => runtime.apply_config(rendered, logs).await,
      Self::Mock(runtime) => runtime.apply_config(rendered, logs).await,
    }
  }

  pub async fn stop(&self) -> Result<()> {
    match self {
      Self::Real(runtime) => runtime.stop().await,
      Self::Mock(runtime) => runtime.stop().await,
    }
  }
}

impl From<ProcessRuntime> for CaddyRuntime {
  fn from(runtime: ProcessRuntime) -> Self {
    Self::Real(runtime)
  }
}

#[derive(Debug, Clone)]
pub struct ProcessRuntime {
  resolver: RealCaddyResolver,
  paths: RuntimePaths,
  child: Arc<Mutex<Option<ProcessTreeChild>>>,
  timeouts: RuntimeTimeouts,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeTimeouts {
  pub start_check: Duration,
  pub reload: Duration,
  pub graceful_stop: Duration,
  pub stop_wait: Duration,
  pub kill_wait: Duration,
}

impl Default for RuntimeTimeouts {
  fn default() -> Self {
    Self {
      start_check: Duration::from_millis(250),
      reload: Duration::from_secs(30),
      graceful_stop: Duration::from_secs(5),
      stop_wait: Duration::from_secs(5),
      kill_wait: Duration::from_secs(5),
    }
  }
}

impl ProcessRuntime {
  pub fn new(resolver: RealCaddyResolver, paths: RuntimePaths) -> Self {
    Self::with_timeouts(resolver, paths, RuntimeTimeouts::default())
  }

  pub fn with_timeouts(
    resolver: RealCaddyResolver,
    paths: RuntimePaths,
    timeouts: RuntimeTimeouts,
  ) -> Self {
    Self {
      resolver,
      paths,
      child: Arc::new(Mutex::new(None)),
      timeouts,
    }
  }

  pub async fn inspect(&self) -> RuntimeState {
    let binary_path = self
      .resolver
      .resolve()
      .ok()
      .map(|path| path.display().to_string());
    let mut guard = self.child.lock().await;
    if let Some(child) = guard.as_mut() {
      let process_id = child.id();
      let status = child.try_wait();
      match status {
        Ok(None) => {
          return RuntimeState {
            status: RuntimeStatus::Running,
            binary_path,
            version: None,
            process_id,
            admin_endpoint: Some("localhost:2019".to_string()),
            diagnostics: Vec::new(),
          };
        }
        Ok(Some(status)) => {
          *guard = None;
          return RuntimeState {
            status: RuntimeStatus::Unhealthy,
            binary_path,
            version: None,
            process_id,
            admin_endpoint: Some("localhost:2019".to_string()),
            diagnostics: vec![cadder_protocol::RuntimeDiagnostic {
              code: "runtime-exited".to_string(),
              message: format!("real Caddy runtime exited with status {status}"),
              operation: Some("inspect".to_string()),
            }],
          };
        }
        Err(error) => {
          *guard = None;
          return RuntimeState {
            status: RuntimeStatus::Unhealthy,
            binary_path,
            version: None,
            process_id,
            admin_endpoint: Some("localhost:2019".to_string()),
            diagnostics: vec![cadder_protocol::RuntimeDiagnostic {
              code: "runtime-inspect-failed".to_string(),
              message: format!("could not inspect real Caddy runtime: {error}"),
              operation: Some("inspect".to_string()),
            }],
          };
        }
      }
    }

    RuntimeState::idle()
  }

  pub async fn apply_config(&self, rendered: &[u8], logs: &CaddyLogStore) -> Result<()> {
    let config_path = self.paths.effective_config_path();
    fs::write(&config_path, rendered)
      .await
      .with_context(|| format!("write effective config {}", config_path.display()))?;

    if !self.runtime_is_running(logs).await {
      self.start(&config_path, logs).await?;
    } else {
      self.reload(&config_path, logs).await?;
    }
    Ok(())
  }

  async fn start(&self, config_path: &PathBuf, logs: &CaddyLogStore) -> Result<()> {
    let binary = self.resolver.resolve()?;
    let mut command = Command::new(binary);
    command
      .arg("run")
      .arg("--config")
      .arg(config_path)
      .stdout(Stdio::piped())
      .stderr(Stdio::piped());
    let mut child = ProcessTreeChild::spawn(command).context("start real Caddy runtime")?;

    if let Some(stdout) = child.take_stdout() {
      spawn_log_reader(stdout, logs.clone(), "stdout");
    }
    if let Some(stderr) = child.take_stderr() {
      spawn_log_reader(stderr, logs.clone(), "stderr");
    }

    yield_now().await;
    match timeout(self.timeouts.start_check, child.wait()).await {
      Ok(Ok(status)) => {
        anyhow::bail!("real Caddy runtime exited immediately with status {status}");
      }
      Ok(Err(error)) => return Err(error).context("inspect started real Caddy runtime"),
      Err(_) => {}
    }

    *self.child.lock().await = Some(child);
    logs.append(
      LogStreamIdentity::runtime_control(),
      LogSeverity::Info,
      "real Caddy runtime started",
      LogAttributionKind::RuntimeControl,
      Some("start".to_string()),
    );
    Ok(())
  }

  async fn reload(&self, config_path: &PathBuf, logs: &CaddyLogStore) -> Result<()> {
    let binary = self.resolver.resolve()?;
    let mut command = Command::new(binary);
    command
      .arg("reload")
      .arg("--config")
      .arg(config_path)
      .stdout(Stdio::piped())
      .stderr(Stdio::piped());
    let child = ProcessTreeChild::spawn(command).context("start real Caddy reload")?;
    let output = child
      .wait_for_output(self.timeouts.reload, "real Caddy reload")
      .await?;
    if output.status.success() {
      logs.append(
        LogStreamIdentity::runtime_control(),
        LogSeverity::Info,
        "real Caddy runtime reloaded",
        LogAttributionKind::RuntimeControl,
        Some("reload".to_string()),
      );
      Ok(())
    } else {
      let message = String::from_utf8_lossy(&output.stderr).to_string();
      logs.append(
        LogStreamIdentity::runtime_control(),
        LogSeverity::Error,
        &message,
        LogAttributionKind::RuntimeControl,
        Some("reload".to_string()),
      );
      anyhow::bail!("caddy reload failed: {message}");
    }
  }

  pub async fn stop(&self) -> Result<()> {
    let child = {
      let mut guard = self.child.lock().await;
      guard.take()
    };
    if let Some(mut child) = child {
      if child
        .try_wait()
        .context("inspect real Caddy runtime before stop")?
        .is_some()
      {
        return Ok(());
      }
      let mut stop_error = None;
      if let Ok(binary) = self.resolver.resolve() {
        stop_error = request_graceful_stop(binary, self.timeouts.graceful_stop)
          .await
          .err();
      }
      if timeout(self.timeouts.stop_wait, child.wait())
        .await
        .is_err()
      {
        let _ = child.kill().await;
        timeout(self.timeouts.kill_wait, child.wait())
          .await
          .context("wait for killed real Caddy runtime")?
          .context("wait for real Caddy runtime after kill")?;
        if stop_error.is_none() {
          stop_error = Some(anyhow::anyhow!(
            "real Caddy runtime did not stop within {} seconds and was killed",
            self.timeouts.stop_wait.as_secs()
          ));
        }
      }
      if let Some(error) = stop_error {
        return Err(error);
      }
    }
    Ok(())
  }

  async fn runtime_is_running(&self, logs: &CaddyLogStore) -> bool {
    let mut guard = self.child.lock().await;
    let Some(child) = guard.as_mut() else {
      return false;
    };
    let status = child.try_wait();
    match status {
      Ok(None) => true,
      Ok(Some(status)) => {
        logs.append(
          LogStreamIdentity::runtime_control(),
          LogSeverity::Warn,
          format!("real Caddy runtime exited with status {status}; restarting"),
          LogAttributionKind::RuntimeControl,
          Some("runtime-liveness".to_string()),
        );
        *guard = None;
        false
      }
      Err(error) => {
        logs.append(
          LogStreamIdentity::runtime_control(),
          LogSeverity::Error,
          format!("could not inspect real Caddy runtime: {error}; restarting"),
          LogAttributionKind::RuntimeControl,
          Some("runtime-liveness".to_string()),
        );
        *guard = None;
        false
      }
    }
  }
}

#[derive(Debug, Clone)]
pub struct MockCaddyRuntime {
  paths: RuntimePaths,
  state: Arc<Mutex<MockCaddyRuntimeState>>,
}

#[derive(Debug, Default)]
struct MockCaddyRuntimeState {
  running: bool,
  applied_config_bytes: usize,
}

impl MockCaddyRuntime {
  pub fn new(paths: RuntimePaths) -> Self {
    Self {
      paths,
      state: Arc::new(Mutex::new(MockCaddyRuntimeState::default())),
    }
  }

  pub async fn inspect(&self) -> RuntimeState {
    let state = self.state.lock().await;
    if state.running {
      RuntimeState {
        status: RuntimeStatus::Running,
        binary_path: Some("mock-caddy".to_string()),
        version: Some(format!("mock:{}", state.applied_config_bytes)),
        process_id: None,
        admin_endpoint: None,
        diagnostics: Vec::new(),
      }
    } else {
      RuntimeState::idle()
    }
  }

  pub async fn apply_config(&self, rendered: &[u8], logs: &CaddyLogStore) -> Result<()> {
    let config_path = self.paths.effective_config_path();
    if let Some(parent) = config_path.parent() {
      fs::create_dir_all(parent)
        .await
        .with_context(|| format!("create mock runtime directory {}", parent.display()))?;
    }
    fs::write(&config_path, rendered)
      .await
      .with_context(|| format!("write mock effective config {}", config_path.display()))?;

    {
      let mut state = self.state.lock().await;
      state.running = true;
      state.applied_config_bytes = rendered.len();
    }

    logs.append(
      LogStreamIdentity::runtime_control(),
      LogSeverity::Info,
      "mock Caddy runtime accepted effective config without starting a process",
      LogAttributionKind::RuntimeControl,
      Some("mock-apply".to_string()),
    );
    Ok(())
  }

  pub async fn stop(&self) -> Result<()> {
    self.state.lock().await.running = false;
    Ok(())
  }
}

async fn request_graceful_stop(binary: PathBuf, graceful_stop: Duration) -> Result<()> {
  let mut command = Command::new(binary);
  command
    .arg("stop")
    .arg("--address")
    .arg("localhost:2019")
    .stdout(Stdio::null())
    .stderr(Stdio::null());
  let mut child = ProcessTreeChild::spawn(command).context("start caddy stop")?;
  let status = match timeout(graceful_stop, child.wait()).await {
    Ok(result) => result.context("wait for caddy stop")?,
    Err(_) => {
      let _ = child.kill().await;
      anyhow::bail!(
        "caddy stop timed out after {} seconds",
        graceful_stop.as_secs()
      );
    }
  };
  if !status.success() {
    anyhow::bail!("caddy stop failed with status {status}");
  }
  Ok(())
}

fn spawn_log_reader<R>(reader: R, logs: CaddyLogStore, channel: &'static str)
where
  R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
  tokio::spawn(async move {
    let mut reader = BufReader::new(reader).lines();
    while let Ok(Some(line)) = reader.next_line().await {
      let severity = if channel == "stderr" {
        LogSeverity::Error
      } else {
        LogSeverity::Info
      };
      logs.append(
        LogStreamIdentity {
          stream_id: "runtime".to_string(),
          domain_key: None,
          channel: channel.to_string(),
        },
        severity,
        line,
        LogAttributionKind::Runtime,
        None,
      );
    }
  });
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::logs::LogQuery;
  use std::{fs as std_fs, path::Path};
  use tokio::time::sleep;

  #[derive(Debug, Clone, Copy)]
  enum FakeRuntimeMode {
    LongRunning,
    ShortRun,
    FailReload,
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
      reload: Duration::from_millis(500),
      graceful_stop: Duration::from_millis(500),
      stop_wait: Duration::from_secs(2),
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
    let resolver = RealCaddyResolver::new(Some(fake_caddy.display().to_string()));
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
    assert_eq!(timeouts.graceful_stop, Duration::from_secs(5));
    assert_eq!(timeouts.stop_wait, Duration::from_secs(5));
    assert_eq!(timeouts.kill_wait, Duration::from_secs(5));
    assert!(format!("{:?}", timeouts).contains("RuntimeTimeouts"));
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
    let logs = fixture.logs.query(
      LogQuery {
        stream: LogStreamIdentity::runtime_control(),
        limit: 20,
        after_sequence: None,
        minimum_severity: None,
      },
      true,
    );

    assert!(logs.entries.iter().any(|entry| {
      entry.operation.as_deref() == Some("reload")
        && entry.raw_message.contains("real Caddy runtime reloaded")
    }));
    fixture.runtime.stop().await.unwrap();
  }

  #[tokio::test]
  async fn apply_config_reports_reload_failure_for_running_runtime() {
    let fixture = runtime_fixture(FakeRuntimeMode::FailReload);
    fixture
      .runtime
      .apply_config(br#"{"apps":{}}"#, &fixture.logs)
      .await
      .unwrap();
    wait_for_command_log(&fixture.command_log, "run").await;

    let error = fixture
      .runtime
      .apply_config(br#"{"apps":{"http":{}}}"#, &fixture.logs)
      .await
      .unwrap_err();

    assert!(error.to_string().contains("caddy reload failed"));
    wait_for_command_log(&fixture.command_log, "reload").await;
    fixture.runtime.stop().await.unwrap();
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
  async fn request_graceful_stop_times_out_the_stop_command() {
    let temp = tempfile::tempdir().unwrap();
    let command_log = temp.path().join("fake-caddy.log");
    let fake_caddy = write_fake_caddy(temp.path(), &command_log, FakeRuntimeMode::SlowStop);

    let error = request_graceful_stop(fake_caddy, Duration::from_millis(500))
      .await
      .unwrap_err();

    assert!(error.to_string().contains("caddy stop timed out"));
    wait_for_command_log(&command_log, "stop").await;
  }

  #[tokio::test]
  async fn request_graceful_stop_reports_failed_stop_status() {
    let temp = tempfile::tempdir().unwrap();
    let command_log = temp.path().join("fake-caddy.log");
    let fake_caddy = write_fake_caddy(temp.path(), &command_log, FakeRuntimeMode::FailStop);

    let error = request_graceful_stop(fake_caddy, Duration::from_secs(1))
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
          .is_none_or(|child| child.try_wait().unwrap().is_some())
      };
      if has_exited {
        return;
      }
      sleep(Duration::from_millis(20)).await;
    }
    panic!("expected fake Caddy runtime child to exit");
  }

  fn write_fake_caddy(dir: &Path, command_log: &Path, mode: FakeRuntimeMode) -> PathBuf {
    let stop_file = dir.join("fake-caddy.stop");
    let run_exit_file = dir.join("fake-caddy.exit");

    #[cfg(windows)]
    {
      let path = dir.join("fake-caddy.cmd");
      let reload_behavior = if matches!(mode, FakeRuntimeMode::FailReload) {
        "echo reload failed 1>&2\r\n  exit /b 7"
      } else {
        "exit /b 0"
      };
      let stop_behavior = if matches!(mode, FakeRuntimeMode::SlowStop) {
        "\"%SystemRoot%\\System32\\ping.exe\" -n 3 127.0.0.1 >nul\r\n  echo stop> \"{stop_file}\"\r\n  exit /b 0"
      } else if matches!(mode, FakeRuntimeMode::FailStop) {
        "echo stop> \"{stop_file}\"\r\n  exit /b 7"
      } else {
        "echo stop> \"{stop_file}\"\r\n  exit /b 0"
      };
      let run_behavior = if matches!(mode, FakeRuntimeMode::ShortRun) {
        ":short_run_loop\r\n  if exist \"{stop_file}\" exit /b 0\r\n  if exist \"{run_exit_file}\" goto short_run_exit\r\n  \"%SystemRoot%\\System32\\ping.exe\" -n 2 127.0.0.1 >nul\r\n  goto short_run_loop\r\n  :short_run_exit\r\n  del /q \"{run_exit_file}\"\r\n  echo run-exited>> \"{command_log}\"\r\n  exit /b 0"
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
          reload_behavior =
            reload_behavior.replace("{stop_file}", &stop_file.display().to_string()),
          stop_behavior = stop_behavior.replace("{stop_file}", &stop_file.display().to_string()),
          run_behavior = run_behavior
            .replace("{stop_file}", &stop_file.display().to_string())
            .replace("{run_exit_file}", &run_exit_file.display().to_string())
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
        "printf '%s\n' 'reload failed' >&2\n  exit 7"
      } else {
        "exit 0"
      };
      let stop_behavior = if matches!(mode, FakeRuntimeMode::SlowStop) {
        "sleep 1\n  : > '{stop_file}'\n  exit 0"
      } else if matches!(mode, FakeRuntimeMode::FailStop) {
        ": > '{stop_file}'\n  exit 7"
      } else {
        ": > '{stop_file}'\n  exit 0"
      };
      let run_behavior = if matches!(mode, FakeRuntimeMode::ShortRun) {
        "while [ ! -f '{run_exit_file}' ] && [ ! -f '{stop_file}' ]; do /bin/sleep 0.02; done\n  if [ -f '{run_exit_file}' ]; then /bin/rm -f '{run_exit_file}'; printf '%s\n' 'run-exited' >> '{command_log}'; fi\n  exit 0"
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
          reload_behavior = reload_behavior,
          stop_behavior = stop_behavior.replace("{stop_file}", &stop_file.display().to_string()),
          run_behavior = run_behavior
            .replace("{stop_file}", &stop_file.display().to_string())
            .replace("{run_exit_file}", &run_exit_file.display().to_string())
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
}
