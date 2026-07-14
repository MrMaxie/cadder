use cadder_daemon::{
  CadderClient, CadderSession, RUNTIME_GUARD_PROTOCOL_REVISION, RuntimeGuardBootstrapRequest,
  RuntimeGuardGenerationContext, RuntimeGuardRequest, RuntimePaths,
};
use cadder_ipc::{
  ActivationState, BasicResponse, EntrypointInstanceIdentity, EntrypointRegistration,
  LogStreamIdentity, OwnerProcessIdentity, QueryStateRequest, QueryStateResponse,
  RegisterEntrypointRequest, RegisterEntrypointResponse, RegisteredDomain, ShimRunMetadata,
  SourcePath, message_types,
};
use chrono::Utc;
use serde_json::Value;
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

#[cfg(not(windows))]
fn unique_trusted_runtime_dir(name: &str) -> PathBuf {
  unique_runtime_dir(name)
}

fn test_cadderd_executable(runtime_dir: &Path) -> PathBuf {
  std::fs::create_dir_all(runtime_dir).unwrap();
  let executable = runtime_dir.join(if cfg!(windows) {
    "cadderd.exe"
  } else {
    "cadderd"
  });
  if !executable.exists() {
    std::fs::copy(env!("CARGO_BIN_EXE_cadderd"), &executable).unwrap();
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

fn retained_real_cadderd(
  runtime_dir: &Path,
  fake_caddy: &Path,
  command_log: &Path,
) -> RetainedChild {
  let mut command = Command::new(test_cadderd_executable(runtime_dir));
  command
    .arg("--caddy-backend")
    .arg("real")
    .arg("--real-caddy")
    .arg(fake_caddy)
    .env("CADDER_TEST_ALLOW_UNTRUSTED_CADDY", "1")
    .env("CADDER_TEST_CADDY_COMMAND_LOG", command_log)
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
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: cadder_ipc::new_request_id("runtime-guard-query"),
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

async fn wait_for_containment_child(paths: &RuntimePaths) -> Value {
  for _ in 0..500 {
    if let Ok(bytes) = std::fs::read(paths.containment_record_path())
      && let Ok(record) = serde_json::from_slice::<Value>(&bytes)
      && record.get("state").and_then(Value::as_str) == Some("ready")
      && record.get("child").is_some()
      && record.get("treeEmpty") == Some(&Value::Bool(false))
    {
      return record;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
  panic!("runtime containment record did not publish an active child");
}

fn read_json(path: PathBuf) -> Value {
  serde_json::from_slice(&std::fs::read(&path).expect("read JSON artifact"))
    .expect("decode JSON artifact")
}

async fn shutdown_daemon(client: &CadderClient, request_prefix: &str) -> BasicResponse {
  client
    .request(
      message_types::SHUTDOWN_DAEMON_REQUEST,
      message_types::SHUTDOWN_DAEMON_RESPONSE,
      &cadder_ipc::ShutdownDaemonRequest {
        request_id: cadder_ipc::new_request_id(request_prefix),
      },
    )
    .await
    .expect("request guarded daemon shutdown")
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
      message_types::SHUTDOWN_DAEMON_REQUEST,
      message_types::SHUTDOWN_DAEMON_RESPONSE,
      &cadder_ipc::ShutdownDaemonRequest {
        request_id: cadder_ipc::new_request_id("test-shutdown"),
      },
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
      message_types::SHUTDOWN_DAEMON_REQUEST,
      message_types::SHUTDOWN_DAEMON_RESPONSE,
      &cadder_ipc::ShutdownDaemonRequest {
        request_id: cadder_ipc::new_request_id("test-shutdown"),
      },
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
    pinned_caddy: None,
  });
  let mut command = Command::new(test_cadderd_executable(&runtime_dir));
  command
    .arg("--runtime-guard")
    .arg("--runtime-guard-instance")
    .arg(&context.daemon_instance_id)
    .arg("--runtime-guard-owner-generation")
    .arg(&context.owner_generation)
    .arg("--runtime-guard-commitment")
    .arg(&context.nonce_commitment)
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
    first_binding.pointer("/generation/context/daemonInstanceId"),
    first_ready.get("daemonInstanceId")
  );
  assert!(first_ready.get("guard").is_some());
  assert_eq!(
    first_binding.pointer("/generation/guard"),
    first_ready.get("guard")
  );
  assert_eq!(first_binding.get("lastChild"), Some(&Value::Null));
  assert_eq!(
    first_binding.pointer("/generation/context/nonceCommitment"),
    first_ready.get("nonceCommitment")
  );
  assert_eq!(first_ready.get("treeEmpty"), Some(&Value::Bool(true)));
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
    replacement_lock.pointer("/containment/generation/context/daemonInstanceId"),
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

#[tokio::test]
async fn runtime_guard_containment_owner_loss_terminates_the_guarded_caddy_tree_before_replacement()
{
  let _test_lock = BINARY_PROCESS_TEST_LOCK.lock().await;
  let runtime_dir = unique_trusted_runtime_dir("runtime-guard-owner-loss");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  paths.ensure_dirs().unwrap();
  let command_log = runtime_dir.join("fake-caddy-commands.log");
  let fake_caddy = write_guarded_fake_caddy(&runtime_dir);
  let config_path = runtime_dir.join("Caddyfile");
  std::fs::write(&config_path, ":8080 { respond \"ok\" }\n").unwrap();
  let client = CadderClient::new(paths.clone());
  let mut daemon = retained_real_cadderd(&runtime_dir, &fake_caddy, &command_log);

  let state = wait_for_state_or_exit(&client, &mut daemon).await;
  assert!(state.accepted, "{state:?}");
  let mut session = CadderSession::connect(&paths).await.unwrap();
  let registration: RegisterEntrypointResponse = session
    .request(
      message_types::REGISTER_ENTRYPOINT_REQUEST,
      message_types::REGISTER_ENTRYPOINT_RESPONSE,
      &RegisterEntrypointRequest {
        request_id: cadder_ipc::new_request_id("guarded-register"),
        registration: guarded_registration(&config_path),
      },
    )
    .await
    .unwrap();
  assert!(registration.accepted, "{}", registration.message);
  drop(session);
  let active = wait_for_containment_child(&paths).await;
  let active_child = active.get("child").unwrap().clone();
  let active_lock = read_json(paths.lock_metadata_path());
  assert_eq!(
    active_lock.pointer("/containment/lastChild"),
    Some(&active_child)
  );

  let output = daemon.terminate_with_output();
  assert!(!output.status.success(), "{output:?}");
  let terminal = wait_for_containment_state(&paths, "terminal").await;

  assert_eq!(terminal.get("treeEmpty"), Some(&Value::Bool(true)));
  assert_eq!(terminal.get("child"), Some(&active_child));
  assert_eq!(
    terminal.pointer("/terminal/reason").and_then(Value::as_str),
    Some("ownerChannelClosed")
  );

  let mut replacement = retained_cadderd(&runtime_dir);
  let replacement_state = wait_for_state_or_exit(&client, &mut replacement).await;
  assert!(replacement_state.accepted, "{replacement_state:?}");
  let shutdown = shutdown_daemon(&client, "runtime-guard-owner-loss-replacement").await;
  assert!(shutdown.accepted, "{}", shutdown.message);
  assert!(replacement.wait().success());
  let _ = std::fs::remove_dir_all(runtime_dir);
}

#[tokio::test]
async fn runtime_guard_exit_forces_the_ready_daemon_to_shut_down() {
  let _test_lock = BINARY_PROCESS_TEST_LOCK.lock().await;
  let runtime_dir = unique_trusted_runtime_dir("runtime-guard-supervision");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let client = CadderClient::new(paths.clone());
  let mut daemon = retained_cadderd(&runtime_dir);

  let state = wait_for_state_or_exit(&client, &mut daemon).await;
  assert!(state.accepted, "{state:?}");
  let ready = wait_for_containment_state(&paths, "ready").await;
  let guard_process_id = ready
    .pointer("/guard/process/processId")
    .and_then(Value::as_u64)
    .and_then(|value| u32::try_from(value).ok())
    .expect("ready containment record exposes the guard process ID");

  force_kill_process(guard_process_id);
  let status = tokio::time::timeout(Duration::from_secs(10), async { daemon.wait() })
    .await
    .expect("daemon stayed ready after its runtime guard exited");

  assert!(!status.success(), "{status}");
  assert!(
    client
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: cadder_ipc::new_request_id("guard-exit-query"),
        },
      )
      .await
      .is_err(),
    "daemon still accepted requests after losing its runtime guard"
  );
  let _ = std::fs::remove_dir_all(runtime_dir);
}

fn force_kill_process(process_id: u32) {
  #[cfg(windows)]
  let status = Command::new("taskkill")
    .args(["/PID", &process_id.to_string(), "/F"])
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .unwrap();
  #[cfg(unix)]
  let status = Command::new("kill")
    .args(["-KILL", &process_id.to_string()])
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .unwrap();
  assert!(status.success(), "could not kill process {process_id}");
}

fn guarded_registration(config_path: &Path) -> EntrypointRegistration {
  let now = Utc::now();
  let nonce = "guarded-session";
  let identity = EntrypointInstanceIdentity {
    instance_id: "guarded-entrypoint".to_string(),
    started_at_utc: now,
    shim_session_nonce: nonce.to_string(),
  };
  EntrypointRegistration {
    registration_id: "guarded-entrypoint".to_string(),
    entrypoint_instance: identity,
    source_working_directory: SourcePath::new(".", None),
    source_config_path: SourcePath::new(
      config_path.display().to_string(),
      Some(config_path.display().to_string()),
    ),
    registered_domains: vec![RegisteredDomain::active("guarded.localhost")],
    activation_state: ActivationState::Active,
    owner_process: OwnerProcessIdentity {
      process_id: std::process::id(),
      process_start_time_utc: now,
      shim_session_nonce: nonce.to_string(),
      executable_path: None,
    },
    log_stream: LogStreamIdentity::entrypoint("guarded-entrypoint"),
    shim_run: Some(ShimRunMetadata {
      adapter: Some("caddyfile".to_string()),
      raw_arguments: vec!["run".to_string()],
      command_line: "run".to_string(),
    }),
    created_at_utc: now,
    last_heartbeat_utc: now,
  }
}

fn write_guarded_fake_caddy(dir: &Path) -> PathBuf {
  let modules = r#"[{"module_name":"http"},{"module_name":"http.encoders.gzip"},{"module_name":"http.encoders.zstd"},{"module_name":"http.handlers.encode"},{"module_name":"http.handlers.file_server"},{"module_name":"http.handlers.headers"},{"module_name":"http.handlers.reverse_proxy"},{"module_name":"http.handlers.rewrite"},{"module_name":"http.handlers.static_response"},{"module_name":"http.handlers.subroute"},{"module_name":"http.matchers.header"},{"module_name":"http.matchers.host"},{"module_name":"http.matchers.method"},{"module_name":"http.matchers.path"},{"module_name":"http.matchers.query"},{"module_name":"http.reverse_proxy.transport.http"},{"module_name":"pki"},{"module_name":"tls"},{"module_name":"tls.issuance.internal"}]"#;
  let source = r##"
use std::{env, fs::OpenOptions, io::Write, process::Command, thread, time::Duration};

const MODULES: &str = __MODULES__;

fn main() {
  let args = env::args().skip(1).collect::<Vec<_>>();
  if let Some(path) = env::var_os("CADDER_TEST_CADDY_COMMAND_LOG") {
    let mut log = OpenOptions::new().create(true).append(true).open(path).unwrap();
    writeln!(log, "{}", args.join(" ")).unwrap();
  }
  match args.first().map(String::as_str) {
    Some("version") => println!("2.11.3"),
    Some("list-modules") => println!("{MODULES}"),
    Some("adapt") => println!("{}", r#"{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["guarded.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}"#),
    Some("run") => {
      println!("guarded runtime stdout");
      eprintln!("guarded runtime stderr");
      let _grandchild = Command::new(env::current_exe().unwrap())
        .arg("grandchild")
        .spawn()
        .unwrap();
      loop { thread::sleep(Duration::from_secs(1)); }
    }
    Some("grandchild") => loop { thread::sleep(Duration::from_secs(1)); },
    Some("stop") | _ => {}
  }
}
"##
    .replace("__MODULES__", &format!("{modules:?}"));
  let source_path = dir.join("guarded-caddy.rs");
  std::fs::write(&source_path, source).unwrap();
  let executable = dir.join(if cfg!(windows) {
    "guarded-caddy.exe"
  } else {
    "guarded-caddy"
  });
  let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
  let output = Command::new(rustc)
    .arg(&source_path)
    .arg("--edition=2024")
    .arg("-o")
    .arg(&executable)
    .output()
    .unwrap();
  assert!(
    output.status.success(),
    "fake Caddy fixture compilation failed: {}",
    String::from_utf8_lossy(&output.stderr)
  );
  executable
}
