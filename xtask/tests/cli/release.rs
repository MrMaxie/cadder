#[test]
fn dist_command_builds_release_binaries_and_verifies_layout() {
  let dir = unique_temp_dir("dist-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-cargo.log");
  write_fake_command(&fake_bin, "cargo", 0);
  write_fake_command(&fake_bin, "bun", 0);

  let target = fake_target_name("dist-command");
  let release_dir = workspace_root()
    .join("target")
    .join(&target)
    .join("release");
  fs::create_dir_all(&release_dir).unwrap();
  write_fake_portable_executable_layout_for_target(
    &release_dir,
    &target,
    runtime_portable_binaries(),
  );

  let out_dir = dir.join("dist");
  let output = Command::new(xtask_bin())
    .args(["dist", "--out"])
    .arg(&out_dir)
    .args(["--target", &target])
    .current_dir(workspace_root())
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .output()
    .unwrap();

  assert_success(output);
  let invocation = fs::read_to_string(&log).unwrap();
  assert!(invocation.contains("build --release"), "{invocation}");
  assert!(invocation.contains("-p cadder-shim"), "{invocation}");
  assert!(invocation.contains("--target"), "{invocation}");
  assert!(invocation.contains(&target), "{invocation}");
  for binary in runtime_portable_binaries() {
    assert!(out_dir.join(exe_name_for_target(binary, &target)).is_file());
  }
  assert!(out_dir.join("cadder.toml").is_file());

  fs::remove_dir_all(workspace_root().join("target").join(&target)).unwrap();
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn package_command_builds_dist_and_writes_archive_checksum() {
  let dir = unique_temp_dir("package-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-cargo.log");
  write_fake_command(&fake_bin, "cargo", 0);
  write_fake_command(&fake_bin, "bun", 0);

  let target = format!("fake-windows-package-command-{}", unique_suffix());
  let release_dir = workspace_root()
    .join("target")
    .join(&target)
    .join("release");
  fs::create_dir_all(&release_dir).unwrap();
  write_fake_portable_executable_layout_for_target(
    &release_dir,
    &target,
    runtime_portable_binaries(),
  );

  let out_dir = dir.join("package");
  let output = Command::new(xtask_bin())
    .args(["package", "--out"])
    .arg(&out_dir)
    .args([
      "--platform",
      "windows-x64",
      "--target",
      &target,
      "--version",
      "1.2.3",
    ])
    .current_dir(workspace_root())
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .output()
    .unwrap();

  assert_success(output);
  let invocation = fs::read_to_string(&log).unwrap();
  assert!(invocation.contains("build --release"), "{invocation}");
  assert!(invocation.contains(&target), "{invocation}");
  assert!(out_dir.join("cadder-1.2.3-windows-x64.zip").is_file());
  assert!(
    out_dir
      .join("cadder-1.2.3-windows-x64.zip.sha256")
      .is_file()
  );
  assert!(
    out_dir
      .join("layouts")
      .join("cadder-1.2.3-windows-x64")
      .join("cadder.toml")
      .is_file()
  );

  fs::remove_dir_all(workspace_root().join("target").join(&target)).unwrap();
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn runtime_installer_command_builds_runtime_only_windows_msi_and_manifest() {
  let dir = unique_temp_dir("runtime-installer-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let cargo_log = dir.join("fake-cargo.log");
  let wix_log = dir.join("fake-wix.log");
  write_fake_command(&fake_bin, "cargo", 0);
  write_fake_command(&fake_bin, "bun", 0);
  write_fake_wix(&fake_bin);

  let target = format!("fake-windows-runtime-installer-{}", unique_suffix());
  let release_dir = workspace_root()
    .join("target")
    .join(&target)
    .join("release");
  fs::create_dir_all(&release_dir).unwrap();
  write_fake_portable_executable_layout_for_target(
    &release_dir,
    &target,
    runtime_portable_binaries(),
  );

  let out_dir = dir.join("runtime-installer");
  let output = Command::new(xtask_bin())
    .args(["runtime-installer", "--out"])
    .arg(&out_dir)
    .args([
      "--platform",
      "windows-x64",
      "--target",
      &target,
      "--version",
      "1.2.3",
    ])
    .current_dir(workspace_root())
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &cargo_log)
    .env("CADDER_FAKE_WIX_LOG", &wix_log)
    .output()
    .unwrap();

  assert_success(output);
  let cargo_invocation = fs::read_to_string(&cargo_log).unwrap();
  assert!(
    cargo_invocation.contains("build --release"),
    "{cargo_invocation}"
  );
  let wix_invocation = fs::read_to_string(&wix_log).unwrap();
  assert!(wix_invocation.contains("build"), "{wix_invocation}");
  assert!(wix_invocation.contains("-arch x64"), "{wix_invocation}");

  let installer = out_dir.join("cadder-runtime-1.2.3-windows-x64.msi");
  let manifest = out_dir.join("cadder-runtime-1.2.3-windows-x64.msi.manifest.json");
  assert!(installer.is_file());
  assert!(
    out_dir
      .join("cadder-runtime-1.2.3-windows-x64.msi.sha256")
      .is_file()
  );
  assert!(manifest.is_file());
  assert!(
    fs::read_to_string(&manifest)
      .unwrap()
      .contains(r#""component": "daemon-runtime""#)
  );
  fs::remove_dir_all(workspace_root().join("target").join(&target)).unwrap();
  fs::remove_dir_all(&dir).unwrap();
}
