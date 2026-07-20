fn verify_release_profile() -> Result<()> {
  verify_release_profile_manifest(Path::new(WORKSPACE_MANIFEST))?;
  println!("verified release profile policy");
  Ok(())
}

fn verify_release_identity() -> Result<()> {
  verify_assets()?;
  verify_release_identity_files(
    Path::new(WORKSPACE_MANIFEST),
    &workspace_root().join(DOCS_DOWNLOAD_SCRIPT),
  )?;
  println!("verified release identity metadata");
  Ok(())
}

fn verify_assets() -> Result<()> {
  verify_assets_at(&workspace_root())?;
  println!("verified visual asset contract");
  Ok(())
}

fn verify_workspace_topology() -> Result<()> {
  verify_workspace_topology_manifest(Path::new(WORKSPACE_MANIFEST))?;
  println!("verified workspace topology contract");
  Ok(())
}

fn verify_assets_at(root: &Path) -> Result<()> {
  verify_required_asset_paths(root, &REQUIRED_VISUAL_ASSETS)?;
  verify_canonical_asset_copies(root, &CANONICAL_ASSET_COPIES)?;
  verify_obsolete_assets_absent(root, &OBSOLETE_SCAFFOLD_ASSETS)?;
  verify_docs_webmanifest(root, &root.join(DOCS_WEBMANIFEST))
}

fn verify_release_identity_files(workspace_manifest: &Path, download_script: &Path) -> Result<()> {
  workspace_package_version_from_manifest(workspace_manifest)?;
  verify_docs_download_metadata(download_script)
}

fn verify_docs_download_metadata(download_script: &Path) -> Result<()> {
  let contents = fs::read_to_string(download_script)
    .with_context(|| format!("read {}", download_script.display()))?;
  for (label, snippet) in REQUIRED_DOWNLOAD_METADATA_SNIPPETS {
    if !contents.contains(snippet) {
      bail!(
        "download metadata for {label} is missing expected snippet {snippet:?} in {}",
        download_script.display()
      );
    }
  }
  Ok(())
}

fn verify_required_asset_paths(root: &Path, assets: &[(&str, &str)]) -> Result<()> {
  for (label, relative_path) in assets {
    let path = root.join(relative_path);
    if !path.is_file() {
      bail!(
        "{label} is missing at {}",
        normalize_repo_path(relative_path)
      );
    }
  }
  Ok(())
}

fn verify_canonical_asset_copies(root: &Path, copies: &[(&str, &str, &str)]) -> Result<()> {
  for (label, canonical_path, copy_path) in copies {
    let canonical = root.join(canonical_path);
    let copy = root.join(copy_path);
    let canonical_hash = compute_sha256(&canonical)?;
    let copy_hash = compute_sha256(&copy)?;
    if canonical_hash != copy_hash {
      bail!(
        "{label} at {} differs from canonical asset {}",
        normalize_repo_path(copy_path),
        normalize_repo_path(canonical_path)
      );
    }
  }
  Ok(())
}

fn verify_obsolete_assets_absent(root: &Path, assets: &[(&str, &str)]) -> Result<()> {
  for (label, relative_path) in assets {
    if root.join(relative_path).exists() {
      bail!(
        "obsolete scaffold asset {label} must not be checked in at {}",
        normalize_repo_path(relative_path)
      );
    }
  }
  Ok(())
}

fn verify_docs_webmanifest(root: &Path, manifest_path: &Path) -> Result<()> {
  let manifest = read_json_value(manifest_path)?;
  ensure_json_string(
    manifest_path,
    &manifest,
    &["name"],
    "Cadder",
    "web manifest name",
  )?;
  ensure_json_string(
    manifest_path,
    &manifest,
    &["short_name"],
    "Cadder",
    "web manifest short_name",
  )?;

  let icons = manifest
    .get("icons")
    .and_then(JsonValue::as_array)
    .with_context(|| format!("icons missing in {}", manifest_path.display()))?;
  if icons.is_empty() {
    bail!("icons is empty in {}", manifest_path.display());
  }

  let public_root = root.join("docs/site/public");
  for icon in icons {
    let src = json_path_string(manifest_path, icon, &["src"], "web manifest icon src")?;
    let relative_path = src.strip_prefix('/').unwrap_or(src);
    let path = public_root.join(relative_path);
    if !path.is_file() {
      bail!(
        "web manifest icon {} is missing at {}",
        src,
        normalize_repo_path(&path_relative_to(root, &path))
      );
    }
  }
  Ok(())
}

fn normalize_repo_path(path: &str) -> String {
  path.replace('\\', "/")
}

fn path_relative_to(root: &Path, path: &Path) -> String {
  path
    .strip_prefix(root)
    .unwrap_or(path)
    .display()
    .to_string()
}

fn read_json_value(path: &Path) -> Result<JsonValue> {
  let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
  serde_json::from_str(&contents).with_context(|| format!("parse JSON {}", path.display()))
}

fn ensure_json_string(
  path: &Path,
  root: &JsonValue,
  keys: &[&str],
  expected: &str,
  label: &str,
) -> Result<()> {
  let value = json_path_string(path, root, keys, label)?;
  if value == expected {
    return Ok(());
  }

  bail!(
    "{label} expected {expected:?}, found {value:?} in {}",
    path.display()
  )
}

fn json_path_string<'a>(
  path: &Path,
  root: &'a JsonValue,
  keys: &[&str],
  label: &str,
) -> Result<&'a str> {
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
  value
    .as_str()
    .with_context(|| format!("{label} is not a string in {}", path.display()))
}
