#[test]
fn coverage_command_invokes_cargo_llvm_cov_and_checks_lcov_threshold() {
  let dir = unique_temp_dir("coverage-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-cargo.log");
  write_fake_command(&fake_bin, "bun", 0);
  write_fake_coverage_cargo(
    &fake_bin,
    0,
    "TN:\nSF:lib.rs\nDA:1,1\nDA:2,1\nDA:3,1\nend_of_record\n",
  );

  let report = dir.join("reports").join("summary.lcov");
  let output = Command::new(xtask_bin())
    .args(["coverage", "--output"])
    .arg(&report)
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .output()
    .unwrap();

  assert_success(output);
  assert!(report.parent().unwrap().is_dir());
  let invocation = fs::read_to_string(&log).unwrap();
  assert!(invocation.contains("llvm-cov"), "{invocation}");
  assert!(invocation.contains("--workspace"), "{invocation}");
  assert!(invocation.contains("--lcov"), "{invocation}");
  assert!(invocation.contains("--output-path"), "{invocation}");
  assert!(
    invocation.contains(&report.display().to_string()),
    "{invocation}"
  );
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn coverage_command_reports_lcov_below_threshold_after_cargo_success() {
  let dir = unique_temp_dir("coverage-command-low-report");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  write_fake_command(&fake_bin, "bun", 0);
  write_fake_coverage_cargo(
    &fake_bin,
    0,
    "TN:\nSF:lib.rs\nDA:1,1\nDA:2,1\nDA:3,0\nend_of_record\n",
  );

  let output = Command::new(xtask_bin())
    .args(["coverage", "--output"])
    .arg(dir.join("summary.lcov"))
    .env("PATH", &fake_bin)
    .output()
    .unwrap();

  assert!(!output.status.success());
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("below required 85.00%"), "{stderr}");
  assert!(stderr.contains("2/3"), "{stderr}");
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn coverage_command_reports_cargo_failure() {
  let dir = unique_temp_dir("coverage-command-failure");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  write_fake_command(&fake_bin, "bun", 0);
  write_fake_command(&fake_bin, "cargo", 7);

  let output = Command::new(xtask_bin())
    .args(["coverage", "--output"])
    .arg(dir.join("summary.lcov"))
    .env("PATH", &fake_bin)
    .output()
    .unwrap();

  assert!(!output.status.success());
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("cargo"), "{stderr}");
  assert!(stderr.contains("failed"), "{stderr}");
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn dev_env_command_outputs_mock_backend_configuration() {
  let output = Command::new(xtask_bin())
    .args(["dev-env", "--format", "json"])
    .output()
    .unwrap();

  let stdout = output.stdout.clone();
  assert_success(output);
  let value: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
  assert_eq!(value["CADDER_CADDY_BACKEND"], "mock");
  assert!(value.get("CADDER_DESKTOP_DEV_WINDOWS").is_none());
}

#[test]
fn dev_run_command_invokes_program_with_dev_environment() {
  let dir = unique_temp_dir("dev-run-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-tool.log");
  let env_log = dir.join("fake-tool-env.log");
  write_fake_command(&fake_bin, "dev-helper", 0);

  let output = Command::new(xtask_bin())
    .args(["dev-run", "--", "dev-helper", "status"])
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .env("CADDER_FAKE_TOOL_ENV_LOG", &env_log)
    .output()
    .unwrap();

  assert_success(output);
  assert!(fs::read_to_string(&log).unwrap().contains("status"));
  let env_value = fs::read_to_string(&env_log).unwrap();
  assert!(
    env_value.contains("CADDER_CADDY_BACKEND=mock"),
    "{env_value}"
  );
  fs::remove_dir_all(&dir).unwrap();
}
