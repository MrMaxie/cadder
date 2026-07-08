use std::{
  fs,
  path::PathBuf,
  process::{Command, Output},
};

fn run_shim(args: &[&str]) -> Output {
  Command::new(env!("CARGO_BIN_EXE_caddy"))
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

fn write_fake_real_caddy(name: &str, exit_code: u8) -> PathBuf {
  let dir = unique_runtime_dir(name);
  fs::create_dir_all(&dir).unwrap();

  #[cfg(windows)]
  {
    let path = dir.join("real-caddy.cmd");
    fs::write(
      &path,
      format!(
        r#"@echo off
echo delegated %*
exit /b {exit_code}
"#
      ),
    )
    .unwrap();
    path
  }

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("real-caddy");
    fs::write(
      &path,
      format!(
        r#"#!/usr/bin/env sh
printf '%s\n' "delegated $*"
exit {exit_code}
"#
      ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
  }
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
  let output = Command::new(env!("CARGO_BIN_EXE_caddy"))
    .arg("--cadder-shim-info")
    .env("CADDER_CADDY_BACKEND", "invalid")
    .output()
    .unwrap();

  assert!(output.status.success());
  let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
  assert_eq!(json["role"], "caddy-shim");
}

#[test]
fn delegated_command_reports_real_caddy_resolution_failure() {
  let output = run_shim(&[
    "--cadder-real-caddy-command",
    "definitely-missing-caddy",
    "version",
  ]);

  assert_eq!(output.status.code(), Some(1));
  let stderr = String::from_utf8(output.stderr).unwrap();
  assert!(
    stderr.contains("could not resolve a safe real Caddy binary"),
    "{stderr}"
  );
}

#[test]
fn delegated_command_propagates_real_caddy_exit_code() {
  let fake_caddy = write_fake_real_caddy("delegated-exit", 7);
  let fake_caddy_arg = fake_caddy.display().to_string();

  let output = run_shim(&["--cadder-real-caddy-command", &fake_caddy_arg, "version"]);

  assert_eq!(output.status.code(), Some(7));
  assert!(
    String::from_utf8(output.stdout)
      .unwrap()
      .contains("delegated version")
  );
  let _ = fs::remove_dir_all(fake_caddy.parent().unwrap());
}

#[test]
fn unsupported_command_does_not_delegate_to_real_caddy() {
  let fake_caddy = write_fake_real_caddy("unsupported-start", 0);
  let fake_caddy_arg = fake_caddy.display().to_string();

  let output = run_shim(&["--cadder-real-caddy-command", &fake_caddy_arg, "start"]);

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
  let _ = fs::remove_dir_all(fake_caddy.parent().unwrap());
}

#[test]
fn managed_run_reports_missing_backend_for_runtime_dir() {
  let runtime_dir = unique_runtime_dir("missing-backend");
  let runtime_dir_arg = runtime_dir.display().to_string();
  let missing_daemon = runtime_dir.join(if cfg!(windows) {
    "missing-cadderd.exe"
  } else {
    "missing-cadderd"
  });
  let missing_daemon_arg = missing_daemon.display().to_string();

  let output = run_shim(&[
    "--cadder-runtime-dir",
    &runtime_dir_arg,
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
