fn verify_release_assets(options: &ReleaseAssetsOptions) -> Result<()> {
  verify_release_identity()?;
  if options.mode == ReleaseAssetMode::Publish {
    verify_release_signing_inputs()?;
  }

  reject_raw_app_bundles(&options.dir)?;
  for platform in ReleasePlatform::ALL {
    verify_portable_release_asset(
      &options.dir,
      &options.version,
      platform,
      PortableTopology::Runtime,
    )?;
    verify_runtime_installer_release_assets_for_platform(&options.dir, &options.version, platform)?;
  }

  reject_unmatched_runtime_installer_release_assets(&options.dir, &options.version)?;
  print_artifact_summary("combined release assets", &options.dir)
}

fn verify_release_signing_inputs() -> Result<()> {
  for variable in SIGNING_READY_ENV_VARS {
    if env::var(variable).ok().as_deref() != Some("true") {
      bail!(
        "publish release requires {variable}=true after platform signing/notarization inputs are configured"
      );
    }
  }
  Ok(())
}

fn verify_portable_release_asset(
  dir: &Path,
  version: &str,
  platform: ReleasePlatform,
  topology: PortableTopology,
) -> Result<()> {
  let archive_stem = topology.package_archive_stem(version, platform.name())?;
  let artifact = dir.join(format!(
    "{archive_stem}.{}",
    platform.portable_archive_extension()
  ));
  if !artifact.is_file() {
    bail!("release asset missing: {}", artifact.display());
  }
  verify_sha256_file(&artifact)?;
  verify_portable_archive_contents(&artifact, &archive_stem, topology, platform)
}

fn verify_portable_archive_contents(
  artifact: &Path,
  archive_stem: &str,
  topology: PortableTopology,
  platform: ReleasePlatform,
) -> Result<()> {
  let entries = portable_archive_entries(artifact, platform)?;
  for binary in topology.binaries() {
    let expected = format!(
      "{archive_stem}/{}",
      release_platform_exe_name(binary, platform)
    );
    if !entries.iter().any(|entry| entry == &expected) {
      bail!(
        "portable archive {} is missing expected entry {expected}",
        artifact.display()
      );
    }
  }
  let config_entry = format!("{archive_stem}/cadder.toml");
  let has_config = entries.iter().any(|entry| entry == &config_entry);
  if topology.includes_runtime() && !has_config {
    bail!(
      "portable archive {} is missing expected entry {config_entry}",
      artifact.display()
    );
  }
  verify_portable_archive_file_set(artifact, archive_stem, topology, platform, entries)?;

  Ok(())
}

fn verify_portable_archive_file_set(
  artifact: &Path,
  archive_stem: &str,
  topology: PortableTopology,
  platform: ReleasePlatform,
  entries: Vec<String>,
) -> Result<()> {
  let mut expected = BTreeSet::new();
  for binary in topology.binaries() {
    expected.insert(format!(
      "{archive_stem}/{}",
      release_platform_exe_name(binary, platform)
    ));
  }
  if topology.includes_runtime() {
    expected.insert(format!("{archive_stem}/cadder.toml"));
  }

  let actual = entries
    .into_iter()
    .filter(|entry| !entry.ends_with('/'))
    .collect::<BTreeSet<_>>();
  if actual != expected {
    bail!(
      "portable archive {} contains unexpected file set: expected {:?}, found {:?}",
      artifact.display(),
      expected,
      actual
    );
  }

  Ok(())
}

fn portable_archive_entries(artifact: &Path, platform: ReleasePlatform) -> Result<Vec<String>> {
  if platform == ReleasePlatform::WindowsX64 {
    return zip_archive_entries(artifact);
  }
  tar_gz_archive_entries(artifact)
}

fn zip_archive_entries(artifact: &Path) -> Result<Vec<String>> {
  let file = File::open(artifact).with_context(|| format!("open {}", artifact.display()))?;
  let mut archive =
    zip::ZipArchive::new(file).with_context(|| format!("read ZIP {}", artifact.display()))?;
  let mut entries = Vec::new();
  for index in 0..archive.len() {
    let file = archive
      .by_index(index)
      .with_context(|| format!("read ZIP entry {index} from {}", artifact.display()))?;
    if file.is_file() {
      entries.push(file.name().trim_end_matches('/').to_string());
    }
  }
  entries.sort();
  Ok(entries)
}

