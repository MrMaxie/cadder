//! Operating-system identities used by runtime-guard containment records.

use crate::caddy_path_trust::{CaddyPathProvenance, validate_trusted_executable};
use crate::runtime_guard_record::{
  RuntimeGuardIdentity, RuntimeGuardImageIdentity, RuntimeGuardProcessIdentity,
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
  env,
  fs::{File, OpenOptions},
  io::{Read, Seek, SeekFrom},
  path::{Path, PathBuf},
};

#[cfg(debug_assertions)]
const UNTRUSTED_TEST_FIXTURE_ENV: &str = "CADDER_TEST_ALLOW_UNTRUSTED_RUNTIME_GUARD";

pub(crate) struct PinnedRuntimeGuardImage {
  path: PathBuf,
  file_identity: String,
  sha256: String,
  _file: File,
}

impl PinnedRuntimeGuardImage {
  pub(crate) fn open(path: &Path) -> Result<Self> {
    let path = validate_guard_executable(path, "validate the runtime-guard executable source")?;
    let file = open_image_file(&path)?;
    validate_image_file(&file, &path)?;
    let pinned = Self {
      file_identity: image_file_identity(&file)?,
      sha256: file_digest(&file)?,
      path,
      _file: file,
    };
    pinned.reverify_path()?;
    Ok(pinned)
  }

  pub(crate) fn path(&self) -> &Path {
    &self.path
  }

  pub(crate) fn reverify_path(&self) -> Result<()> {
    let path = validate_guard_executable(
      &self.path,
      "revalidate the pinned runtime-guard executable source",
    )?;
    ensure!(
      path == self.path,
      "runtime-guard executable path changed after it was pinned"
    );
    let file = open_image_file(&path)?;
    validate_image_file(&file, &path)?;
    ensure!(
      image_file_identity(&file)? == self.file_identity,
      "runtime-guard executable file identity changed after it was pinned"
    );
    ensure!(
      file_digest(&file)? == self.sha256,
      "runtime-guard executable digest changed after it was pinned"
    );
    Ok(())
  }

  pub(crate) fn image_identity(&self) -> RuntimeGuardImageIdentity {
    RuntimeGuardImageIdentity {
      path: self.path().to_path_buf(),
      file_identity: self.file_identity.clone(),
      sha256: self.sha256.clone(),
    }
  }
}

pub(crate) fn current_guard_identity() -> Result<RuntimeGuardIdentity> {
  let requested = env::current_exe().context("resolve the current runtime-guard executable")?;
  let image = PinnedRuntimeGuardImage::open(&requested)?;
  let identity = RuntimeGuardIdentity {
    process: child_process_identity(std::process::id())?,
    image: image.image_identity(),
  };
  Ok(identity)
}

pub(crate) fn child_process_identity(process_id: u32) -> Result<RuntimeGuardProcessIdentity> {
  ensure!(process_id != 0, "runtime guard process ID must not be zero");
  Ok(RuntimeGuardProcessIdentity {
    process_id,
    creation_identity: process_creation_identity(process_id)?,
  })
}

fn validate_guard_executable(path: &Path, operation: &str) -> Result<PathBuf> {
  #[cfg(debug_assertions)]
  if env::var_os(UNTRUSTED_TEST_FIXTURE_ENV).as_deref() == Some(std::ffi::OsStr::new("1")) {
    ensure!(
      path.is_absolute(),
      "runtime-guard test fixture path must be absolute"
    );
    return path
      .canonicalize()
      .with_context(|| format!("canonicalize runtime-guard test fixture {}", path.display()));
  }

  validate_trusted_executable(path, CaddyPathProvenance::UserOwned).context(operation.to_string())
}

fn file_digest(file: &File) -> Result<String> {
  let mut reader = file
    .try_clone()
    .context("clone the runtime-guard executable handle")?;
  reader
    .seek(SeekFrom::Start(0))
    .context("seek the runtime-guard executable")?;
  let mut hasher = Sha256::new();
  let mut buffer = [0_u8; 64 * 1024];
  loop {
    let read = reader
      .read(&mut buffer)
      .context("hash the runtime-guard executable")?;
    if read == 0 {
      break;
    }
    hasher.update(&buffer[..read]);
  }
  Ok(hex::encode(hasher.finalize()))
}

