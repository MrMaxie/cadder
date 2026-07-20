
  fn write_fake_portable_layout(dir: &Path, binaries: &[&str], include_config: bool) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    for binary in binaries {
      fs::write(dir.join(binary), binary).with_context(|| format!("write {binary}"))?;
    }
    if include_config {
      fs::write(dir.join("cadder.toml"), SAMPLE_CADDER_TOML)
        .with_context(|| format!("write {}", dir.join("cadder.toml").display()))?;
    }
    Ok(())
  }

  fn write_complete_release_asset_matrix(dir: &Path, version: &str) {
    for platform in ReleasePlatform::ALL {
      write_portable_release_asset(dir, version, platform, PortableTopology::Runtime);
      write_runtime_installer_release_assets(dir, version, platform);
    }
  }

  fn write_portable_release_asset(
    dir: &Path,
    version: &str,
    platform: ReleasePlatform,
    topology: PortableTopology,
  ) {
    let archive_stem = topology
      .package_archive_stem(version, platform.name())
      .unwrap();
    let layout_parent = unique_temp_dir("release-archive-layout");
    let layout_dir = layout_parent.join(&archive_stem);
    let binaries = topology
      .binaries()
      .iter()
      .map(|binary| release_platform_exe_name(binary, platform))
      .collect::<Vec<_>>();
    let binary_refs = binaries.iter().map(String::as_str).collect::<Vec<_>>();
    write_fake_portable_layout(&layout_dir, &binary_refs, topology.includes_runtime()).unwrap();

    let archive = dir.join(format!(
      "{archive_stem}.{}",
      platform.portable_archive_extension()
    ));
    match platform {
      ReleasePlatform::WindowsX64 => {
        write_zip_archive(&layout_parent, &archive_stem, &archive).unwrap();
      }
      ReleasePlatform::LinuxX64 | ReleasePlatform::MacosX64 | ReleasePlatform::MacosArm64 => {
        write_tar_gz_archive(&layout_parent, &archive_stem, &archive).unwrap();
      }
    }
    write_sha256_file(&archive, &checksum_path_for(&archive).unwrap()).unwrap();
    fs::remove_dir_all(&layout_parent).unwrap();
  }

  fn write_runtime_installer_release_assets(dir: &Path, version: &str, platform: ReleasePlatform) {
    let options = RuntimeInstallerOptions {
      out_dir: dir.to_path_buf(),
      version: version.to_string(),
      target: None,
      platform,
      signing_mode: SigningMode::UnsignedDryRun,
    };
    for kind in platform.required_runtime_installer_artifact_kinds() {
      let artifact = dir.join(runtime_installer_artifact_name(version, platform, *kind));
      fs::write(&artifact, kind.label()).unwrap();
      write_sha256_file(&artifact, &checksum_path_for(&artifact).unwrap()).unwrap();
      write_runtime_installer_manifest(&artifact, &options).unwrap();
    }
  }

  fn write_release_asset(dir: &Path, file_name: &str) {
    let artifact = dir.join(file_name);
    fs::write(&artifact, file_name).unwrap();
    write_sha256_file(&artifact, &checksum_path_for(&artifact).unwrap()).unwrap();
  }

  fn write_fake_portable_executable_layout(dir: &Path, topology: PortableTopology) {
    let helper_dir = tempfile::tempdir().unwrap();
    let helper = build_fake_portable_tool(helper_dir.path());
    for binary in topology.binaries() {
      fs::copy(&helper, dir.join(exe_name(binary, None)))
        .with_context(|| format!("copy fake executable for {binary}"))
        .unwrap();
    }
    if topology.includes_runtime() {
      fs::write(dir.join("cadder.toml"), SAMPLE_CADDER_TOML)
        .with_context(|| format!("write {}", dir.join("cadder.toml").display()))
        .unwrap();
    }
  }

  fn build_fake_portable_tool(dir: &Path) -> PathBuf {
    let source = dir.join("fake_portable_tool.rs");
    let helper = dir.join(exe_name("fake-portable-tool", None));
    fs::write(
      &source,
      r#"
fn main() {
  let args = std::env::args().collect::<Vec<_>>();
  let exe_name = std::env::current_exe()
    .ok()
    .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_string()))
    .unwrap_or_default();
  if exe_name.starts_with("caddy")
    && args.get(1).map(String::as_str) == Some("--cadder-shim-info")
  {
    println!("{{\"role\":\"caddy-shim\"}}");
    return;
  }
  if args.get(1).map(String::as_str) == Some("--help") {
    println!("fake help");
    return;
  }
  if args.get(1).map(String::as_str) == Some("--version") {
    println!("fake version");
    return;
  }
  eprintln!("unexpected fake portable tool invocation: {args:?}");
  std::process::exit(2);
}
"#,
    )
    .unwrap();
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let mut command = Command::new(rustc);
    configure_hidden_child(&mut command);
    let status = command
      .arg(&source)
      .arg("-o")
      .arg(&helper)
      .status()
      .with_context(|| format!("compile {}", source.display()))
      .unwrap();
    assert!(status.success(), "failed to compile fake portable tool");
    helper
  }

  fn build_process_helper(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let source = dir.join("process_helper.rs");
    let helper = dir.join(exe_name("process-helper", None));
    fs::write(
      &source,
      r#"
use std::path::PathBuf;

fn main() {
  let args = std::env::args().collect::<Vec<_>>();
  match args.get(1).map(String::as_str) {
    Some("ok") => {}
    Some("fail") => std::process::exit(7),
    Some("cwd-env") => {
      let expected_cwd = args.get(2).map(PathBuf::from).expect("missing cwd");
      let expected_env = args.get(3).expect("missing env");
      if std::env::current_dir().ok().as_ref() != Some(&expected_cwd) {
        std::process::exit(8);
      }
      if std::env::var("CADDER_XTASK_PROCESS_HELPER").ok().as_ref() != Some(expected_env) {
        std::process::exit(9);
      }
    }
    other => {
      eprintln!("unexpected process helper argument: {other:?}");
      std::process::exit(10);
    }
  }
}
"#,
    )
    .unwrap();
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let mut command = Command::new(rustc);
    configure_hidden_child(&mut command);
    let status = command
      .arg(&source)
      .arg("-o")
      .arg(&helper)
      .status()
      .with_context(|| format!("compile {}", source.display()))
      .unwrap();
    assert!(status.success(), "failed to compile process helper");
    helper
  }

  fn assert_zip_entries(archive_path: &Path, expected: &[&str]) {
    let file = File::open(archive_path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entries = (0..archive.len())
      .map(|index| archive.by_index(index).unwrap().name().to_string())
      .collect::<Vec<_>>();
    entries.sort();

    let mut expected = expected
      .iter()
      .map(|entry| entry.to_string())
      .collect::<Vec<_>>();
    expected.sort();

    assert_eq!(entries, expected);
  }

  fn assert_tar_gz_entries(archive_path: &Path, expected: &[&str]) {
    let file = File::open(archive_path).unwrap();
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut entries = archive
      .entries()
      .unwrap()
      .map(|entry| {
        entry
          .unwrap()
          .path()
          .unwrap()
          .to_string_lossy()
          .replace('\\', "/")
      })
      .collect::<Vec<_>>();
    entries.sort();

    let mut expected = expected
      .iter()
      .map(|entry| entry.to_string())
      .collect::<Vec<_>>();
    expected.sort();

    assert_eq!(entries, expected);
  }

  fn unique_temp_dir(name: &str) -> PathBuf {
    env::temp_dir().join(format!(
      "cadder-xtask-{name}-{}-{}",
      std::process::id(),
      unique_suffix()
    ))
  }

  fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_nanos()
  }

  fn expected_release_profile_manifest() -> &'static str {
    r#"
