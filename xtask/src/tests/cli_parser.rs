  #[test]
  fn cargo_xtask_alias_is_configured() {
    let config_path = workspace_root().join(".cargo").join("config.toml");
    let config = fs::read_to_string(&config_path).unwrap();

    assert!(config.contains("[alias]"), "{config}");
    assert!(config.contains("xtask = \"run -p xtask --\""), "{config}");
  }

  #[test]
  fn docs_site_dir_points_to_docs_package() {
    assert!(docs_site_dir().join("package.json").is_file());
    assert!(docs_site_dir().join("bun.lock").is_file());
  }

  #[test]
  fn coverage_toolchain_selection_respects_override_active_and_windows_fallback() {
    assert_eq!(
      coverage_toolchain_argument(Some("nightly"), Some("stable"), true, true).unwrap(),
      Some("+nightly".to_string())
    );
    assert_eq!(
      coverage_toolchain_argument(Some("+beta"), None, false, false).unwrap(),
      Some("+beta".to_string())
    );
    assert_eq!(
      coverage_toolchain_argument(None, Some("stable-x86_64-pc-windows-msvc"), true, true).unwrap(),
      None
    );
    assert_eq!(
      coverage_toolchain_argument(None, Some("stable-x86_64-pc-windows-gnu"), true, true).unwrap(),
      Some(format!("+{WINDOWS_COVERAGE_TOOLCHAIN}"))
    );
    assert_eq!(
      coverage_toolchain_argument(None, Some("stable"), true, true).unwrap(),
      Some(format!("+{WINDOWS_COVERAGE_TOOLCHAIN}"))
    );
    assert_eq!(
      coverage_toolchain_argument(None, None, true, false).unwrap(),
      Some(format!("+{WINDOWS_COVERAGE_TOOLCHAIN}"))
    );
    assert_eq!(
      coverage_toolchain_argument(None, None, false, false).unwrap(),
      None
    );
  }

  #[test]
  fn workspace_package_version_from_manifest_reads_workspace_package_version() {
    let dir = unique_temp_dir("workspace-version");
    let manifest = dir.join("Cargo.toml");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
      &manifest,
      r#"
[workspace.package]
version = "1.2.3"
edition = "2024"
"#,
    )
    .unwrap();

    assert_eq!(workspace_package_version_from_manifest(&manifest).unwrap(), "1.2.3");
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn workspace_package_version_from_manifest_reports_missing_and_empty_versions() {
    let dir = unique_temp_dir("workspace-version-errors");
    fs::create_dir_all(&dir).unwrap();
    let missing = dir.join("missing-version.toml");
    fs::write(&missing, "[workspace.package]\nedition = \"2024\"\n").unwrap();
    assert!(
      workspace_package_version_from_manifest(&missing)
        .unwrap_err()
        .to_string()
        .contains("version not found")
    );

    let empty = dir.join("empty-version.toml");
    fs::write(&empty, "[workspace.package]\nversion = \"\"\n").unwrap();
    assert!(
      workspace_package_version_from_manifest(&empty)
        .unwrap_err()
        .to_string()
        .contains("version is empty")
    );
    fs::remove_dir_all(&dir).unwrap();
  }
