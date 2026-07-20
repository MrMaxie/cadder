  #[test]
  fn portable_layout_includes_expected_binaries() {
    assert_eq!(
      PortableTopology::Runtime.binaries(),
      &["cadderd", "cadder", "caddy"]
    );
  }

  #[test]
  fn portable_topology_archive_stem_uses_runtime_name() {
    assert_eq!(
      PortableTopology::Runtime
        .package_archive_stem("1.2.3", "windows-x64")
        .unwrap(),
      "cadder-1.2.3-windows-x64"
    );
  }


  #[test]
  fn release_binary_path_uses_release_directory_and_executable_name() {
    assert_eq!(
      release_binary_path("cadderd", None),
      PathBuf::from("target")
        .join("release")
        .join(exe_name("cadderd", None))
    );
  }

  #[test]
  fn release_binary_path_uses_target_release_directory() {
    assert_eq!(
      release_binary_path("cadderd", Some("x86_64-unknown-linux-gnu")),
      PathBuf::from("target")
        .join("x86_64-unknown-linux-gnu")
        .join("release")
        .join("cadderd")
    );
  }

  #[test]
  fn exe_name_uses_windows_suffix_for_windows_target() {
    assert_eq!(
      exe_name("cadderd", Some("x86_64-pc-windows-msvc")),
      "cadderd.exe"
    );
  }

  #[test]
  fn archive_extension_uses_zip_for_windows() {
    assert_eq!(archive_extension("windows-x64"), "zip");
  }

  #[test]
  fn archive_extension_uses_tar_gz_for_unix_platforms() {
    assert_eq!(archive_extension("linux-x64"), "tar.gz");
    assert_eq!(archive_extension("macos-arm64"), "tar.gz");
  }

  #[test]
  fn verify_dist_rejects_missing_portable_files() {
    let dir = unique_temp_dir("missing-portable-files");
    fs::create_dir_all(&dir).unwrap();

    let error = verify_dist(&VerifyDistOptions {
      dir: dir.clone(),
      target: None,
      topology: PortableTopology::Runtime,
    })
    .unwrap_err();

    assert!(error.to_string().contains("portable binary missing"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_dist_rejects_missing_sample_config_after_binaries_exist() {
    let dir = unique_temp_dir("missing-sample-config");
    fs::create_dir_all(&dir).unwrap();
    for binary in PortableTopology::Runtime.binaries() {
      fs::write(dir.join(exe_name(binary, None)), b"not executable").unwrap();
    }

    let error = verify_dist(&VerifyDistOptions {
      dir: dir.clone(),
      target: None,
      topology: PortableTopology::Runtime,
    })
    .unwrap_err();

    assert!(
      error
        .to_string()
        .contains("portable sample configuration missing")
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_dist_accepts_fake_portable_layout_and_binary_contract() {
    let dir = unique_temp_dir("portable-layout");
    fs::create_dir_all(&dir).unwrap();
    write_fake_portable_executable_layout(&dir, PortableTopology::Runtime);

    verify_dist(&VerifyDistOptions {
      dir: dir.clone(),
      target: None,
      topology: PortableTopology::Runtime,
    })
    .unwrap();

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_dist_rejects_extra_portable_files() {
    let dir = unique_temp_dir("portable-extra-files");
    fs::create_dir_all(&dir).unwrap();
    write_fake_portable_executable_layout(&dir, PortableTopology::Runtime);
    fs::write(dir.join("extra.txt"), b"extra").unwrap();

    let error = verify_dist(&VerifyDistOptions {
      dir: dir.clone(),
      target: None,
      topology: PortableTopology::Runtime,
    })
    .unwrap_err();

    assert!(error.to_string().contains("contains unexpected file set"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn dist_with_builder_copies_target_release_binaries_and_sample_config() {
    let unique = unique_suffix();
    let target = format!("cadder-test-windows-{unique}");
    let release_dir = PathBuf::from("target").join(&target).join("release");
    let out_dir = unique_temp_dir("dist-layout");
    fs::create_dir_all(&release_dir).unwrap();
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(out_dir.join("cadder.exe"), b"stale operator").unwrap();

    let result = dist_with_builder(
      DistOptions {
        out_dir: out_dir.clone(),
        target: Some(target.clone()),
        topology: PortableTopology::Runtime,
      },
      |requested_target, topology| {
        assert_eq!(requested_target, Some(target.as_str()));
        assert_eq!(topology, PortableTopology::Runtime);
        write_fake_portable_executable_layout(&release_dir, topology);
        Ok(())
      },
    );

    result.unwrap();
    for binary in PortableTopology::Runtime.binaries() {
      assert!(out_dir.join(format!("{binary}.exe")).is_file());
    }
    assert_ne!(
      fs::read(out_dir.join("cadder.exe")).unwrap(),
      b"stale operator"
    );
    assert_eq!(
      fs::read_to_string(out_dir.join("cadder.toml")).unwrap(),
      SAMPLE_CADDER_TOML
    );

    fs::remove_dir_all(&out_dir).unwrap();
    fs::remove_dir_all(PathBuf::from("target").join(&target)).unwrap();
  }

  #[test]
  fn replace_dir_from_recursively_overwrites_existing_target_tree() {
    let dir = unique_temp_dir("replace-tree");
    let source = dir.join("source");
    let target = dir.join("target");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::create_dir_all(target.join("stale")).unwrap();
    fs::write(source.join("root.txt"), "root").unwrap();
    fs::write(source.join("nested").join("child.txt"), "child").unwrap();
    fs::write(target.join("stale").join("old.txt"), "old").unwrap();

    replace_dir_from(&source, &target).unwrap();

    assert_eq!(fs::read_to_string(target.join("root.txt")).unwrap(), "root");
    assert_eq!(
      fs::read_to_string(target.join("nested").join("child.txt")).unwrap(),
      "child"
    );
    assert!(!target.join("stale").exists());
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn write_sha256_file_writes_hash_and_file_name() {
    let dir = unique_temp_dir("checksum");
    fs::create_dir_all(&dir).unwrap();
    let archive_path = dir.join("artifact.tar.gz");
    let checksum_path = dir.join("artifact.tar.gz.sha256");
    fs::write(&archive_path, b"artifact").unwrap();

    write_sha256_file(&archive_path, &checksum_path).unwrap();

    let checksum = fs::read_to_string(&checksum_path).unwrap();
    assert!(checksum.ends_with("  artifact.tar.gz\n"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn artifact_summary_helpers_collect_file_paths_and_directory_sizes() {
    let dir = unique_temp_dir("artifact-summary");
    let nested = dir.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(dir.join("root.bin"), [1, 2, 3]).unwrap();
    fs::write(nested.join("child.bin"), [4, 5]).unwrap();

    let mut paths = Vec::new();
    collect_file_paths(&dir, &mut paths).unwrap();

    assert_eq!(
      paths,
      vec![dir.join("nested").join("child.bin"), dir.join("root.bin")]
    );
    assert_eq!(path_size(&nested).unwrap(), 2);
    assert_eq!(
      relative_display_path(&dir, &nested.join("child.bin")),
      PathBuf::from("nested")
        .join("child.bin")
        .display()
        .to_string()
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn run_helpers_report_process_success_failure_cwd_and_env() {
    let dir = unique_temp_dir("process-helper");
    let cwd = dir.join("work");
    let env_value = dir.join("env-value");
    fs::create_dir_all(&cwd).unwrap();
    let helper = build_process_helper(&dir);
    let helper = helper.to_str().unwrap();
    let cwd_arg = cwd.to_str().unwrap();
    let env_arg = env_value.to_str().unwrap();

    run(helper, &["ok"]).unwrap();
    run_in(helper, &["ok"], &cwd).unwrap();
    run_in_with_env(
      helper,
      &["cwd-env", cwd_arg, env_arg],
      &cwd,
      &[("CADDER_XTASK_PROCESS_HELPER", &env_value)],
    )
    .unwrap();

    let run_error = run(helper, &["fail"]).unwrap_err();
    assert!(run_error.to_string().contains("failed with"));
    let run_in_error = run_in(helper, &["fail"], &cwd).unwrap_err();
    assert!(run_in_error.to_string().contains("failed with"));

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn package_with_dist_writes_windows_zip_and_checksum() {
    let out_dir = unique_temp_dir("windows-package");
    let expected_layout = out_dir.join("layouts").join("cadder-1.2.3-windows-x64");

    package_with_dist(
      PackageOptions {
        out_dir: out_dir.clone(),
        version: "1.2.3".to_string(),
        platform: "windows-x64".to_string(),
        target: Some("x86_64-pc-windows-msvc".to_string()),
        topology: PortableTopology::Runtime,
      },
      |dist_options| {
        assert_eq!(dist_options.out_dir, expected_layout);
        assert_eq!(
          dist_options.target,
          Some("x86_64-pc-windows-msvc".to_string())
        );
        assert_eq!(dist_options.topology, PortableTopology::Runtime);
        write_fake_portable_layout(
          &dist_options.out_dir,
          &["cadderd.exe", "cadder.exe", "caddy.exe"],
          true,
        )
      },
    )
    .unwrap();

    let archive_path = out_dir.join("cadder-1.2.3-windows-x64.zip");
    let checksum_path = out_dir.join("cadder-1.2.3-windows-x64.zip.sha256");
    assert!(archive_path.is_file());
    assert!(checksum_path.is_file());
    assert_zip_entries(
      &archive_path,
      &[
        "cadder-1.2.3-windows-x64/",
        "cadder-1.2.3-windows-x64/cadder.exe",
        "cadder-1.2.3-windows-x64/cadder.toml",
        "cadder-1.2.3-windows-x64/cadderd.exe",
        "cadder-1.2.3-windows-x64/caddy.exe",
      ],
    );
    assert!(
      fs::read_to_string(&checksum_path)
        .unwrap()
        .ends_with("  cadder-1.2.3-windows-x64.zip\n")
    );

    fs::remove_dir_all(&out_dir).unwrap();
  }

  #[test]
  fn package_with_dist_writes_unix_tar_gz_and_checksum() {
    let out_dir = unique_temp_dir("linux-package");
    let expected_layout = out_dir.join("layouts").join("cadder-1.2.3-linux-x64");

    package_with_dist(
      PackageOptions {
        out_dir: out_dir.clone(),
        version: "1.2.3".to_string(),
        platform: "linux-x64".to_string(),
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        topology: PortableTopology::Runtime,
      },
      |dist_options| {
        assert_eq!(dist_options.out_dir, expected_layout);
        assert_eq!(
          dist_options.target,
          Some("x86_64-unknown-linux-gnu".to_string())
        );
        assert_eq!(dist_options.topology, PortableTopology::Runtime);
        write_fake_portable_layout(&dist_options.out_dir, &["cadderd", "cadder", "caddy"], true)
      },
    )
    .unwrap();

    let archive_path = out_dir.join("cadder-1.2.3-linux-x64.tar.gz");
    let checksum_path = out_dir.join("cadder-1.2.3-linux-x64.tar.gz.sha256");
    assert!(archive_path.is_file());
    assert!(checksum_path.is_file());
    assert_tar_gz_entries(
      &archive_path,
      &[
        "cadder-1.2.3-linux-x64/",
        "cadder-1.2.3-linux-x64/cadder",
        "cadder-1.2.3-linux-x64/cadder.toml",
        "cadder-1.2.3-linux-x64/cadderd",
        "cadder-1.2.3-linux-x64/caddy",
      ],
    );
    assert!(
      fs::read_to_string(&checksum_path)
        .unwrap()
        .ends_with("  cadder-1.2.3-linux-x64.tar.gz\n")
    );

    fs::remove_dir_all(&out_dir).unwrap();
  }