[workspace]
members = []

[profile.release]
opt-level = "s"
lto = "thin"
codegen-units = 1
debug = false
strip = "symbols"
panic = "unwind"

[profile.profiling]
inherits = "release"
debug = true
strip = "none"
"#
  }

  fn write_workspace_topology_fixture(root: &Path) {
    fs::write(root.join("Cargo.toml"), workspace_topology_manifest(&[])).unwrap();
    for contract in WORKSPACE_MEMBER_CONTRACTS {
      write_workspace_member_manifest(root, contract, contract.package);
    }
  }

  fn write_workspace_member_manifest(
    root: &Path,
    contract: WorkspaceMemberContract,
    package_name: &str,
  ) {
    let crate_dir = root.join(contract.path);
    fs::create_dir_all(&crate_dir).unwrap();
    fs::write(
      crate_dir.join("Cargo.toml"),
      format!(
        r#"
[package]
name = {package_name:?}
version = "0.1.0"
edition = "2024"
"#
      ),
    )
    .unwrap();
  }

  fn workspace_topology_manifest(extra_members: &[&str]) -> String {
    let mut manifest = String::from(
      r#"
[workspace]
members = [
"#,
    );
    for contract in WORKSPACE_MEMBER_CONTRACTS {
      manifest.push_str(&format!("  {:?},\n", contract.path));
    }
    for member in extra_members {
      manifest.push_str(&format!("  {member:?},\n"));
    }
    manifest.push_str(
      r#"]
"#,
    );
    manifest
  }