fn tar_gz_archive_entries(artifact: &Path) -> Result<Vec<String>> {
  let file = File::open(artifact).with_context(|| format!("open {}", artifact.display()))?;
  let decoder = GzDecoder::new(file);
  let mut archive = tar::Archive::new(decoder);
  let mut entries = Vec::new();
  for entry in archive
    .entries()
    .with_context(|| format!("read TAR {}", artifact.display()))?
  {
    let entry = entry.with_context(|| format!("read TAR entry from {}", artifact.display()))?;
    if entry.header().entry_type().is_file() {
      let path = entry.path()?;
      entries.push(path_to_archive_name(path.as_ref())?);
    }
  }
  entries.sort();
  Ok(entries)
}

fn release_platform_exe_name(name: &str, platform: ReleasePlatform) -> String {
  if platform.uses_windows_executables() {
    format!("{name}.exe")
  } else {
    name.to_string()
  }
}

fn verify_runtime_installer_release_assets_for_platform(
  dir: &Path,
  version: &str,
  platform: ReleasePlatform,
) -> Result<()> {
  for kind in platform.required_runtime_installer_artifact_kinds() {
    let artifact = dir.join(runtime_installer_artifact_name(version, platform, *kind));
    if !artifact.is_file() {
      bail!(
        "runtime installer release asset missing: {}",
        artifact.display()
      );
    }
    verify_sha256_file(&artifact)?;
    verify_runtime_installer_manifest(&artifact, version, platform, *kind)?;
  }
  Ok(())
}

fn verify_runtime_installer_manifest(
  artifact: &Path,
  version: &str,
  platform: ReleasePlatform,
  kind: RuntimeInstallerArtifactKind,
) -> Result<()> {
  let manifest_path = runtime_installer_manifest_path_for(artifact)?;
  if !manifest_path.is_file() {
    bail!(
      "runtime installer manifest missing: {}",
      manifest_path.display()
    );
  }
  verify_sha256_file(&manifest_path)?;
  let manifest = read_json_value(&manifest_path)?;
  let artifact_name = artifact
    .file_name()
    .and_then(|name| name.to_str())
    .context("runtime installer artifact path has no UTF-8 file name")?;
  ensure_json_string(
    &manifest_path,
    &manifest,
    &["artifact"],
    artifact_name,
    "runtime installer artifact",
  )?;
  ensure_json_string(
    &manifest_path,
    &manifest,
    &["component"],
    "daemon-runtime",
    "runtime installer component",
  )?;
  ensure_json_string(
    &manifest_path,
    &manifest,
    &["platform"],
    platform.name(),
    "runtime installer platform",
  )?;
  ensure_json_string(
    &manifest_path,
    &manifest,
    &["packageKind"],
    kind.label(),
    "runtime installer package kind",
  )?;
  ensure_json_string(
    &manifest_path,
    &manifest,
    &["version"],
    version,
    "runtime installer version",
  )?;
  let install_paths = json_path_array(
    &manifest_path,
    &manifest,
    &["installPaths"],
    "runtime installer install paths",
  )?;
  let expected_paths = runtime_installer_expected_install_paths(platform);
  for expected in &expected_paths {
    if !install_paths
      .iter()
      .any(|value| value.as_str() == Some(expected.as_str()))
    {
      bail!(
        "runtime installer manifest {} is missing install path {expected}",
        manifest_path.display()
      );
    }
  }
  for value in install_paths {
    let Some(actual) = value.as_str() else {
      bail!(
        "runtime installer manifest {} contains non-string install path {}",
        manifest_path.display(),
        value
      );
    };
    if !expected_paths.iter().any(|expected| expected == actual) {
      bail!(
        "runtime installer manifest {} contains unexpected install path {actual}",
        manifest_path.display()
      );
    }
  }
  if install_paths.len() != expected_paths.len() {
    bail!(
      "runtime installer manifest {} expected {} install paths, found {}",
      manifest_path.display(),
      expected_paths.len(),
      install_paths.len()
    );
  }
  Ok(())
}

