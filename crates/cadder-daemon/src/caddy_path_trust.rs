//! Filesystem validation and identity checks for executable images and Caddy configuration.

use anyhow::{Context, Result, ensure};
use std::{
  fs::{self, File},
  path::{Path, PathBuf},
};

/// A Caddy configuration file whose validated identity remains open for parsing.
pub(crate) struct CaddyConfigFile {
  file: File,
  canonical_path: PathBuf,
}

impl CaddyConfigFile {
  pub(crate) fn canonical_path(&self) -> &Path {
    &self.canonical_path
  }

  pub(crate) fn into_file(self) -> File {
    self.file
  }
}

pub(crate) fn validate_caddy_executable(path: &Path) -> Result<PathBuf> {
  validate_native_executable(path, "Caddy executable")
}

pub(crate) fn validate_runtime_guard_executable(path: &Path) -> Result<PathBuf> {
  validate_native_executable(path, "runtime-guard executable")
}

fn validate_native_executable(path: &Path, description: &str) -> Result<PathBuf> {
  #[cfg(windows)]
  reject_windows_command_script(path, description)?;
  #[cfg(windows)]
  platform::reject_final_reparse_point(path, description)?;

  let canonical = canonicalize_absolute(path, description)?;
  let metadata = fs::metadata(&canonical)
    .with_context(|| format!("inspect {description} {}", canonical.display()))?;
  ensure!(
    metadata.is_file(),
    "{description} must resolve to a regular file"
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
      metadata.permissions().mode() & 0o111 != 0,
      "{description} is not executable"
    );
  }
  Ok(canonical)
}

pub(crate) fn open_caddy_config(path: &Path) -> Result<CaddyConfigFile> {
  #[cfg(windows)]
  platform::reject_final_reparse_point(path, "Caddy configuration")?;
  let canonical_path = canonicalize_absolute(path, "Caddy configuration")?;
  let file = File::open(&canonical_path)
    .with_context(|| format!("open Caddy configuration {}", canonical_path.display()))?;
  ensure!(
    file
      .metadata()
      .with_context(|| format!("inspect Caddy configuration {}", canonical_path.display()))?
      .is_file(),
    "Caddy configuration must resolve to a regular file"
  );
  Ok(CaddyConfigFile {
    file,
    canonical_path,
  })
}

/// Reports whether two paths resolve to the same filesystem object.
///
/// Symbolic links are followed before comparing the platform file identity, so this also detects
/// a symlink or hardlink that aliases the Cadder Caddy shim.
pub(crate) fn same_file_identity(left: &Path, right: &Path) -> Result<bool> {
  let left = canonicalize_absolute(left, "first identity path")?;
  let right = canonicalize_absolute(right, "second identity path")?;
  platform::same_file_identity(&left, &right)
}

fn canonicalize_absolute(path: &Path, description: &str) -> Result<PathBuf> {
  ensure!(
    path.is_absolute(),
    "{description} must use an absolute path"
  );
  fs::canonicalize(path).with_context(|| format!("canonicalize {description} {}", path.display()))
}

#[cfg(windows)]
fn reject_windows_command_script(path: &Path, description: &str) -> Result<()> {
  let is_command_script = path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| {
      extension.eq_ignore_ascii_case("bat") || extension.eq_ignore_ascii_case("cmd")
    });
  ensure!(
    !is_command_script,
    "{description} must be a native executable, not a batch script"
  );
  Ok(())
}

#[cfg(unix)]
mod platform {
  use anyhow::{Context, Result};
  use std::{fs, os::unix::fs::MetadataExt, path::Path};

  pub(super) fn same_file_identity(left: &Path, right: &Path) -> Result<bool> {
    let left = fs::metadata(left)
      .with_context(|| format!("inspect first file identity {}", left.display()))?;
    let right = fs::metadata(right)
      .with_context(|| format!("inspect second file identity {}", right.display()))?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
  }
}

#[cfg(windows)]
mod platform {
  use anyhow::{Context, Result, ensure};
  use std::{
    os::windows::{
      ffi::OsStrExt,
      io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::null_mut,
  };
  use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    Storage::FileSystem::{
      BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT,
      FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
      FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
      OPEN_EXISTING,
    },
  };

