  #[test]
  fn verify_workspace_topology_manifest_accepts_expected_members() {
    let dir = unique_temp_dir("workspace-topology");
    fs::create_dir_all(&dir).unwrap();
    write_workspace_topology_fixture(&dir);

    verify_workspace_topology_manifest(&dir.join("Cargo.toml")).unwrap();

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_workspace_topology_manifest_rejects_undocumented_member() {
    let dir = unique_temp_dir("workspace-topology-extra");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
      dir.join("Cargo.toml"),
      workspace_topology_manifest(&["crates/cadder2"]),
    )
    .unwrap();

    let error = verify_workspace_topology_manifest(&dir.join("Cargo.toml")).unwrap_err();

    assert!(error.to_string().contains("unexpected: crates/cadder2"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_workspace_topology_manifest_rejects_wrong_member_package_name() {
    let dir = unique_temp_dir("workspace-topology-wrong-package");
    fs::create_dir_all(&dir).unwrap();
    write_workspace_topology_fixture(&dir);
    let cadder = WORKSPACE_MEMBER_CONTRACTS
      .iter()
      .find(|contract| contract.path == "crates/cadder-client")
      .copied()
      .unwrap();
    write_workspace_member_manifest(&dir, cadder, "cadder-web");

    let error = verify_workspace_topology_manifest(&dir.join("Cargo.toml")).unwrap_err();

    assert!(
      error
        .to_string()
        .contains("package name must be \"cadder-client\"")
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_identity_accepts_current_repo_metadata() {
    verify_release_identity_files(
      Path::new(WORKSPACE_MANIFEST),
      &workspace_root().join(DOCS_DOWNLOAD_SCRIPT),
    )
    .unwrap();
  }

  #[test]
  fn verify_assets_accepts_current_repo_contract() {
    verify_assets_at(&workspace_root()).unwrap();
  }

  #[test]
  fn verify_canonical_asset_copies_rejects_drifted_docs_copy() {
    let dir = unique_temp_dir("asset-copy-drift");
    fs::create_dir_all(dir.join("assets")).unwrap();
    fs::create_dir_all(dir.join("docs/site/src/assets")).unwrap();
    fs::write(dir.join("assets/logo.png"), b"canonical").unwrap();
    fs::write(dir.join("docs/site/src/assets/logo.png"), b"copy").unwrap();

    let error = verify_canonical_asset_copies(
      &dir,
      &[(
        "docs logo pipeline copy",
        "assets/logo.png",
        "docs/site/src/assets/logo.png",
      )],
    )
    .unwrap_err();

    assert!(
      error
        .to_string()
        .contains("docs logo pipeline copy at docs/site/src/assets/logo.png differs")
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_obsolete_assets_absent_rejects_scaffold_asset() {
    let dir = unique_temp_dir("obsolete-asset");
    fs::create_dir_all(dir.join("scaffold/public")).unwrap();
    fs::write(dir.join("scaffold/public/vite.svg"), "<svg />").unwrap();

    let error =
      verify_obsolete_assets_absent(&dir, &[("Vite starter logo", "scaffold/public/vite.svg")])
        .unwrap_err();

    assert!(error.to_string().contains("obsolete scaffold asset"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_docs_webmanifest_rejects_empty_product_name() {
    let dir = unique_temp_dir("webmanifest-product-name");
    let public_dir = dir.join("docs/site/public");
    fs::create_dir_all(&public_dir).unwrap();
    fs::write(public_dir.join("android-chrome-192x192.png"), b"icon").unwrap();
    let manifest = public_dir.join("site.webmanifest");
    fs::write(
      &manifest,
      serde_json::to_string_pretty(&json!({
        "name": "",
        "short_name": "Cadder",
        "icons": [
          {
            "src": "/android-chrome-192x192.png",
            "sizes": "192x192",
            "type": "image/png",
          },
        ],
      }))
      .unwrap(),
    )
    .unwrap();

    let error = verify_docs_webmanifest(&dir, &manifest).unwrap_err();

    assert!(error.to_string().contains("web manifest name"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_docs_download_metadata_rejects_missing_asset_pattern() {
    let dir = unique_temp_dir("download-metadata-drift");
    let script = dir.join("cadder-downloads.js");
    fs::create_dir_all(&dir).unwrap();
    fs::write(&script, "const cadderRuntimeAssetPatterns = {};\n").unwrap();

    let error = verify_docs_download_metadata(&script).unwrap_err();

    assert!(error.to_string().contains("runtime Windows archive"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_profile_manifest_accepts_expected_policy() {
    let dir = unique_temp_dir("release-profile");
    let manifest = dir.join("Cargo.toml");
    fs::create_dir_all(&dir).unwrap();
    fs::write(&manifest, expected_release_profile_manifest()).unwrap();

    verify_release_profile_manifest(&manifest).unwrap();

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_profile_manifest_rejects_drifted_release_setting() {
    let dir = unique_temp_dir("release-profile-drift");
    let manifest = dir.join("Cargo.toml");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
      &manifest,
      expected_release_profile_manifest().replace("opt-level = \"s\"", "opt-level = 3"),
    )
    .unwrap();

    let error = verify_release_profile_manifest(&manifest).unwrap_err();

    assert!(
      error
        .to_string()
        .contains("profile.release.opt-level expected \"s\", found 3")
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_profile_manifest_rejects_missing_profiling_setting() {
    let dir = unique_temp_dir("release-profile-missing");
    let manifest = dir.join("Cargo.toml");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
      &manifest,
      expected_release_profile_manifest().replace("strip = \"none\"\n", ""),
    )
    .unwrap();

    let error = verify_release_profile_manifest(&manifest).unwrap_err();

    assert!(
      error
        .to_string()
        .contains("profile.profiling.strip missing")
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn release_platform_parses_and_infers_targets() {
    assert_eq!(
      ReleasePlatform::infer_from_target(Some("x86_64-unknown-linux-gnu")).unwrap(),
      ReleasePlatform::LinuxX64
    );
  }

  #[test]
  fn release_platform_defines_runtime_installer_artifact_kinds() {
    assert_eq!(
      ReleasePlatform::WindowsX64.required_runtime_installer_artifact_kinds(),
      &[RuntimeInstallerArtifactKind::WindowsMsi]
    );
    assert_eq!(
      ReleasePlatform::LinuxX64.required_runtime_installer_artifact_kinds(),
      &[
        RuntimeInstallerArtifactKind::LinuxDeb,
        RuntimeInstallerArtifactKind::LinuxRpm
      ]
    );
    assert_eq!(
      ReleasePlatform::MacosArm64.required_runtime_installer_artifact_kinds(),
      &[RuntimeInstallerArtifactKind::MacosPkg]
    );
  }

  #[test]
  fn runtime_installer_artifact_names_use_canonical_release_platforms() {
    assert_eq!(
      runtime_installer_artifact_name(
        "1.2.3",
        ReleasePlatform::WindowsX64,
        RuntimeInstallerArtifactKind::WindowsMsi
      ),
      "cadder-runtime-1.2.3-windows-x64.msi"
    );
    assert_eq!(
      runtime_installer_artifact_name(
        "1.2.3",
        ReleasePlatform::LinuxX64,
        RuntimeInstallerArtifactKind::LinuxDeb
      ),
      "cadder-runtime-1.2.3-linux-x64.deb"
    );
  }
