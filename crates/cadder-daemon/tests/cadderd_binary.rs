use cadder_daemon::{CadderClient, RuntimePaths};
use cadder_ipc::{BasicResponse, QueryStatePayload, QueryStateResponse, ShutdownDaemonPayload};
use std::{
  path::{Path, PathBuf},
  process::{Child, Command, Output, Stdio},
  time::{Duration, SystemTime, UNIX_EPOCH},
};

static BINARY_PROCESS_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn unique_runtime_dir(name: &str) -> PathBuf {
  std::env::temp_dir().join(format!(
    "cadderd-{name}-{}-{}",
    std::process::id(),
    SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_nanos()
  ))
}

fn test_cadderd_executable(runtime_dir: &Path) -> PathBuf {
  std::fs::create_dir_all(runtime_dir).unwrap();
  let executable = runtime_dir.join(if cfg!(windows) {
    "cadderd.exe"
  } else {
    "cadderd"
  });
  if !executable.exists() {
    std::fs::copy(env!("CARGO_BIN_EXE_cadder-daemon"), &executable).unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
      permissions.set_mode(0o755);
      std::fs::set_permissions(&executable, permissions).unwrap();
    }
  }
  executable
}

struct RetainedChild(Option<Child>);

impl RetainedChild {
  fn spawn(command: &mut Command) -> Self {
    Self(Some(command.spawn().expect("spawn retained test process")))
  }

  fn try_wait(&mut self) -> Option<std::process::ExitStatus> {
    self
      .0
      .as_mut()
      .expect("retained test process should still be owned")
      .try_wait()
      .expect("inspect retained test process")
  }

  fn wait(&mut self) -> std::process::ExitStatus {
    self
      .0
      .take()
      .expect("retained test process should still be owned")
      .wait()
      .expect("wait for retained test process")
  }

  fn wait_with_output(&mut self) -> Output {
    self
      .0
      .take()
      .expect("retained test process should still be owned")
      .wait_with_output()
      .expect("wait for retained test process output")
  }

  fn terminate_with_output(&mut self) -> Output {
    let child = self
      .0
      .as_mut()
      .expect("retained test process should still be owned");
    let _ = child.kill();
    self.wait_with_output()
  }
}

impl Drop for RetainedChild {
  fn drop(&mut self) {
    let Some(mut child) = self.0.take() else {
      return;
    };
    if matches!(child.try_wait(), Ok(None)) {
      let _ = child.kill();
    }
    let _ = child.wait();
  }
}

fn retained_cadderd(runtime_dir: &Path) -> RetainedChild {
  let mut command = Command::new(test_cadderd_executable(runtime_dir));
  command
    .arg("--caddy-backend")
    .arg("mock")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
  RetainedChild::spawn(&mut command)
}

async fn wait_for_state_or_exit(
  client: &CadderClient,
  child: &mut RetainedChild,
) -> QueryStateResponse {
  for _ in 0..1_500 {
    if let Ok(response) = client
      .request::<_>(
        cadder_ipc::new_request_id("binary-query"),
        &QueryStatePayload::default(),
      )
      .await
    {
      return response;
    }
    if let Some(status) = child.try_wait() {
      let output = child.wait_with_output();
      panic!(
        "cadderd exited before readiness with {status}: {}",
        String::from_utf8_lossy(&output.stderr)
      );
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
  let output = child.terminate_with_output();
  panic!(
    "cadderd did not become ready: {}",
    String::from_utf8_lossy(&output.stderr)
  );
}

#[tokio::test]
async fn cadderd_binary_serves_ipc_and_shuts_down_cleanly() {
  let _test_lock = BINARY_PROCESS_TEST_LOCK.lock().await;
  let runtime_dir = unique_runtime_dir("ipc");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let client = CadderClient::new(paths);
  let mut child = retained_cadderd(&runtime_dir);

  let state = wait_for_state_or_exit(&client, &mut child).await;
  let shutdown: BasicResponse = client
    .request(
      cadder_ipc::new_request_id("test-shutdown"),
      &ShutdownDaemonPayload::default(),
    )
    .await
    .unwrap();
  let status = child.wait();

  assert!(state.accepted);
  assert!(shutdown.accepted, "{}", shutdown.message);
  assert!(status.success(), "{status}");
  let _ = std::fs::remove_dir_all(runtime_dir);
}

#[tokio::test]
async fn cadderd_binary_repeated_start_succeeds_when_runtime_is_already_running() {
  let _test_lock = BINARY_PROCESS_TEST_LOCK.lock().await;
  let runtime_dir = unique_runtime_dir("repeated-start");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let client = CadderClient::new(paths);
  let mut child = retained_cadderd(&runtime_dir);

  let state = wait_for_state_or_exit(&client, &mut child).await;
  let repeated = Command::new(test_cadderd_executable(&runtime_dir))
    .arg("--caddy-backend")
    .arg("mock")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .unwrap();
  let shutdown: BasicResponse = client
    .request(
      cadder_ipc::new_request_id("test-shutdown"),
      &ShutdownDaemonPayload::default(),
    )
    .await
    .unwrap();
  let status = child.wait();

  assert!(state.accepted);
  assert!(repeated.success(), "{repeated}");
  assert!(shutdown.accepted, "{}", shutdown.message);
  assert!(status.success(), "{status}");
  let _ = std::fs::remove_dir_all(runtime_dir);
}
