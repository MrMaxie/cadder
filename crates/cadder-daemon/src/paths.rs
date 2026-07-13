use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use sha2::{Digest, Sha256};
#[cfg(not(unix))]
use std::fs;
use std::{
  env, fmt,
  path::{Path, PathBuf},
  str::FromStr,
};

pub const CADDER_RUNTIME_PROFILE_ENV: &str = "CADDER_RUNTIME_PROFILE";
pub const CADDER_DEV_WORKSPACE_ENV: &str = "CADDER_DEV_WORKSPACE";
pub const CADDER_DEV_ID_ENV: &str = "CADDER_DEV_ID";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RuntimeProfile {
  #[default]
  Default,
  Dev,
}

impl RuntimeProfile {
  pub fn from_env() -> Result<Self> {
    env::var(CADDER_RUNTIME_PROFILE_ENV)
      .ok()
      .map_or(Ok(Self::Default), |value| value.parse())
  }

  pub fn parse_cli(value: &str) -> std::result::Result<Self, String> {
    value.parse::<Self>().map_err(|error| error.to_string())
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::Default => "default",
      Self::Dev => "dev",
    }
  }
}

impl fmt::Display for RuntimeProfile {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for RuntimeProfile {
  type Err = anyhow::Error;

  fn from_str(value: &str) -> Result<Self> {
    match value.trim().to_ascii_lowercase().as_str() {
      "" | "default" | "prod" | "production" => Ok(Self::Default),
      "dev" | "development" => Ok(Self::Dev),
      other => Err(anyhow!(
        "unknown Cadder runtime profile `{other}`; expected `default` or `dev`"
      )),
    }
  }
}

#[derive(Debug, Clone)]
pub struct RuntimePaths {
  runtime_dir: PathBuf,
  storage_paths: StoragePaths,
  instance_key: String,
  socket_name: String,
  runtime_profile: RuntimeProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoragePaths {
  profile_dir: PathBuf,
}

impl StoragePaths {
  fn new(profile_dir: PathBuf) -> Self {
    Self { profile_dir }
  }

  pub fn profile_dir(&self) -> &Path {
    &self.profile_dir
  }

  pub fn lock_path(&self) -> PathBuf {
    self.profile_dir.join("storage.lock")
  }

  pub fn manifest_path(&self) -> PathBuf {
    self.profile_dir.join("manifest.json")
  }

  pub fn generations_dir(&self) -> PathBuf {
    self.profile_dir.join("generations")
  }

  pub fn plans_dir(&self) -> PathBuf {
    self.profile_dir.join("plans")
  }

  pub fn secrets_dir(&self) -> PathBuf {
    self.profile_dir.join("secrets")
  }

  pub fn recovery_dir(&self) -> PathBuf {
    self.profile_dir.join("recovery")
  }
}

impl RuntimePaths {
  pub fn resolve(override_dir: Option<PathBuf>) -> Result<Self> {
    Self::resolve_with_profile(override_dir, None)
  }

  pub fn resolve_with_profile(
    override_dir: Option<PathBuf>,
    runtime_profile: Option<RuntimeProfile>,
  ) -> Result<Self> {
    let env_runtime_dir = env::var_os("CADDER_RUNTIME_DIR");
    let runtime_profile = match runtime_profile {
      Some(profile) => profile,
      None if override_dir.is_none() && env_runtime_dir.is_none() => RuntimeProfile::from_env()?,
      None => RuntimeProfile::Default,
    };
    let runtime_override = override_dir.or_else(|| env_runtime_dir.map(PathBuf::from));
    let (runtime_dir, storage_paths) = if let Some(path) = runtime_override {
      let storage_paths = StoragePaths::new(path.join("data"));
      (path, storage_paths)
    } else {
      profile_paths(runtime_profile)?
    };

    let mut hasher = Sha256::new();
    hasher.update(runtime_dir.to_string_lossy().as_bytes());
    let instance_key = hex::encode(&hasher.finalize()[..8]);
    let socket_name = format!("cadder-{instance_key}.sock");

    Ok(Self {
      runtime_dir,
      storage_paths,
      instance_key,
      socket_name,
      runtime_profile,
    })
  }

