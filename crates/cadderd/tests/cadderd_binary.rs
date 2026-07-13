use cadder_daemon::{CadderClient, RuntimePaths};
#[cfg(windows)]
use cadder_daemon::{
  RUNTIME_GUARD_PROTOCOL_REVISION, RuntimeGuardBootstrapRequest, RuntimeGuardGenerationContext,
  RuntimeGuardRequest,
};
use cadder_protocol::{BasicResponse, QueryStateRequest, QueryStateResponse, message_types};
#[cfg(windows)]
use serde_json::Value;
#[cfg(windows)]
use std::{io::Write, process::Output};
use std::{
  path::{Path, PathBuf},
  process::{Child, Command, Stdio},
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

#[cfg(windows)]
fn unique_trusted_runtime_dir(name: &str) -> PathBuf {
  let user_profile = std::env::var_os("USERPROFILE").expect("Windows exposes USERPROFILE");
  let unique = unique_runtime_dir(name)
    .file_name()
    .expect("unique runtime has a final component")
    .to_owned();
  PathBuf::from(user_profile).join(format!(".cadder-guard-test-{}", unique.to_string_lossy()))
}

fn spawn_cadderd(runtime_dir: &PathBuf) -> Child {
  Command::new(test_cadderd_executable(runtime_dir))
    .arg("--runtime-dir")
    .arg(runtime_dir)
    .arg("--caddy-backend")
    .arg("mock")
    .env("CADDER_TEST_ALLOW_UNTRUSTED_RUNTIME_GUARD", "1")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .unwrap()
}

fn test_cadderd_executable(runtime_dir: &Path) -> PathBuf {
  #[cfg(windows)]
  {
    let paths = RuntimePaths::resolve(Some(runtime_dir.to_path_buf())).unwrap();
    paths.ensure_dirs().unwrap();
    let executable = runtime_dir.join("cadderd-test.exe");
    if !executable.exists() {
      std::fs::copy(env!("CARGO_BIN_EXE_cadderd"), &executable).unwrap();
    }
    executable
  }

  #[cfg(not(windows))]
  {
    PathBuf::from(env!("CARGO_BIN_EXE_cadderd"))
  }
}

#[cfg(windows)]
struct RetainedChild(Option<Child>);

#[cfg(windows)]
impl RetainedChild {
  fn spawn(command: &mut Command) -> Self {
    Self(Some(command.spawn().expect("spawn retained test process")))
  }

  fn stdin(&mut self) -> std::process::ChildStdin {
    self
      .0
      .as_mut()
      .and_then(|child| child.stdin.take())
      .expect("retained test process should expose stdin")
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
    let mut child = self
      .0
      .take()
      .expect("retained test process should still be owned");
    child.wait().expect("wait for retained test process")
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

#[cfg(windows)]
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

#[cfg(windows)]
fn retained_cadderd(runtime_dir: &PathBuf) -> RetainedChild {
  let mut command = Command::new(test_cadderd_executable(runtime_dir));
  command
    .arg("--runtime-dir")
    .arg(runtime_dir)
    .arg("--caddy-backend")
    .arg("mock")
    .env("CADDER_TEST_ALLOW_UNTRUSTED_RUNTIME_GUARD", "1")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
  RetainedChild::spawn(&mut command)
}

#[cfg(windows)]
async fn wait_for_state_or_exit(
  client: &CadderClient,
  child: &mut RetainedChild,
) -> QueryStateResponse {
  for _ in 0..1_500 {
    if let Ok(response) = client
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: cadder_protocol::new_request_id("runtime-guard-query"),
        },
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
    "cadderd did not become ready with its runtime guard: {}",
    String::from_utf8_lossy(&output.stderr)
  );
}

#[cfg(windows)]
async fn wait_for_containment_state(paths: &RuntimePaths, expected: &str) -> Value {
  for _ in 0..200 {
    if let Ok(bytes) = std::fs::read(paths.containment_record_path())
      && let Ok(record) = serde_json::from_slice::<Value>(&bytes)
      && record.get("state").and_then(Value::as_str) == Some(expected)
    {
      return record;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
  panic!("runtime containment record did not reach state `{expected}`");
}

#[cfg(windows)]
fn read_json(path: PathBuf) -> Value {
  serde_json::from_slice(&std::fs::read(&path).expect("read JSON artifact"))
    .expect("decode JSON artifact")
}

#[cfg(windows)]
async fn shutdown_daemon(client: &CadderClient, request_prefix: &str) -> BasicResponse {
  client
    .request(
      message_types::SHUTDOWN_DAEMON_REQUEST,
      message_types::SHUTDOWN_DAEMON_RESPONSE,
      &cadder_protocol::ShutdownDaemonRequest {
        request_id: cadder_protocol::new_request_id(request_prefix),
      },
    )
    .await
    .expect("request guarded daemon shutdown")
}

async fn wait_for_state(client: &CadderClient) -> QueryStateResponse {
  for _ in 0..1_500 {
    let response = client
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: cadder_protocol::new_request_id("test-query"),
        },
      )
      .await;
    if let Ok(response) = response {
      return response;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
  panic!("cadderd did not become ready");
}

#[tokio::test]
async fn cadderd_binary_serves_ipc_and_shuts_down_cleanly() {
  let _test_lock = BINARY_PROCESS_TEST_LOCK.lock().await;
  let runtime_dir = unique_runtime_dir("ipc");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let client = CadderClient::new(paths);
  let mut child = spawn_cadderd(&runtime_dir);

  let state = wait_for_state(&client).await;
  let shutdown: BasicResponse = client
    .request(
      message_types::SHUTDOWN_DAEMON_REQUEST,
      message_types::SHUTDOWN_DAEMON_RESPONSE,
      &cadder_protocol::ShutdownDaemonRequest {
        request_id: cadder_protocol::new_request_id("test-shutdown"),
      },
    )
    .await
    .unwrap();
  let status = child.wait().unwrap();

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
  let mut child = spawn_cadderd(&runtime_dir);

  let state = wait_for_state(&client).await;
  let repeated = Command::new(env!("CARGO_BIN_EXE_cadderd"))
    .arg("--runtime-dir")
    .arg(&runtime_dir)
    .arg("--caddy-backend")
    .arg("mock")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .unwrap();
  let shutdown: BasicResponse = client
    .request(
      message_types::SHUTDOWN_DAEMON_REQUEST,
      message_types::SHUTDOWN_DAEMON_RESPONSE,
      &cadder_protocol::ShutdownDaemonRequest {
        request_id: cadder_protocol::new_request_id("test-shutdown"),
      },
    )
    .await
    .unwrap();
  let status = child.wait().unwrap();

  assert!(state.accepted);
  assert!(repeated.success(), "{repeated}");
  assert!(shutdown.accepted, "{}", shutdown.message);
  assert!(status.success(), "{status}");
  let _ = std::fs::remove_dir_all(runtime_dir);
}

#[cfg(windows)]
#[tokio::test]
async fn runtime_guard_rejects_wrong_bootstrap_secret_before_record_or_lock() {
  let _test_lock = BINARY_PROCESS_TEST_LOCK.lock().await;
  let runtime_dir = unique_trusted_runtime_dir("runtime-guard-wrong-bootstrap");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let context = RuntimeGuardGenerationContext {
    profile: "default".to_string(),
    runtime_id: paths.instance_key().to_string(),
    daemon_instance_id: "00112233445566778899aabbccddeeff".to_string(),
    owner_generation: "ffeeddccbbaa99887766554433221100".to_string(),
    nonce_commitment: "11".repeat(32),
  };
  let wrong_nonce = "00".repeat(32);
  let request = RuntimeGuardRequest::Bootstrap(RuntimeGuardBootstrapRequest {
    protocol_revision: RUNTIME_GUARD_PROTOCOL_REVISION,
    context: context.clone(),
    request_id: 1,
    nonce: wrong_nonce.clone(),
  });
  let mut command = Command::new(test_cadderd_executable(&runtime_dir));
  command
    .arg("--runtime-guard")
    .arg("--runtime-dir")
    .arg(&runtime_dir)
    .arg("--runtime-guard-instance")
    .arg(&context.daemon_instance_id)
    .arg("--runtime-guard-owner-generation")
    .arg(&context.owner_generation)
    .arg("--runtime-guard-commitment")
    .arg(&context.nonce_commitment)
    .env("CADDER_TEST_ALLOW_UNTRUSTED_RUNTIME_GUARD", "1")
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
  let mut guard = RetainedChild::spawn(&mut command);
  let mut stdin = guard.stdin();
  serde_json::to_writer(&mut stdin, &request).unwrap();
  stdin.write_all(b"\n").unwrap();
  drop(stdin);

  let output = guard.wait_with_output();
  let stderr = String::from_utf8_lossy(&output.stderr);

  assert!(!output.status.success(), "{output:?}");
  assert!(
    stderr.contains("runtime guard bootstrap authentication failed"),
    "{stderr}"
  );
  assert!(!stderr.contains(&wrong_nonce), "{stderr}");
  assert!(!paths.containment_record_path().exists());
  assert!(!paths.containment_lock_path().exists());
  let _ = std::fs::remove_dir_all(runtime_dir);
}

#[cfg(windows)]
#[tokio::test]
async fn runtime_guard_daemon_cycle_publishes_bound_ready_then_terminal_record_and_allows_verified_replacement()
 {
  let _test_lock = BINARY_PROCESS_TEST_LOCK.lock().await;
  let runtime_dir = unique_trusted_runtime_dir("runtime-guard-daemon-cycle");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let client = CadderClient::new(paths.clone());
  let mut first = retained_cadderd(&runtime_dir);

  let first_state = wait_for_state_or_exit(&client, &mut first).await;
  let first_ready = wait_for_containment_state(&paths, "ready").await;
  let first_lock = read_json(paths.lock_metadata_path());
  let first_binding = first_lock
    .get("containment")
    .expect("daemon metadata should retain its ready containment binding")
    .clone();

  assert!(first_state.accepted, "{first_state:?}");
  assert_eq!(
    first_ready.get("profile").and_then(Value::as_str),
    Some("default")
  );
  assert_eq!(
    first_ready.get("runtimeId").and_then(Value::as_str),
    Some(paths.instance_key())
  );
  assert_eq!(
    first_ready.get("ownerGeneration"),
    first_lock.get("ownerGeneration")
  );
  assert_eq!(
    first_binding.pointer("/context/daemonInstanceId"),
    first_ready.get("daemonInstanceId")
  );
  assert_eq!(first_binding.get("guard"), first_ready.get("guard"));
  assert!(first_ready.get("treeEmpty").is_none());
  assert!(paths.containment_lock_path().exists());

  let first_shutdown = shutdown_daemon(&client, "runtime-guard-first-shutdown").await;
  let first_status = first.wait();
  let first_terminal = wait_for_containment_state(&paths, "terminal").await;
  let preserved_first_lock = read_json(paths.lock_metadata_path());

  assert!(first_shutdown.accepted, "{}", first_shutdown.message);
  assert!(first_status.success(), "{first_status}");
  assert_eq!(first_terminal.get("treeEmpty"), Some(&Value::Bool(true)));
  assert_eq!(
    first_terminal
      .pointer("/terminal/reason")
      .and_then(Value::as_str),
    Some("cleanFinalize")
  );
  assert_eq!(
    preserved_first_lock.get("containment"),
    Some(&first_binding)
  );

  let mut replacement = retained_cadderd(&runtime_dir);
  let replacement_state = wait_for_state_or_exit(&client, &mut replacement).await;
  let replacement_ready = wait_for_containment_state(&paths, "ready").await;
  let replacement_lock = read_json(paths.lock_metadata_path());

  assert!(replacement_state.accepted, "{replacement_state:?}");
  assert!(replacement_lock.get("predecessorContainment").is_none());
  assert_ne!(replacement_lock.get("containment"), Some(&first_binding));
  assert_ne!(
    replacement_ready.get("daemonInstanceId"),
    first_terminal.get("daemonInstanceId")
  );
  assert_ne!(
    replacement_ready.get("ownerGeneration"),
    first_terminal.get("ownerGeneration")
  );
  assert_eq!(
    replacement_lock.pointer("/containment/context/daemonInstanceId"),
    replacement_ready.get("daemonInstanceId")
  );

  let replacement_shutdown = shutdown_daemon(&client, "runtime-guard-replacement-shutdown").await;
  let replacement_status = replacement.wait();
  let replacement_terminal = wait_for_containment_state(&paths, "terminal").await;

  assert!(
    replacement_shutdown.accepted,
    "{}",
    replacement_shutdown.message
  );
  assert!(replacement_status.success(), "{replacement_status}");
  assert_eq!(
    replacement_terminal.get("treeEmpty"),
    Some(&Value::Bool(true))
  );
  let _ = std::fs::remove_dir_all(runtime_dir);
}
