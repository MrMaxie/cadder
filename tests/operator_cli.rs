use std::{
  env, fs,
  io::{Read, Seek},
  path::{Path, PathBuf},
  process::{Command, Output, Stdio},
  time::Duration,
};

use tempfile::TempDir;
use wait_timeout::ChildExt;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const DAEMON_START_TIMEOUT: Duration = Duration::from_secs(45);

struct OperatorHarness {
  _temp: TempDir,
  binary_dir: PathBuf,
  runtime_dir: PathBuf,
  operator: PathBuf,
}

impl OperatorHarness {
  fn new() -> Self {
    let temp = tempfile::tempdir().expect("create operator test directory");
    let binary_dir = temp.path().join("bin");
    let runtime_dir = temp.path().join("runtime");
    fs::create_dir_all(&binary_dir).expect("create operator test binary directory");
    let operator = install_binary(env!("CARGO_BIN_EXE_cadder"), &binary_dir, "cadder");
    install_binary(env!("CARGO_BIN_EXE_cadderd"), &binary_dir, "cadderd");
    install_binary(env!("CARGO_BIN_EXE_caddy"), &binary_dir, "caddy");
    Self {
      _temp: temp,
      binary_dir,
      runtime_dir,
      operator,
    }
  }

  fn run(&self, arguments: &[&str]) -> Output {
    self.run_with_timeout(arguments, COMMAND_TIMEOUT)
  }

  fn run_with_timeout(&self, arguments: &[&str], timeout: Duration) -> Output {
    self
      .try_run_with_timeout(arguments, timeout)
      .unwrap_or_else(|error| panic!("run cadder {arguments:?}: {error}"))
  }

  fn try_run(&self, arguments: &[&str]) -> Result<Output, String> {
    self.try_run_with_timeout(arguments, COMMAND_TIMEOUT)
  }