  pub(super) fn reject_final_reparse_point(path: &Path, description: &str) -> Result<()> {
    ensure!(
      path.is_absolute(),
      "{description} must use an absolute path"
    );
    let handle = open_path(path, true)?;
    let information = file_information(&handle)?;
    ensure!(
      information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
      "{description} must not be a Windows reparse point: {}",
      path.display()
    );
    Ok(())
  }

  pub(super) fn same_file_identity(left: &Path, right: &Path) -> Result<bool> {
    let left = open_path(left, false)?;
    let right = open_path(right, false)?;
    let left = file_information(&left)?;
    let right = file_information(&right)?;
    Ok(
      left.dwVolumeSerialNumber == right.dwVolumeSerialNumber
        && left.nFileIndexHigh == right.nFileIndexHigh
        && left.nFileIndexLow == right.nFileIndexLow,
    )
  }

  fn open_path(path: &Path, inspect_reparse_point: bool) -> Result<OwnedHandle> {
    let encoded = wide_path(path)?;
    let flags = FILE_FLAG_BACKUP_SEMANTICS
      | if inspect_reparse_point {
        FILE_FLAG_OPEN_REPARSE_POINT
      } else {
        0
      };
    // SAFETY: `encoded` is NUL-terminated, no mutable pointers are supplied, and a successful
    // handle is adopted exactly once below.
    let handle = unsafe {
      CreateFileW(
        encoded.as_ptr(),
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        null_mut(),
        OPEN_EXISTING,
        flags,
        null_mut(),
      )
    };
    if handle == INVALID_HANDLE_VALUE {
      return Err(std::io::Error::last_os_error())
        .with_context(|| format!("open path {}", path.display()));
    }
    // SAFETY: `CreateFileW` returned a fresh owned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
  }

  fn file_information(handle: &impl AsRawHandle) -> Result<BY_HANDLE_FILE_INFORMATION> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is live and `information` is a correctly sized initialized out buffer.
    if unsafe { GetFileInformationByHandle(handle.as_raw_handle(), &mut information) } == 0 {
      return Err(std::io::Error::last_os_error()).context("inspect file identity");
    }
    Ok(information)
  }

  fn wide_path(path: &Path) -> Result<Vec<u16>> {
    let mut encoded: Vec<u16> = path.as_os_str().encode_wide().collect();
    ensure!(
      !encoded.contains(&0),
      "Windows path contains an interior NUL byte"
    );
    encoded.push(0);
    Ok(encoded)
  }
}

#[cfg(not(any(unix, windows)))]
mod platform {
  use anyhow::{Result, bail};
  use std::path::Path;

  pub(super) fn same_file_identity(_left: &Path, _right: &Path) -> Result<bool> {
    bail!("file identity is unsupported on this platform")
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{env, fs};

  #[test]
  fn trusted_caddy_source_requires_absolute_executable_path() {
    let error = validate_caddy_executable(Path::new("caddy")).unwrap_err();

    assert!(error.to_string().contains("absolute path"));
  }

  #[test]
  fn runtime_guard_source_requires_absolute_executable_path() {
    let error = validate_runtime_guard_executable(Path::new("cadderd")).unwrap_err();

    assert!(error.to_string().contains("absolute path"));
  }

  #[test]
  fn trusted_caddy_source_same_file_identity_detects_hardlink() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("caddy.exe");
    let alias = temp.path().join("caddy-alias.exe");
    fs::write(&original, b"fixture").unwrap();
    fs::hard_link(&original, &alias).unwrap();

    assert!(same_file_identity(&original, &alias).unwrap());
  }

  #[test]
  fn trusted_caddy_source_same_file_identity_distinguishes_files() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.exe");
    let second = temp.path().join("second.exe");
    fs::write(&first, b"first").unwrap();
    fs::write(&second, b"second").unwrap();

