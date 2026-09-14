use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RuntimePaths {
  runtime_dir: PathBuf,
  storage_paths: StoragePaths,
  instance_key: String,
  socket_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoragePaths {
  profile_dir: PathBuf,
}

impl StoragePaths {
  fn new(profile_dir: PathBuf) -> Self {
    Self { profile_dir }
  }

  #[cfg(test)]
  pub(crate) fn new_for_test(profile_dir: PathBuf) -> Self {
    Self::new(profile_dir)
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

  pub fn storage_paths(&self) -> &StoragePaths {
    &self.storage_paths
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
mod tests;