  pub fn ensure_dirs(&self) -> Result<()> {
    #[cfg(unix)]
    {
      crate::ipc_unix_security::secure_runtime_paths(self)
        .with_context(|| format!("secure runtime directory {}", self.runtime_dir.display()))
    }

    #[cfg(windows)]
    {
      fs::create_dir_all(&self.runtime_dir)
        .with_context(|| format!("create runtime directory {}", self.runtime_dir.display()))?;
      crate::ipc_windows_security::secure_owner_only_runtime_directory(&self.runtime_dir)
        .with_context(|| format!("secure runtime directory {}", self.runtime_dir.display()))
    }

    #[cfg(not(any(unix, windows)))]
    {
      fs::create_dir_all(&self.runtime_dir)
        .with_context(|| format!("create runtime directory {}", self.runtime_dir.display()))
    }
  }

  pub fn runtime_dir(&self) -> &Path {
    &self.runtime_dir
  }

  pub fn socket_name(&self) -> &str {
    &self.socket_name
  }

  pub fn instance_key(&self) -> &str {
    &self.instance_key
  }

  pub fn runtime_profile(&self) -> RuntimeProfile {
    self.runtime_profile
  }

  pub fn storage_paths(&self) -> &StoragePaths {
    &self.storage_paths
  }

  pub fn lock_path(&self) -> PathBuf {
    self.runtime_dir.join("cadder.lock")
  }

  pub fn lock_metadata_path(&self) -> PathBuf {
    self.runtime_dir.join("cadder.lock.json")
  }

  pub fn ipc_endpoint_path(&self) -> PathBuf {
    self.runtime_dir.join("cadder-ipc.json")
  }

  pub fn ipc_discovery_lock_path(&self) -> PathBuf {
    self.runtime_dir.join("cadder-ipc.lock")
  }

  pub fn metadata_path(&self) -> PathBuf {
    self.runtime_dir.join("daemon.json")
  }

  pub fn effective_config_path(&self) -> PathBuf {
    self.runtime_dir.join("effective-caddy.json")
  }
}

fn profile_paths(runtime_profile: RuntimeProfile) -> Result<(PathBuf, StoragePaths)> {
  let dirs = ProjectDirs::from("dev", "Cadder", "Cadder")
    .ok_or_else(|| anyhow!("could not resolve per-user project directories"))?;
  let runtime_dir = dirs
    .runtime_dir()
    .map(Path::to_path_buf)
    .unwrap_or_else(|| dirs.data_local_dir().join("run"));
  let storage_dir = dirs.data_local_dir().to_path_buf();
  match runtime_profile {
    RuntimeProfile::Default => Ok((runtime_dir, StoragePaths::new(storage_dir))),
    RuntimeProfile::Dev => {
      let profile_id = dev_profile_id()?;
      let profile_suffix = Path::new("profiles").join("dev").join(profile_id);
      Ok((
        runtime_dir.join(&profile_suffix),
        StoragePaths::new(storage_dir.join(profile_suffix)),
      ))
    }
  }
}

fn dev_profile_id() -> Result<String> {
  if let Some(value) = env::var(CADDER_DEV_ID_ENV)
    .ok()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
  {
    return validate_dev_profile_id(value);
  }

  let workspace = env::var_os(CADDER_DEV_WORKSPACE_ENV)
    .map(PathBuf::from)
    .map(Ok)
    .unwrap_or_else(env::current_dir)
    .context("resolve Cadder dev workspace")?;
  let canonical = workspace.canonicalize().unwrap_or(workspace);
  let mut hasher = Sha256::new();
  hasher.update(canonical.to_string_lossy().as_bytes());
  Ok(format!(
    "workspace-{}",
    hex::encode(&hasher.finalize()[..8])
  ))
}

fn validate_dev_profile_id(value: String) -> Result<String> {
  if !matches!(value.as_str(), "." | "..")
    && value
      .chars()
      .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
  {
    Ok(value)
  } else {
    Err(anyhow!(
      "{CADDER_DEV_ID_ENV} may only contain ASCII letters, digits, `.`, `_`, or `-`"
    ))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{ffi::OsString, fs};

  struct EnvSnapshot {
    key: &'static str,
    value: Option<OsString>,
  }

  impl EnvSnapshot {
    fn capture(key: &'static str) -> Self {
      Self {
        key,
        value: env::var_os(key),
      }
    }
  }

  impl Drop for EnvSnapshot {
    fn drop(&mut self) {
      unsafe {
        match &self.value {
          Some(value) => env::set_var(self.key, value),
          None => env::remove_var(self.key),
        }
      }
    }
  }

  fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    crate::TEST_ENV_LOCK
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner())
  }

