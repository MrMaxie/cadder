fn verify_runtime_installer_dist(options: &VerifyRuntimeInstallerDistOptions) -> Result<()> {
  verify_runtime_installer_release_assets_for_platform(
    &options.dir,
    &options.version,
    options.platform,
  )?;
  print_selected_artifact_summary(
    "runtime installer",
    &options.dir,
    &runtime_installer_dist_artifacts(&options.dir, &options.version, options.platform)?,
  )
}

fn runtime_installer_dist_artifacts(
  dir: &Path,
  version: &str,
  platform: ReleasePlatform,
) -> Result<Vec<PathBuf>> {
  let mut artifacts = Vec::new();
  for kind in platform.required_runtime_installer_artifact_kinds() {
    let artifact = dir.join(runtime_installer_artifact_name(version, platform, *kind));
    artifacts.push(artifact.clone());
    artifacts.push(checksum_path_for(&artifact)?);
    let manifest = runtime_installer_manifest_path_for(&artifact)?;
    artifacts.push(manifest.clone());
    artifacts.push(checksum_path_for(&manifest)?);
  }
  Ok(artifacts)
}

fn runtime_installer_artifact_name(
  version: &str,
  platform: ReleasePlatform,
  kind: RuntimeInstallerArtifactKind,
) -> String {
  format!(
    "{package}-{version}-{platform}.{extension}",
    package = RUNTIME_INSTALLER_PACKAGE_NAME,
    platform = platform.name(),
    extension = kind.extension(),
  )
}

fn runtime_installer_manifest_path_for(artifact_path: &Path) -> Result<PathBuf> {
  let file_name = artifact_path
    .file_name()
    .and_then(|name| name.to_str())
    .context("runtime installer artifact path has no UTF-8 file name")?;
  Ok(artifact_path.with_file_name(format!("{file_name}.manifest.json")))
}

fn write_runtime_installer_manifest(
  artifact: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<()> {
  let file_name = artifact
    .file_name()
    .and_then(|name| name.to_str())
    .context("runtime installer artifact path has no UTF-8 file name")?;
  let kind = runtime_installer_artifact_kind(options.platform, artifact)
    .context("runtime installer artifact kind is not recognized")?;
  let manifest_path = runtime_installer_manifest_path_for(artifact)?;
  let manifest = json!({
    "artifact": file_name,
    "component": "daemon-runtime",
    "platform": options.platform.name(),
    "packageKind": kind.label(),
    "version": options.version,
    "installPaths": runtime_installer_expected_install_paths(options.platform),
  });
  fs::write(
    &manifest_path,
    serde_json::to_string_pretty(&manifest).context("serialize runtime installer manifest")?,
  )
  .with_context(|| format!("write {}", manifest_path.display()))?;
  let checksum_path = checksum_path_for(&manifest_path)?;
  write_sha256_file(&manifest_path, &checksum_path)?;
  Ok(())
}
