#[test]
fn verify_dist_command_accepts_fake_portable_layout() {
  let dir = unique_temp_dir("verify-dist");
  fs::create_dir_all(&dir).unwrap();
  write_fake_portable_executable_layout(&dir, runtime_portable_binaries(), true);

  let output = run_xtask(["verify-dist", "--dir"], Some(&dir));

  assert_success(output);
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn help_and_list_commands_are_lightweight_and_discover_docs_release_and_dev_tasks() {
  let help = Command::new(xtask_bin()).arg("--help").output().unwrap();
  let help_stdout = help.stdout.clone();
  assert_success(help);
  let help_text = String::from_utf8_lossy(&help_stdout);
  assert!(
    help_text.contains("Cadder repository task runner"),
    "{help_text}"
  );
  assert!(help_text.contains("Usage: xtask"), "{help_text}");
  assert!(help_text.contains("docs-check"), "{help_text}");
  assert!(help_text.contains("openspec-check"), "{help_text}");
  assert!(help_text.contains("verify-assets"), "{help_text}");
  assert!(help_text.contains("verify-release-assets"), "{help_text}");
  assert!(
    help_text.contains("verify-workspace-topology"),
    "{help_text}"
  );
  assert!(help_text.contains("dev-run"), "{help_text}");

  let list = Command::new(xtask_bin()).arg("list").output().unwrap();
  let list_stdout = list.stdout.clone();
  assert_success(list);
  let list_text = String::from_utf8_lossy(&list_stdout);
  assert!(list_text.lines().any(|line| line == "check"), "{list_text}");
  assert!(
    list_text.lines().any(|line| line == "docs-build"),
    "{list_text}"
  );
  assert!(
    list_text.lines().any(|line| line == "openspec-check"),
    "{list_text}"
  );
  assert!(
    list_text.lines().any(|line| line == "runtime-installer"),
    "{list_text}"
  );
  assert!(
    list_text.lines().any(|line| line == "verify-assets"),
    "{list_text}"
  );
  assert!(
    list_text
      .lines()
      .any(|line| line == "verify-workspace-topology"),
    "{list_text}"
  );

  let openspec_help = Command::new(xtask_bin())
    .args(["help", "openspec-check"])
    .output()
    .unwrap();
  let openspec_help_stdout = openspec_help.stdout.clone();
  assert_success(openspec_help);
  let openspec_help_text = String::from_utf8_lossy(&openspec_help_stdout);
  assert!(
    openspec_help_text.contains("openspec-check"),
    "{openspec_help_text}"
  );
}

#[test]
fn verify_assets_command_accepts_current_repo_contract() {
  let output = Command::new(xtask_bin())
    .arg("verify-assets")
    .output()
    .unwrap();

  assert_success(output);
}

#[test]
fn verify_workspace_topology_command_accepts_current_repo_contract() {
  let output = Command::new(xtask_bin())
    .arg("verify-workspace-topology")
    .output()
    .unwrap();

  assert_success(output);
}

#[test]
fn docs_commands_install_dependencies_and_run_docs_scripts_from_docs_site() {
  let dir = unique_temp_dir("docs-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-bun.log");
  let cwd_log = dir.join("fake-bun-cwd.log");
  write_fake_command(&fake_bin, "bun", 0);

  let check = Command::new(xtask_bin())
    .arg("docs-check")
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .env("CADDER_FAKE_TOOL_CWD_LOG", &cwd_log)
    .output()
    .unwrap();
  assert_success(check);

  let build = Command::new(xtask_bin())
    .arg("docs-build")
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .env("CADDER_FAKE_TOOL_CWD_LOG", &cwd_log)
    .output()
    .unwrap();
  assert_success(build);

  let invocations = fs::read_to_string(&log).unwrap();
  assert!(
    invocations.matches("install --frozen-lockfile").count() >= 2,
    "{invocations}"
  );
  assert!(invocations.contains("run check"), "{invocations}");
  assert!(invocations.contains("run build"), "{invocations}");
  let cwd = fs::read_to_string(&cwd_log).unwrap().replace('\\', "/");
  assert!(cwd.contains("docs/site"), "{cwd}");
  fs::remove_dir_all(&dir).unwrap();
}
