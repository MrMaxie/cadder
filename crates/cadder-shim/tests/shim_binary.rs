use std::{
  env, fs,
  io::Write,
  path::PathBuf,
  process::{Command, Output, Stdio},
};

fn run_shim(args: &[&str]) -> Output {
  Command::new(env!("CARGO_BIN_EXE_cadder-shim"))
    .args(args)
    .output()
    .unwrap()
}

fn unique_runtime_dir(name: &str) -> PathBuf {
  std::env::temp_dir().join(format!(
    "cadder-shim-{name}-{}-{}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_nanos()
  ))
}

fn native_fixture_path() -> PathBuf {
  let shim = PathBuf::from(env!("CARGO_BIN_EXE_cadder-shim"));
  let profile = shim.parent().expect("shim binary has a profile directory");
  let path = profile.join(if cfg!(windows) {
    "cadder-test-process.exe"
  } else {
    "cadder-test-process"
  });
  assert!(
    path.is_file(),
    "missing native test process at {}; build it with `cargo build -p cadder-daemon --bin cadder-test-process` before this focused test",
    path.display()
  );
  path
}

fn run_passthrough(mode: &str, input: &[u8], args: &[&str]) -> (tempfile::TempDir, Output) {
  let temp = tempfile::tempdir().unwrap();
  let fixture_name = if cfg!(windows) {
    "caddy-test-process.exe"
  } else {
    "caddy-test-process"
  };
  let fixture = temp.path().join(fixture_name);
  fs::copy(native_fixture_path(), &fixture).unwrap();
  fs::write(temp.path().join("cadder-test.mode"), mode).unwrap();
  // Copying the released shim makes its portable trusted config path disposable and deterministic.
  let portable_config = format!("[caddy]\nreal_command = \"{fixture_name}\"\n");
  fs::write(temp.path().join("cadder.toml"), portable_config).unwrap();
  let shim_name = if cfg!(windows) {
    "cadder-shim.exe"
  } else {
    "cadder-shim"
  };
  let shim = temp.path().join(shim_name);
  fs::copy(env!("CARGO_BIN_EXE_cadder-shim"), &shim).unwrap();

  let path = env::join_paths([temp.path()]).unwrap();
  let mut child = Command::new(&shim)
    .args(args)
    .env("PATH", path)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
  let mut input_pipe = child.stdin.take().expect("shim stdin is piped");
  input_pipe.write_all(input).unwrap();
  drop(input_pipe);
  (temp, wait_for_output(child))
}

fn wait_for_output(mut child: std::process::Child) -> Output {
  let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
  loop {
    if child.try_wait().unwrap().is_some() {
      return child.wait_with_output().unwrap();
    }
    if std::time::Instant::now() >= deadline {
      let _ = child.kill();
      let output = child.wait_with_output().unwrap();
      panic!(
        "released shim passthrough exceeded 10s; stdout={:?}, stderr={:?}",
        output.stdout, output.stderr
      );
    }
    std::thread::sleep(std::time::Duration::from_millis(10));
  }
}

fn passthrough_record(temp: &tempfile::TempDir) -> serde_json::Value {
  serde_json::from_slice(&fs::read(temp.path().join("shim-passthrough.json")).unwrap()).unwrap()
}

#[test]
fn real_caddy_passthrough_preserves_arguments_stdin_streams_and_success_exit() {
  let args = [
    "fmt",
    "name with spaces",
    "żółć",
    "quote\"value",
    "apostrophe's",
  ];
  let input = b"stdin bytes \xE2\x9C\x93\n";
  let (temp, output) = run_passthrough("shim-passthrough-success", input, &args);

  assert_eq!(output.status.code(), Some(0));
  assert_eq!(output.stdout, b"FAKE_CADDY_PASSTHROUGH_STDOUT\n");
  assert_eq!(output.stderr, b"FAKE_CADDY_PASSTHROUGH_STDERR\n");
  let record = passthrough_record(&temp);
  assert_eq!(record["args"], serde_json::json!(args));
  assert_eq!(record["stdin_bytes"], serde_json::json!(input));
}