  #[test]
  fn resolve_override_derives_stable_socket_and_runtime_paths() {
    let dir = tempfile::tempdir().unwrap();

    let first = RuntimePaths::resolve(Some(dir.path().to_path_buf())).unwrap();
    let second = RuntimePaths::resolve(Some(dir.path().to_path_buf())).unwrap();

    assert_eq!(first.runtime_dir(), dir.path());
    assert_eq!(first.storage_paths().profile_dir(), dir.path().join("data"));
    assert_eq!(
      first.storage_paths().lock_path(),
      dir.path().join("data").join("storage.lock")
    );
    assert_eq!(
      first.storage_paths().manifest_path(),
      dir.path().join("data").join("manifest.json")
    );
    assert_eq!(
      first.storage_paths().generations_dir(),
      dir.path().join("data").join("generations")
    );
    assert_eq!(
      first.storage_paths().plans_dir(),
      dir.path().join("data").join("plans")
    );
    assert_eq!(
      first.storage_paths().secrets_dir(),
      dir.path().join("data").join("secrets")
    );
    assert_eq!(
      first.storage_paths().recovery_dir(),
      dir.path().join("data").join("recovery")
    );
    assert_eq!(first.instance_key(), second.instance_key());
    assert_eq!(first.socket_name(), second.socket_name());
    assert!(first.socket_name().starts_with("cadder-"));
    assert_eq!(first.runtime_profile(), RuntimeProfile::Default);
    assert_eq!(first.lock_path(), dir.path().join("cadder.lock"));
    assert_eq!(
      first.lock_metadata_path(),
      dir.path().join("cadder.lock.json")
    );
    assert_eq!(
      first.ipc_endpoint_path(),
      dir.path().join("cadder-ipc.json")
    );
    assert_eq!(first.metadata_path(), dir.path().join("daemon.json"));
    assert_eq!(
      first.effective_config_path(),
      dir.path().join("effective-caddy.json")
    );
  }

  #[test]
  fn resolve_uses_runtime_dir_environment_override() {
    let _lock = lock_env();
    let _snapshot = EnvSnapshot::capture("CADDER_RUNTIME_DIR");
    let _profile_snapshot = EnvSnapshot::capture(CADDER_RUNTIME_PROFILE_ENV);
    let dir = tempfile::tempdir().unwrap();
    let runtime_dir = dir.path().join("env-runtime");
    unsafe {
      env::set_var("CADDER_RUNTIME_DIR", &runtime_dir);
      env::set_var(CADDER_RUNTIME_PROFILE_ENV, "dev");
    }

    let paths = RuntimePaths::resolve(None).unwrap();

    assert_eq!(paths.runtime_dir(), runtime_dir);
    assert_eq!(
      paths.storage_paths().profile_dir(),
      runtime_dir.join("data")
    );
    assert!(paths.socket_name().starts_with("cadder-"));
  }

  #[test]
  fn resolve_without_override_uses_project_runtime_location() {
    let _lock = lock_env();
    let _snapshot = EnvSnapshot::capture("CADDER_RUNTIME_DIR");
    let _profile_snapshot = EnvSnapshot::capture(CADDER_RUNTIME_PROFILE_ENV);
    unsafe {
      env::remove_var("CADDER_RUNTIME_DIR");
      env::remove_var(CADDER_RUNTIME_PROFILE_ENV);
    }

    let paths = RuntimePaths::resolve(None).unwrap();
    let dirs = ProjectDirs::from("dev", "Cadder", "Cadder").unwrap();

    assert!(!paths.runtime_dir().as_os_str().is_empty());
    assert_eq!(paths.storage_paths().profile_dir(), dirs.data_local_dir());
    assert!(paths.socket_name().starts_with("cadder-"));
  }