fn json_path_array<'a>(
  path: &Path,
  root: &'a JsonValue,
  keys: &[&str],
  label: &str,
) -> Result<&'a Vec<JsonValue>> {
  json_path_value(path, root, keys, label)?
    .as_array()
    .with_context(|| format!("{label} is not an array in {}", path.display()))
}

fn json_path_value<'a>(
  path: &Path,
  root: &'a JsonValue,
  keys: &[&str],
  label: &str,
) -> Result<&'a JsonValue> {
  let mut value = root;
  for key in keys {
    value = value.get(*key).with_context(|| {
      format!(
        "{label} missing key `{}` in {}",
        keys.join("."),
        path.display()
      )
    })?;
  }
  Ok(value)
}

fn runtime_installer_expected_install_paths(platform: ReleasePlatform) -> Vec<String> {
  match platform {
    ReleasePlatform::WindowsX64 => RUNTIME_PORTABLE_BINARIES
      .iter()
      .map(|binary| format!(r"C:\Program Files\Cadder\{}.exe", binary))
      .chain(std::iter::once(
        r"C:\Program Files\Cadder\cadder.toml".to_string(),
      ))
      .collect(),
    ReleasePlatform::LinuxX64 | ReleasePlatform::MacosX64 | ReleasePlatform::MacosArm64 => {
      RUNTIME_PORTABLE_BINARIES
        .iter()
        .map(|binary| format!("/usr/local/bin/{binary}"))
        .chain(std::iter::once(
          "/usr/local/etc/cadder/cadder.toml".to_string(),
        ))
        .collect()
    }
  }
}

fn runtime_installer_artifact_kind(
  platform: ReleasePlatform,
  path: &Path,
) -> Option<RuntimeInstallerArtifactKind> {
  let name = path.file_name()?.to_str()?.to_ascii_lowercase();
  match platform {
    ReleasePlatform::WindowsX64 if name.ends_with(".msi") => {
      Some(RuntimeInstallerArtifactKind::WindowsMsi)
    }
    ReleasePlatform::LinuxX64 if name.ends_with(".deb") => {
      Some(RuntimeInstallerArtifactKind::LinuxDeb)
    }
    ReleasePlatform::LinuxX64 if name.ends_with(".rpm") => {
      Some(RuntimeInstallerArtifactKind::LinuxRpm)
    }
    ReleasePlatform::MacosX64 | ReleasePlatform::MacosArm64 if name.ends_with(".pkg") => {
      Some(RuntimeInstallerArtifactKind::MacosPkg)
    }
    _ => None,
  }
}

fn reject_unmatched_runtime_installer_release_assets(dir: &Path, version: &str) -> Result<()> {
  let mut expected_names = BTreeSet::new();
  for platform in ReleasePlatform::ALL {
    for kind in platform.required_runtime_installer_artifact_kinds() {
      let artifact_name = runtime_installer_artifact_name(version, platform, *kind);
      expected_names.insert(artifact_name.clone());
      expected_names.insert(format!("{artifact_name}.sha256"));
      let manifest_name = format!("{artifact_name}.manifest.json");
      expected_names.insert(manifest_name.clone());
      expected_names.insert(format!("{manifest_name}.sha256"));
    }
  }

  let mut paths = Vec::new();
  collect_file_paths(dir, &mut paths)?;
  for path in paths {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
      continue;
    };
    if file_name.starts_with("cadder-runtime-") && !expected_names.contains(file_name) {
      bail!(
        "unexpected runtime installer release asset: {}",
        path.display()
      );
    }
  }
  Ok(())
}

fn reject_raw_app_bundles(dir: &Path) -> Result<()> {
  for entry in sorted_dir_entries(dir)? {
    let path = entry.path();
    if path.is_dir() {
      if is_macos_app_bundle(&path) {
        bail!(
          "raw macOS .app bundle is not a publishable release asset: {}",
          path.display()
        );
      }
      reject_raw_app_bundles(&path)?;
    }
  }
  Ok(())
}