#[test]
fn real_caddy_passthrough_propagates_nonzero_child_exit() {
  let (temp, output) = run_passthrough(
    "shim-passthrough-failure",
    b"failure input",
    &["fmt", "--literal"],
  );

  assert_eq!(output.status.code(), Some(23));
  assert_eq!(output.stdout, b"FAKE_CADDY_PASSTHROUGH_STDOUT\n");
  assert_eq!(output.stderr, b"FAKE_CADDY_PASSTHROUGH_STDERR\n");
  let record = passthrough_record(&temp);
  assert_eq!(record["args"], serde_json::json!(["fmt", "--literal"]));
  assert_eq!(record["stdin_bytes"], serde_json::json!(b"failure input"));
}

#[test]
fn shim_info_flag_reports_release_identity_json() {
  let output = run_shim(&["--cadder-shim-info"]);

  assert!(output.status.success());
  let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
  assert_eq!(json["role"], "caddy-shim");
  assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
  assert!(json["executable"].as_str().is_some());
}

#[test]
fn shim_info_flag_ignores_invalid_backend_env() {
  let output = Command::new(env!("CARGO_BIN_EXE_cadder-shim"))
    .arg("--cadder-shim-info")
    .env("CADDER_CADDY_BACKEND", "invalid")
    .output()
    .unwrap();

  assert!(output.status.success());
  let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
  assert_eq!(json["role"], "caddy-shim");
}

#[test]
fn shim_real_caddy_selector_is_rejected() {
  let selected_executable = env!("CARGO_BIN_EXE_cadder-shim");
  let output = run_shim(&[
    "--cadder-real-caddy-command",
    selected_executable,
    "version",
  ]);

  assert_eq!(output.status.code(), Some(1));
  let stderr = String::from_utf8(output.stderr).unwrap();
  assert!(
    stderr.contains("shim cannot select the executable"),
    "{stderr}"
  );
}

#[test]
fn unsupported_command_does_not_delegate_to_real_caddy() {
  let output = run_shim(&["--cadder-caddy-backend", "mock", "start"]);

  assert_eq!(output.status.code(), Some(1));
  assert!(
    !String::from_utf8(output.stdout)
      .unwrap()
      .contains("delegated start")
  );
  let stderr = String::from_utf8(output.stderr).unwrap();
  assert!(
    stderr.contains("Cadder shim does not support `caddy start`"),
    "{stderr}"
  );
}

#[test]
fn managed_run_reports_missing_backend_without_runtime_selection() {
  let runtime_dir = unique_runtime_dir("missing-backend");
  let missing_daemon = runtime_dir.join(if cfg!(windows) {
    "missing-cadderd.exe"
  } else {
    "missing-cadderd"
  });
  let missing_daemon_arg = missing_daemon.display().to_string();

  let output = run_shim(&[
    "--cadder-daemon-path",
    &missing_daemon_arg,
    "--cadder-caddy-backend",
    "mock",
    "run",
  ]);

  assert_eq!(output.status.code(), Some(1));
  let stderr = String::from_utf8(output.stderr).unwrap();
  assert!(
    stderr.contains("Cadder could not recover `caddy run`"),
    "{stderr}"
  );
  let _ = fs::remove_dir_all(runtime_dir);
}

#[test]
fn managed_run_does_not_delegate_to_real_caddy_when_daemon_is_missing() {
  let runtime_dir = unique_runtime_dir("managed-no-delegate");
  let missing_daemon = runtime_dir.join(if cfg!(windows) {
    "missing-cadderd.exe"
  } else {
    "missing-cadderd"
  });
  let missing_daemon_arg = missing_daemon.display().to_string();
  let output = run_shim(&["--cadder-daemon-path", &missing_daemon_arg, "run"]);

  assert_eq!(output.status.code(), Some(1));
  assert!(
    !String::from_utf8(output.stdout)
      .unwrap()
      .contains("delegated run")
  );
  let stderr = String::from_utf8(output.stderr).unwrap();
  assert!(
    stderr.contains("Managed `caddy run` was not delegated to real Caddy"),
    "{stderr}"
  );
  let _ = fs::remove_dir_all(runtime_dir);
}
