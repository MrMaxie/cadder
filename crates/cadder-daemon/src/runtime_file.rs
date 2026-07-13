use crate::RuntimePaths;
use anyhow::{Context, Result, bail};
use std::{
  fs::{self, File},
  io,
  path::{Path, PathBuf},
};
use tokio::io::AsyncWriteExt;

const CANDIDATE_PREFIX: &str = ".effective-caddy.";
const CANDIDATE_SUFFIX: &str = ".tmp";
const GENERATION_BYTES: usize = 16;
const CREATE_ATTEMPTS: usize = 8;

/// A complete runtime configuration that is not authoritative until promoted.
#[derive(Debug)]
pub(crate) struct StagedRuntimeConfig {
  candidate_path: PathBuf,
  effective_path: PathBuf,
  promoted: bool,
}

impl StagedRuntimeConfig {
  pub(crate) async fn stage(paths: &RuntimePaths, rendered: &[u8]) -> Result<Self> {
    paths.ensure_dirs()?;
    let effective_path = paths.effective_config_path();
    let (staged, file) = create_candidate(paths, effective_path)?;
    let mut file = tokio::fs::File::from_std(file);
    file.write_all(rendered).await.with_context(|| {
      format!(
        "write staged Caddy config {}",
        staged.candidate_path.display()
      )
    })?;
    file.sync_all().await.with_context(|| {
      format!(
        "flush staged Caddy config {}",
        staged.candidate_path.display()
      )
    })?;
    drop(file);
    Ok(staged)
  }

  pub(crate) fn path(&self) -> &Path {
    &self.candidate_path
  }

  pub(crate) fn promote(&mut self) -> Result<()> {
    install_owner_only_file(&self.candidate_path, &self.effective_path).with_context(|| {
      format!(
        "promote staged Caddy config {} to {}",
        self.candidate_path.display(),
        self.effective_path.display()
      )
    })?;
    self.promoted = true;
    finalize_owner_only_file(&self.effective_path).with_context(|| {
      format!(
        "finalize promoted Caddy config {}",
        self.effective_path.display()
      )
    })
  }
}

impl Drop for StagedRuntimeConfig {
  fn drop(&mut self) {
    if !self.promoted {
      let _ = fs::remove_file(&self.candidate_path);
    }
  }
}

pub(crate) async fn read_effective_config(paths: &RuntimePaths) -> Result<Option<Vec<u8>>> {
  let path = paths.effective_config_path();
  if !path.exists() {
    return Ok(None);
  }
  secure_existing_owner_only_file(&path)
    .with_context(|| format!("secure effective Caddy config {}", path.display()))?;
  tokio::fs::read(&path)
    .await
    .map(Some)
    .with_context(|| format!("read effective Caddy config {}", path.display()))
}

pub(crate) async fn restore_effective_config(
  paths: &RuntimePaths,
  previous: Option<&[u8]>,
) -> Result<()> {
  match previous {
    Some(previous) => {
      let mut staged = StagedRuntimeConfig::stage(paths, previous).await?;
      staged.promote()
    }
    None => remove_effective_config(paths),
  }
}

pub(crate) fn cleanup_stale_config_candidates(paths: &RuntimePaths) -> Result<()> {
  paths.ensure_dirs()?;
  let mut removed_any = false;
  for entry in fs::read_dir(paths.runtime_dir())? {
    let entry = entry?;
    let path = entry.path();
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
      continue;
    };
    if !is_candidate_name(name) || validate_owner_only_runtime_file(&path).is_err() {
      continue;
    }
    fs::remove_file(&path)
      .with_context(|| format!("remove stale Caddy config candidate {}", path.display()))?;
    removed_any = true;
  }
  if removed_any {
    finalize_removed_file(&paths.effective_config_path())?;
  }
  Ok(())
}

fn is_candidate_name(name: &str) -> bool {
  let Some(generation) = name
    .strip_prefix(CANDIDATE_PREFIX)
    .and_then(|name| name.strip_suffix(CANDIDATE_SUFFIX))
  else {
    return false;
  };
  generation.len() == GENERATION_BYTES * 2
    && generation
      .bytes()
      .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn create_candidate(
  paths: &RuntimePaths,
  effective_path: PathBuf,
) -> Result<(StagedRuntimeConfig, File)> {
  for _ in 0..CREATE_ATTEMPTS {
    let candidate_path = candidate_path(paths)?;
    match create_owner_only_runtime_file(paths, &candidate_path) {
      Ok(file) => {
        return Ok((
          StagedRuntimeConfig {
            candidate_path,
            effective_path,
            promoted: false,
          },
          file,
        ));
      }
      Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
      Err(error) => return Err(error).context("create staged Caddy config"),
    }
  }
  bail!("could not allocate a unique staged Caddy config after {CREATE_ATTEMPTS} attempts")
}

fn candidate_path(paths: &RuntimePaths) -> Result<PathBuf> {
  let mut bytes = [0_u8; GENERATION_BYTES];
  getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
  Ok(paths.runtime_dir().join(format!(
    "{CANDIDATE_PREFIX}{}{CANDIDATE_SUFFIX}",
    hex::encode(bytes)
  )))
}

#[cfg(unix)]
fn create_owner_only_runtime_file(paths: &RuntimePaths, path: &Path) -> io::Result<File> {
  crate::ipc_unix_security::create_owner_only_runtime_file(paths, path)
}

#[cfg(windows)]
fn create_owner_only_runtime_file(_paths: &RuntimePaths, path: &Path) -> io::Result<File> {
  crate::ipc_windows_security::create_owner_only_runtime_file(path)
}

#[cfg(unix)]
fn secure_existing_owner_only_file(path: &Path) -> io::Result<()> {
  crate::ipc_unix_security::secure_owner_only_runtime_file(path)
}

#[cfg(unix)]
fn validate_owner_only_runtime_file(path: &Path) -> io::Result<()> {
  crate::ipc_unix_security::validate_owner_only_runtime_file(path)
}

#[cfg(windows)]
fn validate_owner_only_runtime_file(path: &Path) -> io::Result<()> {
  crate::ipc_windows_security::validate_owner_only_runtime_file(path)
}

#[cfg(not(any(unix, windows)))]
fn validate_owner_only_runtime_file(path: &Path) -> io::Result<()> {
  path.metadata().and_then(|metadata| {
    if metadata.is_file() {
      Ok(())
    } else {
      Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "runtime config candidate is not a regular file",
      ))
    }
  })
}

