  #[test]
  fn verify_release_assets_accepts_complete_dry_run_matrix() {
    let dir = unique_temp_dir("release-assets");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");

    verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap();

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_assets_rejects_missing_portable_checksum() {
    let dir = unique_temp_dir("release-assets-missing-checksum");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");
    fs::remove_file(dir.join("cadder-1.2.3-windows-x64.zip.sha256")).unwrap();

    let error = verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap_err();
    assert!(
      error
        .to_string()
        .contains("cadder-1.2.3-windows-x64.zip.sha256")
    );

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_assets_rejects_portable_archive_with_wrong_contents() {
    let dir = unique_temp_dir("release-assets-wrong-archive");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");
    let archive = dir.join("cadder-1.2.3-windows-x64.zip");
    fs::remove_file(&archive).unwrap();
    fs::remove_file(dir.join("cadder-1.2.3-windows-x64.zip.sha256")).unwrap();
    write_release_asset(&dir, "cadder-1.2.3-windows-x64.zip");

    let error = verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap_err();
    assert!(error.to_string().contains("read ZIP"));

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_assets_rejects_missing_runtime_installer() {
    let dir = unique_temp_dir("release-assets-missing-runtime-installer");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");
    fs::remove_file(dir.join("cadder-runtime-1.2.3-windows-x64.msi")).unwrap();

    let error = verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap_err();
    assert!(
      error
        .to_string()
        .contains("cadder-runtime-1.2.3-windows-x64.msi")
    );

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_assets_rejects_runtime_installer_manifest_with_extra_install_path() {
    let dir = unique_temp_dir("release-assets-runtime-manifest-extra-path");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");
    let manifest = dir.join("cadder-runtime-1.2.3-windows-x64.msi.manifest.json");
    let mut value = read_json_value(&manifest).unwrap();
    value["installPaths"]
      .as_array_mut()
      .unwrap()
      .push(JsonValue::String(
        r"C:\Program Files\Cadder\extra-tool.exe".to_string(),
      ));
    fs::write(&manifest, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    write_sha256_file(&manifest, &checksum_path_for(&manifest).unwrap()).unwrap();

    let error = verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap_err();
    assert!(
      error
        .to_string()
        .contains("contains unexpected install path"),
      "{error}"
    );

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_assets_rejects_unexpected_runtime_installer_asset() {
    let dir = unique_temp_dir("release-assets-unexpected-runtime-installer");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");
    fs::write(dir.join("cadder-runtime-1.2.3-linux-x64.AppImage"), b"bad").unwrap();

    let error = verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap_err();
    assert!(
      error
        .to_string()
        .contains("unexpected runtime installer release asset")
    );

    fs::remove_dir_all(&dir).unwrap();
  }
