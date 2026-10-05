//! Filesystem validation and identity checks for executable images and Caddy configuration.

use anyhow::{Context, Result, ensure};
use same_file::is_same_file;
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
  is_same_file(&left, &right).with_context(|| {
    format!(
      "compare identity of {} with {}",
      left.display(),
      right.display()
    )
  })
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

#[cfg(test)]
mod tests;