#[cfg(windows)]
fn secure_existing_owner_only_file(path: &Path) -> io::Result<()> {
  crate::ipc_windows_security::secure_owner_only_path(path)
}

#[cfg(not(any(unix, windows)))]
fn secure_existing_owner_only_file(path: &Path) -> io::Result<()> {
  path.metadata().map(|_| ())
}

#[cfg(not(any(unix, windows)))]
fn create_owner_only_runtime_file(_paths: &RuntimePaths, path: &Path) -> io::Result<File> {
  std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(path)
}

#[cfg(unix)]
fn install_owner_only_file(temporary: &Path, destination: &Path) -> io::Result<()> {
  if destination.exists() {
    crate::ipc_unix_security::secure_owner_only_runtime_file(destination)?;
  }
  fs::rename(temporary, destination)
}

#[cfg(unix)]
fn finalize_owner_only_file(destination: &Path) -> io::Result<()> {
  crate::ipc_unix_security::sync_parent_directory(destination)
}

#[cfg(windows)]
fn install_owner_only_file(temporary: &Path, destination: &Path) -> io::Result<()> {
  crate::ipc_windows_security::install_discovery_file(temporary, destination)
}

#[cfg(windows)]
fn finalize_owner_only_file(destination: &Path) -> io::Result<()> {
  crate::ipc_windows_security::secure_owner_only_path(destination)
}

#[cfg(not(any(unix, windows)))]
fn install_owner_only_file(temporary: &Path, destination: &Path) -> io::Result<()> {
  fs::rename(temporary, destination)
}

#[cfg(not(any(unix, windows)))]
fn finalize_owner_only_file(_destination: &Path) -> io::Result<()> {
  Ok(())
}

fn remove_effective_config(paths: &RuntimePaths) -> Result<()> {
  let path = paths.effective_config_path();
  if !path.exists() {
    return Ok(());
  }
  secure_existing_owner_only_file(&path)
    .with_context(|| format!("secure effective Caddy config {}", path.display()))?;
  fs::remove_file(&path)
    .with_context(|| format!("remove effective Caddy config {}", path.display()))?;
  finalize_removed_file(&path)
    .with_context(|| format!("finalize removal of Caddy config {}", path.display()))
}

#[cfg(unix)]
fn finalize_removed_file(path: &Path) -> io::Result<()> {
  crate::ipc_unix_security::sync_parent_directory(path)
}

#[cfg(not(unix))]
fn finalize_removed_file(_path: &Path) -> io::Result<()> {
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn dropped_candidate_does_not_change_effective_config() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    fs::write(paths.effective_config_path(), b"previous").unwrap();

    let staged = StagedRuntimeConfig::stage(&paths, b"candidate")
      .await
      .unwrap();
    let candidate_path = staged.path().to_path_buf();

    assert_eq!(
      fs::read(paths.effective_config_path()).unwrap(),
      b"previous"
    );
    assert_eq!(fs::read(&candidate_path).unwrap(), b"candidate");
    drop(staged);
    assert!(!candidate_path.exists());
    assert_eq!(
      fs::read(paths.effective_config_path()).unwrap(),
      b"previous"
    );
  }

  #[tokio::test]
  async fn promotion_atomically_replaces_effective_config() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    fs::write(paths.effective_config_path(), b"previous").unwrap();

    let mut staged = StagedRuntimeConfig::stage(&paths, b"candidate")
      .await
      .unwrap();
    let candidate_path = staged.path().to_path_buf();
    staged.promote().unwrap();

    assert!(!candidate_path.exists());
    assert_eq!(
      fs::read(paths.effective_config_path()).unwrap(),
      b"candidate"
    );

    #[cfg(windows)]
    crate::ipc_windows_security::validate_owner_only_runtime_file(&paths.effective_config_path())
      .unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      assert_eq!(
        fs::metadata(paths.effective_config_path())
          .unwrap()
          .permissions()
          .mode()
          & 0o777,
        0o600
      );
    }
  }

  #[test]
  fn cleanup_removes_only_validated_exact_candidate_files() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let stale = paths
      .runtime_dir()
      .join(".effective-caddy.00112233445566778899aabbccddeeff.tmp");
    let unrelated = paths
      .runtime_dir()
      .join(".effective-caddy.not-a-generation.tmp");
    let stale_file = create_owner_only_runtime_file(&paths, &stale).unwrap();
    drop(stale_file);
    fs::write(&unrelated, b"keep").unwrap();

    cleanup_stale_config_candidates(&paths).unwrap();

    assert!(!stale.exists());
    assert!(unrelated.exists());
  }
}