#[cfg(windows)]
fn process_creation_identity(process_id: u32) -> Result<String> {
  use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
  use windows_sys::Win32::{
    Foundation::FILETIME,
    System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
  };

  // SAFETY: the requested access is query-only, and the returned owned handle is checked and
  // adopted exactly once below.
  let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
  ensure!(
    !handle.is_null(),
    "open process {process_id} for runtime-guard identity: {}",
    std::io::Error::last_os_error()
  );
  // SAFETY: `OpenProcess` returned a fresh owned process handle.
  let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
  let mut creation = FILETIME::default();
  let mut exit = FILETIME::default();
  let mut kernel = FILETIME::default();
  let mut user = FILETIME::default();
  // SAFETY: the process handle remains live and every output points to initialized storage.
  let succeeded = unsafe {
    GetProcessTimes(
      handle.as_raw_handle(),
      &mut creation,
      &mut exit,
      &mut kernel,
      &mut user,
    )
  };
  ensure!(
    succeeded != 0,
    "read process {process_id} creation time: {}",
    std::io::Error::last_os_error()
  );
  let ticks = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
  Ok(format!("windows-filetime-{ticks:016x}"))
}

#[cfg(target_os = "linux")]
fn process_creation_identity(process_id: u32) -> Result<String> {
  let stat_path = format!("/proc/{process_id}/stat");
  let stat = std::fs::read_to_string(&stat_path)
    .with_context(|| format!("read Linux process identity {stat_path}"))?;
  let command_end = stat
    .rfind(')')
    .context("Linux process stat does not contain a complete command field")?;
  let start_ticks = stat[command_end + 1..]
    .split_whitespace()
    .nth(19)
    .context("Linux process stat does not contain its start time")?;
  let start_ticks = start_ticks
    .parse::<u64>()
    .context("Linux process start time is not an integer")?;
  Ok(format!("linux-start-ticks-{start_ticks:016x}"))
}

#[cfg(target_os = "macos")]
fn process_creation_identity(process_id: u32) -> Result<String> {
  use std::mem::{size_of, zeroed};

  let process_id = i32::try_from(process_id).context("macOS process ID exceeds i32")?;
  // SAFETY: `proc_bsdinfo` is a plain C output structure initialized before the system call.
  let mut info = unsafe { zeroed::<libc::proc_bsdinfo>() };
  // SAFETY: the output pointer is valid for exactly the supplied structure size.
  let read = unsafe {
    libc::proc_pidinfo(
      process_id,
      libc::PROC_PIDTBSDINFO,
      0,
      std::ptr::from_mut(&mut info).cast(),
      size_of::<libc::proc_bsdinfo>() as i32,
    )
  };
  ensure!(
    read == size_of::<libc::proc_bsdinfo>() as i32,
    "read macOS process {process_id} creation identity: {}",
    std::io::Error::last_os_error()
  );
  Ok(format!(
    "macos-start-time-{:016x}-{:08x}",
    info.pbi_start_tvsec, info.pbi_start_tvusec
  ))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn process_creation_identity(_process_id: u32) -> Result<String> {
  anyhow::bail!("runtime-guard process identity is unsupported on this Unix platform")
}

#[cfg(unix)]
fn open_image_file(path: &Path) -> Result<File> {
  use std::os::unix::fs::OpenOptionsExt;

  OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
    .open(path)
    .with_context(|| format!("open runtime-guard image {}", path.display()))
}

#[cfg(windows)]
fn open_image_file(path: &Path) -> Result<File> {
  use std::os::windows::fs::OpenOptionsExt;
  use windows_sys::Win32::{
    Foundation::GENERIC_READ,
    Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ},
  };

  OpenOptions::new()
    .access_mode(GENERIC_READ)
    .share_mode(FILE_SHARE_READ)
    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
    .open(path)
    .with_context(|| format!("open runtime-guard image {}", path.display()))
}

#[cfg(unix)]
fn validate_image_file(file: &File, path: &Path) -> Result<()> {
  use std::os::unix::fs::{MetadataExt, PermissionsExt};

  let metadata = file
    .metadata()
    .with_context(|| format!("inspect runtime-guard image {}", path.display()))?;
  ensure!(
    metadata.is_file(),
    "runtime-guard image is not a regular file"
  );
  ensure!(
    metadata.permissions().mode() & 0o111 != 0,
    "runtime-guard image is not executable"
  );
  ensure!(
    metadata.uid() == 0 || metadata.uid() == current_effective_user_id(),
    "runtime-guard image is not owned by root or the runtime owner"
  );
  ensure!(
    metadata.permissions().mode() & 0o022 == 0,
    "runtime-guard image is writable by a less-trusted Unix principal"
  );
  ensure!(
    metadata.nlink() > 0,
    "runtime-guard image has no filesystem links"
  );
  Ok(())
}

#[cfg(unix)]
fn current_effective_user_id() -> u32 {
  // SAFETY: `geteuid` reads process credentials and has no preconditions.
  unsafe { libc::geteuid() }
}

