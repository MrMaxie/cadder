use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};
use std::fs;
use std::{
  fmt,
  path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RuntimeProfile {
  #[default]
  Default,
}

impl RuntimeProfile {
  pub fn as_str(self) -> &'static str {
    "default"
  }
}

impl fmt::Display for RuntimeProfile {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
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
    _runtime_profile: Option<RuntimeProfile>,
  ) -> Result<Self> {
    let runtime_dir = override_dir.map_or_else(runtime_dir_for_current_executable, Ok)?;
    Self::from_runtime_dir(runtime_dir)
  }

  pub fn for_executable(executable: &Path) -> Result<Self> {
    let runtime_dir = executable
      .parent()
      .map(Path::to_path_buf)
      .ok_or_else(|| anyhow!("Cadder executable path has no parent directory"))?;
    Self::from_runtime_dir(runtime_dir)
  }

  fn from_runtime_dir(runtime_dir: PathBuf) -> Result<Self> {
    let storage_paths = StoragePaths::new(runtime_dir.join("data"));

    let mut hasher = Sha256::new();
    hasher.update(runtime_dir.to_string_lossy().as_bytes());
    let instance_key = hex::encode(&hasher.finalize()[..8]);
    let socket_name = format!("cadder-{instance_key}.sock");

    Ok(Self {
      runtime_dir,
      storage_paths,
      instance_key,
      socket_name,
      runtime_profile: RuntimeProfile::Default,
    })
  }

  pub fn ensure_dirs(&self) -> Result<()> {
    fs::create_dir_all(&self.runtime_dir)
      .with_context(|| format!("create runtime directory {}", self.runtime_dir.display()))
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

  pub fn containment_lock_path(&self) -> PathBuf {
    self.runtime_dir.join("cadder-containment.lock")
  }

  pub fn containment_record_path(&self) -> PathBuf {
    self.runtime_dir.join("cadder-containment.json")
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

fn runtime_dir_for_current_executable() -> Result<PathBuf> {
  let executable = std::env::current_exe().context("resolve Cadder executable path")?;
  executable
    .parent()
    .map(Path::to_path_buf)
    .ok_or_else(|| anyhow!("Cadder executable path has no parent directory"))
}

#[cfg(test)]
mod tests {
  use super::*;

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
      first.containment_lock_path(),
      dir.path().join("cadder-containment.lock")
    );
    assert_eq!(
      first.containment_record_path(),
      dir.path().join("cadder-containment.json")
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
  fn for_executable_uses_its_parent_as_the_runtime_directory() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("bin").join("cadder.exe");
    let paths = RuntimePaths::for_executable(&executable).unwrap();

    assert_eq!(paths.runtime_dir(), dir.path().join("bin"));
    assert_eq!(
      paths.storage_paths().profile_dir(),
      dir.path().join("bin/data")
    );
  }

  #[test]
  fn resolve_uses_the_current_executable_parent() {
    let paths = RuntimePaths::resolve(None).unwrap();
    let executable = std::env::current_exe().unwrap();

    assert_eq!(paths.runtime_dir(), executable.parent().unwrap());
  }

  #[test]
  fn ensure_dirs_creates_runtime_directory() {
    let dir = tempfile::tempdir().unwrap();
    let runtime_dir = dir.path().join("nested").join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();

    paths.ensure_dirs().unwrap();

    assert!(runtime_dir.is_dir());
  }
}
