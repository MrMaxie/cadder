fn verify_release_profile_manifest(manifest: &Path) -> Result<()> {
  let manifest_table = read_toml_table(manifest)?;
  for setting in RELEASE_PROFILE_SETTINGS
    .iter()
    .chain(PROFILING_PROFILE_SETTINGS.iter())
  {
    ensure_profile_setting(manifest, &manifest_table, setting)?;
  }
  Ok(())
}

fn verify_workspace_topology_manifest(manifest: &Path) -> Result<()> {
  let root = manifest
    .parent()
    .with_context(|| format!("workspace manifest has no parent: {}", manifest.display()))?;
  let manifest_table = read_toml_table(manifest)?;
  let actual_members = workspace_members_from_manifest(manifest, &manifest_table)?;

  ensure_workspace_members_match(manifest, &actual_members)?;
  ensure_workspace_member_manifests_match(root)?;
  ensure_runtime_release_packages_match_contract()?;
  Ok(())
}

fn workspace_members_from_manifest(manifest: &Path, root: &Table) -> Result<BTreeSet<String>> {
  let workspace = root
    .get("workspace")
    .and_then(Value::as_table)
    .with_context(|| format!("workspace table missing in {}", manifest.display()))?;
  let members = workspace
    .get("members")
    .and_then(Value::as_array)
    .with_context(|| format!("workspace.members array missing in {}", manifest.display()))?;

  let mut result = BTreeSet::new();
  for member in members {
    let member = member
      .as_str()
      .with_context(|| format!("workspace member is not a string in {}", manifest.display()))?;
    let normalized = normalize_repo_path(member);
    if !result.insert(normalized.clone()) {
      bail!(
        "duplicate workspace member {normalized:?} in {}",
        manifest.display()
      );
    }
  }
  Ok(result)
}

fn ensure_workspace_members_match(manifest: &Path, actual: &BTreeSet<String>) -> Result<()> {
  let expected = expected_workspace_members();
  if actual == &expected {
    return Ok(());
  }

  bail!(
    "workspace members in {} do not match documented topology; missing: {}; unexpected: {}; documented topology: {}",
    manifest.display(),
    format_string_set(expected.difference(actual)),
    format_string_set(actual.difference(&expected)),
    documented_workspace_topology()
  )
}

fn ensure_workspace_member_manifests_match(root: &Path) -> Result<()> {
  for contract in WORKSPACE_MEMBER_CONTRACTS {
    let manifest = root.join(contract.path).join("Cargo.toml");
    let package_name = workspace_member_package_name(&manifest)?;
    if package_name != contract.package {
      bail!(
        "{} is classified as {} but package name must be {:?}, found {:?}",
        contract.path,
        contract.classification.label(),
        contract.package,
        package_name
      );
    }
  }
  Ok(())
}

fn workspace_member_package_name(manifest: &Path) -> Result<String> {
  let manifest_table = read_toml_table(manifest)?;
  manifest_table
    .get("package")
    .and_then(Value::as_table)
    .and_then(|package| package.get("name"))
    .and_then(Value::as_str)
    .map(ToOwned::to_owned)
    .with_context(|| format!("package.name missing in {}", manifest.display()))
}

fn ensure_runtime_release_packages_match_contract() -> Result<()> {
  let actual = RUNTIME_RELEASE_PACKAGES
    .iter()
    .map(|package| (*package).to_string())
    .collect::<BTreeSet<_>>();
  let expected = WORKSPACE_MEMBER_CONTRACTS
    .iter()
    .filter(|contract| contract.runtime_release_package)
    .map(|contract| contract.package.to_string())
    .collect::<BTreeSet<_>>();

  if actual == expected {
    return Ok(());
  }

  bail!(
    "runtime release packages do not match documented topology; missing: {}; unexpected: {}",
    format_string_set(expected.difference(&actual)),
    format_string_set(actual.difference(&expected))
  )
}

fn expected_workspace_members() -> BTreeSet<String> {
  WORKSPACE_MEMBER_CONTRACTS
    .iter()
    .map(|contract| contract.path.to_string())
    .collect()
}

fn documented_workspace_topology() -> String {
  WORKSPACE_MEMBER_CONTRACTS
    .iter()
    .map(|contract| {
      format!(
        "{}={} ({})",
        contract.path,
        contract.package,
        contract.classification.label()
      )
    })
    .collect::<Vec<_>>()
    .join(", ")
}

fn format_string_set<'a>(items: impl Iterator<Item = &'a String>) -> String {
  let items = items.map(String::as_str).collect::<Vec<_>>();
  if items.is_empty() {
    "none".to_string()
  } else {
    items.join(", ")
  }
}

fn read_toml_table(path: &Path) -> Result<Table> {
  let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
  contents
    .parse::<Table>()
    .with_context(|| format!("parse TOML {}", path.display()))
}

fn ensure_profile_setting(manifest: &Path, root: &Table, setting: &ProfileSetting) -> Result<()> {
  let profile = root
    .get("profile")
    .and_then(Value::as_table)
    .with_context(|| format!("profile table missing in {}", manifest.display()))?;
  let profile_table = profile
    .get(setting.profile)
    .and_then(Value::as_table)
    .with_context(|| {
      format!(
        "profile.{} table missing in {}",
        setting.profile,
        manifest.display()
      )
    })?;
  let value = profile_table.get(setting.key).with_context(|| {
    format!(
      "profile.{}.{} missing in {}",
      setting.profile,
      setting.key,
      manifest.display()
    )
  })?;

  if setting.expected.matches(value) {
    return Ok(());
  }

  bail!(
    "profile.{}.{} expected {}, found {} in {}",
    setting.profile,
    setting.key,
    setting.expected.describe(),
    describe_toml_value(value),
    manifest.display()
  )
}

impl ExpectedTomlValue {
  fn matches(self, value: &Value) -> bool {
    match self {
      Self::Boolean(expected) => value.as_bool() == Some(expected),
      Self::Integer(expected) => value.as_integer() == Some(expected),
      Self::String(expected) => value.as_str() == Some(expected),
    }
  }

  fn describe(self) -> String {
    match self {
      Self::Boolean(value) => value.to_string(),
      Self::Integer(value) => value.to_string(),
      Self::String(value) => format!("{value:?}"),
    }
  }
}

fn describe_toml_value(value: &Value) -> String {
  match value {
    Value::String(value) => format!("{value:?}"),
    Value::Integer(value) => value.to_string(),
    Value::Float(value) => value.to_string(),
    Value::Boolean(value) => value.to_string(),
    Value::Datetime(value) => value.to_string(),
    Value::Array(_) => "array".to_string(),
    Value::Table(_) => "table".to_string(),
  }
}