#[cfg(windows)]
fn validate_image_file(file: &File, path: &Path) -> Result<()> {
  use std::os::windows::fs::MetadataExt;
  use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
  };

  let attributes = file
    .metadata()
    .with_context(|| format!("inspect runtime-guard image {}", path.display()))?
    .file_attributes();
  ensure!(
    attributes & FILE_ATTRIBUTE_DIRECTORY == 0,
    "runtime-guard image is not a regular file"
  );
  ensure!(
    attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
    "runtime-guard image is a Windows reparse point"
  );
  Ok(())
}

#[cfg(unix)]
fn image_file_identity(file: &File) -> Result<String> {
  use std::os::unix::fs::MetadataExt;

  let metadata = file
    .metadata()
    .context("inspect runtime-guard file identity")?;
  Ok(format!(
    "unix-device-{:016x}-inode-{:016x}",
    metadata.dev(),
    metadata.ino()
  ))
}

#[cfg(windows)]
fn image_file_identity(file: &File) -> Result<String> {
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::Storage::FileSystem::{
    FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
  };

  let mut information = FILE_ID_INFO::default();
  // SAFETY: `file` owns a live handle and `information` is a correctly sized output buffer.
  let succeeded = unsafe {
    GetFileInformationByHandleEx(
      file.as_raw_handle(),
      FileIdInfo,
      std::ptr::from_mut(&mut information).cast(),
      std::mem::size_of::<FILE_ID_INFO>() as u32,
    )
  };
  ensure!(
    succeeded != 0,
    "inspect runtime-guard file identity: {}",
    std::io::Error::last_os_error()
  );
  Ok(format!(
    "windows-volume-{:016x}-file-{}",
    information.VolumeSerialNumber,
    hex::encode(information.FileId.Identifier)
  ))
}

#[cfg(all(test, windows))]
mod tests {
  use super::*;
  use std::{
    process::{Child, Command, Stdio},
    time::Duration,
  };

  struct OwnedTestChild(Child);

  impl Drop for OwnedTestChild {
    fn drop(&mut self) {
      if self.0.try_wait().ok().flatten().is_none() {
        let _ = self.0.kill();
      }
      let _ = self.0.wait();
    }
  }

  #[test]
  fn runtime_guard_process_identity_is_nonempty_and_stable() {
    let first = child_process_identity(std::process::id()).unwrap();
    let second = child_process_identity(std::process::id()).unwrap();

    assert_ne!(first.process_id, 0);
    assert!(!first.creation_identity.is_empty());
    assert_eq!(first, second);
  }

  #[test]
  fn runtime_guard_pinned_system_image_is_absolute_stable_and_hashed() {
    let path = PathBuf::from(env::var_os("SystemRoot").unwrap())
      .join("System32")
      .join("where.exe");
    let pinned = PinnedRuntimeGuardImage::open(&path).unwrap();
    pinned.reverify_path().unwrap();
    let identity = pinned.image_identity();

    assert!(identity.path.is_absolute());
    assert!(!identity.file_identity.is_empty());
    assert_eq!(identity.sha256.len(), 64);
    assert!(identity.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
  }

  #[test]
  fn runtime_guard_process_identity_rejects_zero_pid() {
    let error = child_process_identity(0).unwrap_err();

    assert!(error.to_string().contains("must not be zero"));
  }

  #[test]
  fn runtime_guard_process_identity_rejects_nonexistent_pid() {
    let error = child_process_identity(u32::MAX).unwrap_err();

    assert!(error.to_string().contains("open process"));
  }

  #[test]
  fn runtime_guard_process_identity_tracks_a_live_child() {
    let mut child = OwnedTestChild(
      Command::new(env::current_exe().unwrap())
        .args([
          "--exact",
          "runtime_guard_identity::tests::runtime_guard_child_fixture",
          "--ignored",
        ])
        .env("CADDER_TEST_RUNTIME_GUARD_CHILD_FIXTURE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap(),
    );

    let first = child_process_identity(child.0.id()).unwrap();
    let second = child_process_identity(child.0.id()).unwrap();

    assert_eq!(first, second);
    assert!(child.0.try_wait().unwrap().is_none());
  }

  #[test]
  #[ignore = "fixture process spawned by runtime_guard_process_identity_tracks_a_live_child"]
  fn runtime_guard_child_fixture() {
    if env::var_os("CADDER_TEST_RUNTIME_GUARD_CHILD_FIXTURE").is_some() {
      std::thread::sleep(Duration::from_secs(30));
    }
  }
}