    assert!(!same_file_identity(&first, &second).unwrap());
  }

  #[cfg(windows)]
  #[test]
  fn trusted_caddy_source_windows_rejects_batch_script() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("caddy.CMD");
    fs::write(&script, b"@exit /b 0").unwrap();

    let error = validate_caddy_executable(&script).unwrap_err();

    assert!(error.to_string().contains("batch script"));
  }

  #[cfg(windows)]
  #[test]
  fn runtime_guard_source_windows_rejects_batch_script() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("cadderd.cmd");
    fs::write(&script, b"@exit /b 0").unwrap();

    let error = validate_runtime_guard_executable(&script).unwrap_err();

    assert!(error.to_string().contains("batch script"));
  }

  #[cfg(windows)]
  #[test]
  fn trusted_caddy_source_windows_accepts_system_executable() {
    let executable = PathBuf::from(env::var_os("SystemRoot").unwrap())
      .join("System32")
      .join("where.exe");

    let canonical = validate_caddy_executable(&executable).unwrap();

    assert_eq!(canonical, fs::canonicalize(executable).unwrap());
  }

  #[cfg(windows)]
  #[test]
  fn trusted_caddy_source_windows_accepts_user_writable_config() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config.toml");
    fs::write(&config, "[defaults]").unwrap();

    let config_file = open_caddy_config(&config).unwrap();

    assert_eq!(
      config_file.canonical_path(),
      fs::canonicalize(config).unwrap()
    );
    assert!(config_file.into_file().metadata().unwrap().is_file());
  }

  #[cfg(windows)]
  #[test]
  fn trusted_caddy_source_windows_accepts_user_writable_ancestor() {
    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join("caddy.exe");
    fs::write(&executable, b"fixture").unwrap();

    let canonical = validate_caddy_executable(&executable).unwrap();

    assert_eq!(canonical, fs::canonicalize(executable).unwrap());
  }

  #[cfg(windows)]
  #[test]
  fn runtime_guard_source_windows_accepts_user_writable_ancestor() {
    let executable = env::current_exe().unwrap();

    let canonical = validate_runtime_guard_executable(&executable).unwrap();

    assert_eq!(canonical, fs::canonicalize(executable).unwrap());
  }

  #[cfg(windows)]
  #[test]
  fn trusted_caddy_source_windows_rejects_final_reparse_point() {
    use std::os::windows::fs::symlink_file;

    let home = PathBuf::from(env::var_os("USERPROFILE").unwrap());
    let temp = tempfile::Builder::new()
      .prefix("cadder-path-trust-")
      .tempdir_in(home)
      .unwrap();
    let target = temp.path().join("real-caddy.exe");
    let alias = temp.path().join("caddy.exe");
    fs::write(&target, b"fixture").unwrap();
    symlink_file(&target, &alias).expect("create Windows symlink fixture");

    let error = validate_caddy_executable(&alias).unwrap_err();

    assert!(error.to_string().contains("reparse point"));
  }

  #[cfg(unix)]
  fn executable_fixture() -> (tempfile::TempDir, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join("executable");
    fs::write(&executable, b"fixture").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    (temp, executable)
  }

  #[cfg(unix)]
  #[test]
  fn trusted_caddy_source_unix_accepts_executable() {
    let (_temp, executable) = executable_fixture();

    let canonical = validate_caddy_executable(&executable).unwrap();

    assert_eq!(canonical, fs::canonicalize(executable).unwrap());
  }

  #[cfg(unix)]
  #[test]
  fn trusted_caddy_source_unix_rejects_non_executable_file() {
    use std::os::unix::fs::PermissionsExt;

    let (_temp, executable) = executable_fixture();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o600)).unwrap();

    let error = validate_caddy_executable(&executable).unwrap_err();

    assert!(error.to_string().contains("not executable"));
  }

  #[cfg(unix)]
  #[test]
  fn runtime_guard_source_unix_accepts_writable_ancestor() {
    use std::os::unix::fs::PermissionsExt;

    let (temp, executable) = executable_fixture();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o770)).unwrap();

    let canonical = validate_runtime_guard_executable(&executable).unwrap();

    assert_eq!(canonical, fs::canonicalize(executable).unwrap());
  }

  #[cfg(unix)]
  #[test]
  fn trusted_caddy_source_unix_same_file_identity_follows_symlink() {
    use std::os::unix::fs::symlink;

    let (_temp, executable) = executable_fixture();
    let alias = executable.with_file_name("caddy-alias");
    symlink(&executable, &alias).unwrap();

    assert!(same_file_identity(&executable, &alias).unwrap());
  }
}
