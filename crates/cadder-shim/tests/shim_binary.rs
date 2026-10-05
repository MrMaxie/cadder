use std::{
  fs,
  path::PathBuf,
  process::{Command, Output},
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
