#[derive(Debug, PartialEq, Eq)]
struct CoverageOptions {
  output_path: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
struct DistOptions {
  out_dir: PathBuf,
  target: Option<String>,
  topology: PortableTopology,
}

#[derive(Debug, PartialEq, Eq)]
struct RuntimeInstallerOptions {
  out_dir: PathBuf,
  version: String,
  target: Option<String>,
  platform: ReleasePlatform,
  signing_mode: SigningMode,
}

#[derive(Debug, PartialEq, Eq)]
struct VerifyDistOptions {
  dir: PathBuf,
  target: Option<String>,
  topology: PortableTopology,
}

#[derive(Debug, PartialEq, Eq)]
struct VerifyRuntimeInstallerDistOptions {
  dir: PathBuf,
  version: String,
  platform: ReleasePlatform,
}

#[derive(Debug, PartialEq, Eq)]
struct ReleaseAssetsOptions {
  dir: PathBuf,
  version: String,
  mode: ReleaseAssetMode,
}

#[derive(Debug, PartialEq, Eq)]
struct PackageOptions {
  out_dir: PathBuf,
  version: String,
  platform: String,
  target: Option<String>,
  topology: PortableTopology,
}

fn workspace_package_version() -> Result<String> {
  workspace_package_version_from_manifest(Path::new(WORKSPACE_MANIFEST))
}

fn workspace_package_version_from_manifest(manifest: &Path) -> Result<String> {
  let contents =
    fs::read_to_string(manifest).with_context(|| format!("read {}", manifest.display()))?;
  let mut in_workspace_package = false;

  for line in contents.lines() {
    let trimmed = line.split('#').next().unwrap_or_default().trim();
    if trimmed.is_empty() {
      continue;
    }

    if trimmed.starts_with('[') && trimmed.ends_with(']') {
      in_workspace_package = trimmed == "[workspace.package]";
      continue;
    }

    if in_workspace_package
      && let Some((key, value)) = trimmed.split_once('=')
      && key.trim() == "version"
    {
      let version = value.trim().trim_matches('"');
      if version.is_empty() {
        bail!(
          "workspace package version is empty in {}",
          manifest.display()
        );
      }
      return Ok(version.to_string());
    }
  }

  bail!(
    "workspace package version not found in {}",
    manifest.display()
  )
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ArchiveKind {
  Zip,
  TarGz,
}