  fn try_run_with_timeout(&self, arguments: &[&str], timeout: Duration) -> Result<Output, String> {
    let path = controlled_path(&self.binary_dir)?;
    let mut stdout = tempfile::tempfile_in(self._temp.path()).map_err(|error| error.to_string())?;
    let mut stderr = tempfile::tempfile_in(self._temp.path()).map_err(|error| error.to_string())?;
    let mut child = Command::new(&self.operator)
      .args(arguments)
      .env("CADDER_CADDY_BACKEND", "mock")
      .env("CADDER_RUNTIME_DIR", &self.runtime_dir)
      .env("PATH", path)
      .current_dir(self._temp.path())
      .stdout(Stdio::from(
        stdout.try_clone().map_err(|error| error.to_string())?,
      ))
      .stderr(Stdio::from(
        stderr.try_clone().map_err(|error| error.to_string())?,
      ))
      .spawn()
      .map_err(|error| error.to_string())?;

    let completed = child
      .wait_timeout(timeout)
      .map_err(|error| error.to_string())?;
    let timed_out = completed.is_none();
    let status = if let Some(status) = completed {
      status
    } else {
      let _ = child.kill();
      child.wait().map_err(|error| error.to_string())?
    };
    let output = Output {
      status,
      stdout: read_capture(&mut stdout)?,
      stderr: read_capture(&mut stderr)?,
    };

    if timed_out {
      return Err(format!(
        "timed out after {} seconds\nstdout:\n{}\nstderr:\n{}",
        timeout.as_secs(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
      ));
    }

    Ok(output)
  }

  fn succeeds(&self, arguments: &[&str]) -> String {
    let output = self.run(arguments);
    successful_stdout(arguments, output)
  }

  fn succeeds_with_timeout(&self, arguments: &[&str], timeout: Duration) -> String {
    let output = self.run_with_timeout(arguments, timeout);
    successful_stdout(arguments, output)
  }
}

fn successful_stdout(arguments: &[&str], output: Output) -> String {
  assert!(
    output.status.success(),
    "cadder {arguments:?} failed with {:?}:\nstdout:\n{}\nstderr:\n{}",
    output.status.code(),
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  );
  String::from_utf8(output.stdout).expect("operator stdout should be UTF-8")
}

impl Drop for OperatorHarness {
  fn drop(&mut self) {
    let _ = self.try_run(&["daemon", "stop"]);
  }
}

fn controlled_path(binary_dir: &Path) -> Result<std::ffi::OsString, String> {
  let mut paths = vec![binary_dir.to_path_buf()];
  if let Some(path) = env::var_os("PATH") {
    paths.extend(env::split_paths(&path));
  }
  env::join_paths(paths).map_err(|error| error.to_string())
}

fn read_capture(file: &mut fs::File) -> Result<Vec<u8>, String> {
  file.rewind().map_err(|error| error.to_string())?;
  let mut bytes = Vec::new();
  file
    .read_to_end(&mut bytes)
    .map_err(|error| error.to_string())?;
  Ok(bytes)
}

fn install_binary(source: &str, directory: &Path, name: &str) -> PathBuf {
  let extension = if cfg!(windows) { ".exe" } else { "" };
  let destination = directory.join(format!("{name}{extension}"));
  fs::copy(source, &destination).unwrap_or_else(|error| {
    panic!(
      "copy test binary from {source} to {}: {error}",
      destination.display()
    )
  });
  destination
}

#[test]
fn operator_cli_manages_and_inspects_an_isolated_daemon() {
  let harness = OperatorHarness::new();

  assert_ne!(
    harness.runtime_dir,
    harness
      .operator
      .parent()
      .expect("operator binary should have a parent directory")
  );

  assert!(harness.succeeds(&["--version"]).starts_with("cadder "));
  assert_eq!(
    harness
      .run(&["logs", "runtime", "--limit", "0"])
      .status
      .code(),
    Some(2)
  );

  let started = harness.succeeds_with_timeout(&["daemon", "start"], DAEMON_START_TIMEOUT);
  assert!(started.contains("cadderd is running"));
  assert!(
    harness
      .runtime_dir
      .join("data")
      .join("cadder.sqlite3")
      .is_file(),
    "isolated daemon should create its database under CADDER_RUNTIME_DIR"
  );
  assert!(
    !harness.binary_dir.join("data").exists(),
    "isolated daemon must not derive runtime state from the copied executable"
  );

  let status = harness.succeeds(&["status"]);
  assert!(status.contains("cadderd  running"));
  assert!(status.contains("Caddy    idle"));

  assert!(
    harness
      .succeeds(&["projects", "list"])
      .contains("No projects registered")
  );
  assert!(
    harness
      .succeeds(&["domains", "list"])
      .contains("No domains registered")
  );
  assert!(
    harness
      .succeeds(&["diagnostics"])
      .contains("Runtime diagnostics")
  );
  assert!(
    harness
      .succeeds(&["logs", "runtime", "--limit", "10"])
      .contains("Channel  control")
  );

  let caddyfile = harness._temp.path().join("missing").join("Caddyfile");
  let caddyfile_output = harness.succeeds(&[
    "caddyfile",
    "inspect",
    caddyfile.to_str().expect("temporary path should be UTF-8"),
  ]);
  assert!(caddyfile_output.contains("Registered no"));

  let port_output = harness.succeeds(&["port", "inspect", "49151"]);
  assert!(port_output.contains("No matching routes"));

  for arguments in [
    vec![
      "projects",
      "enable",
      caddyfile.to_str().expect("temporary path should be UTF-8"),
    ],
    vec![
      "projects",
      "disable",
      caddyfile.to_str().expect("temporary path should be UTF-8"),
    ],
    vec![
      "logs",
      "project",
      caddyfile.to_str().expect("temporary path should be UTF-8"),
    ],
    vec!["domains", "inspect", "missing.localhost"],
    vec![
      "domains",
      "inspect",
      "missing.localhost",
      "--caddyfile",
      caddyfile.to_str().expect("temporary path should be UTF-8"),
    ],
    vec!["domains", "enable", "missing.localhost"],
    vec![
      "domains",
      "enable",
      "missing.localhost",
      "--caddyfile",
      caddyfile.to_str().expect("temporary path should be UTF-8"),
    ],
    vec!["domains", "disable", "missing.localhost"],
    vec!["logs", "domain", "missing.localhost"],
    vec![
      "logs",
      "domain",
      "missing.localhost",
      "--caddyfile",
      caddyfile.to_str().expect("temporary path should be UTF-8"),
    ],
  ] {
    let output = harness.run(&arguments);
    assert_eq!(output.status.code(), Some(5), "arguments: {arguments:?}");
  }

  let kill = harness.run(&["port", "kill", "49151", "--pid", "4294967295"]);
  assert_eq!(kill.status.code(), Some(6));

  assert!(
    harness
      .succeeds_with_timeout(&["daemon", "restart"], DAEMON_START_TIMEOUT)
      .contains("cadderd restarted")
  );
  assert!(
    harness
      .succeeds(&["daemon", "stop"])
      .contains("cadderd stop requested")
  );
}
