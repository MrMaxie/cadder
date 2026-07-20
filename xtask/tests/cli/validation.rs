#[test]
fn check_command_runs_validation_pipeline_with_fake_cargo() {
  let dir = unique_temp_dir("check-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-tool.log");
  write_fake_command(&fake_bin, "cargo", 0);
  write_fake_command(&fake_bin, "bun", 0);
  write_fake_openspec(&fake_bin, "1.5.0");

  let output = Command::new(xtask_bin())
    .arg("check")
    .current_dir(workspace_root())
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .output()
    .unwrap();

  assert_success(output);
  let invocations = fs::read_to_string(&log).unwrap();
  assert!(invocations.contains("fmt --check"), "{invocations}");
  assert!(invocations.contains("doctor --json"), "{invocations}");
  assert!(
    invocations.contains("schema validate implementation --json"),
    "{invocations}"
  );
  assert!(
    invocations.contains("validate --specs --strict --json"),
    "{invocations}"
  );
  assert!(
    invocations.contains("clippy --workspace --all-targets -- -D warnings"),
    "{invocations}"
  );
  assert!(invocations.contains("test --workspace"), "{invocations}");
  assert!(
    invocations.contains("install --frozen-lockfile"),
    "{invocations}"
  );
  assert!(invocations.contains("run check"), "{invocations}");
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn openspec_check_command_enforces_version_and_runs_schema_aware_validation() {
  let dir = unique_temp_dir("openspec-check-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-openspec.log");
  write_fake_openspec(&fake_bin, "1.5.0");

  let output = Command::new(xtask_bin())
    .arg("openspec-check")
    .current_dir(workspace_root())
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .output()
    .unwrap();

  assert_success(output);
  let invocations = fs::read_to_string(&log).unwrap();
  assert!(invocations.contains("--version"), "{invocations}");
  assert!(invocations.contains("doctor --json"), "{invocations}");
  assert!(
    invocations.contains("schema validate implementation --json"),
    "{invocations}"
  );
  assert!(
    invocations.contains("validate --specs --strict --json"),
    "{invocations}"
  );
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn openspec_check_command_rejects_a_different_cli_version() {
  let dir = unique_temp_dir("openspec-check-version");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  write_fake_openspec(&fake_bin, "1.6.0");

  let output = Command::new(xtask_bin())
    .arg("openspec-check")
    .current_dir(workspace_root())
    .env("PATH", &fake_bin)
    .output()
    .unwrap();

  assert!(!output.status.success());
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("OpenSpec 1.5.0 is required"), "{stderr}");
  assert!(stderr.contains("1.6.0"), "{stderr}");
  fs::remove_dir_all(&dir).unwrap();
}