  #[test]
  fn resolve_dev_profile_uses_deterministic_workspace_identity() {
    let _lock = lock_env();
    let _runtime_snapshot = EnvSnapshot::capture("CADDER_RUNTIME_DIR");
    let _profile_snapshot = EnvSnapshot::capture(CADDER_RUNTIME_PROFILE_ENV);
    let _workspace_snapshot = EnvSnapshot::capture(CADDER_DEV_WORKSPACE_ENV);
    let _id_snapshot = EnvSnapshot::capture(CADDER_DEV_ID_ENV);
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    unsafe {
      env::remove_var("CADDER_RUNTIME_DIR");
      env::set_var(CADDER_RUNTIME_PROFILE_ENV, "dev");
      env::set_var(CADDER_DEV_WORKSPACE_ENV, &workspace);
      env::remove_var(CADDER_DEV_ID_ENV);
    }

    let first = RuntimePaths::resolve(None).unwrap();
    let second = RuntimePaths::resolve_with_profile(None, Some(RuntimeProfile::Dev)).unwrap();
    let default = RuntimePaths::resolve_with_profile(None, Some(RuntimeProfile::Default)).unwrap();

    assert_eq!(first.runtime_dir(), second.runtime_dir());
    assert_ne!(first.runtime_dir(), default.runtime_dir());
    assert_eq!(first.storage_paths(), second.storage_paths());
    assert_ne!(first.storage_paths(), default.storage_paths());
    assert_eq!(first.runtime_profile(), RuntimeProfile::Dev);
    assert_eq!(default.runtime_profile(), RuntimeProfile::Default);
    assert!(first.runtime_dir().ends_with(first_profile_id(&workspace)));
    assert!(
      first
        .storage_paths()
        .profile_dir()
        .ends_with(first_profile_id(&workspace))
    );
    assert_eq!(first.socket_name(), second.socket_name());
    assert_eq!(first.instance_key(), second.instance_key());
  }

  #[test]
  fn resolve_dev_profile_id_override_controls_runtime_identity() {
    let _lock = lock_env();
    let _runtime_snapshot = EnvSnapshot::capture("CADDER_RUNTIME_DIR");
    let _profile_snapshot = EnvSnapshot::capture(CADDER_RUNTIME_PROFILE_ENV);
    let _workspace_snapshot = EnvSnapshot::capture(CADDER_DEV_WORKSPACE_ENV);
    let _id_snapshot = EnvSnapshot::capture(CADDER_DEV_ID_ENV);
    unsafe {
      env::remove_var("CADDER_RUNTIME_DIR");
      env::set_var(CADDER_RUNTIME_PROFILE_ENV, "dev");
      env::set_var(CADDER_DEV_WORKSPACE_ENV, "ignored-workspace");
      env::set_var(CADDER_DEV_ID_ENV, "ci-profile");
    }

    let paths = RuntimePaths::resolve(None).unwrap();

    assert!(paths.runtime_dir().ends_with("ci-profile"));
  }

  #[test]
  fn runtime_profile_parser_rejects_unknown_values() {
    let error = "isolated".parse::<RuntimeProfile>().unwrap_err();

    assert!(error.to_string().contains("unknown Cadder runtime profile"));
  }

  #[test]
  fn ensure_dirs_creates_runtime_directory() {
    let dir = tempfile::tempdir().unwrap();
    let runtime_dir = dir.path().join("nested").join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();

    paths.ensure_dirs().unwrap();

    assert!(runtime_dir.is_dir());
  }

  fn first_profile_id(workspace: &Path) -> String {
    let canonical = workspace.canonicalize().unwrap();
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    format!("workspace-{}", hex::encode(&hasher.finalize()[..8]))
  }
}
