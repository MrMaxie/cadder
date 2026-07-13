//! Read-only filesystem trust checks for real-Caddy executables and trusted configuration.

use anyhow::{Context, Result, ensure};
use std::{
  fs::{self, File},
  path::{Path, PathBuf},
};

/// Identifies which principals may own or mutate a trusted Caddy path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaddyPathProvenance {
  /// The runtime owner and platform system administrators are trusted.
  UserOwned,
  /// Only platform system administrators are trusted.
  SystemOwned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrustedPathKind {
  Executable,
  Config,
}

/// A trusted configuration file whose validated identity remains open for parsing.
pub(crate) struct TrustedConfigFile {
  file: File,
  canonical_path: PathBuf,
}

impl TrustedConfigFile {
  pub(crate) fn canonical_path(&self) -> &Path {
    &self.canonical_path
  }

  pub(crate) fn into_file(self) -> File {
    self.file
  }
}

/// Canonicalizes and validates a real-Caddy executable without changing its permissions.
///
/// The executable must be an absolute regular file. Its owner and every canonical parent
/// directory must match `provenance`, and no less-trusted principal may replace or modify it.
/// Unix additionally requires an executable mode bit. Windows rejects batch scripts and a final
/// reparse point.
pub(crate) fn validate_trusted_executable(
  path: &Path,
  provenance: CaddyPathProvenance,
) -> Result<PathBuf> {
  #[cfg(windows)]
  reject_windows_command_script(path)?;

  validate_trusted_path(path, provenance, TrustedPathKind::Executable)
}

/// Opens and validates a trusted Caddy configuration without changing its permissions.
///
/// The returned handle remains bound to the validated file identity, so callers must parse this
/// handle instead of reopening the requested path.
pub(crate) fn validate_trusted_config(
  path: &Path,
  provenance: CaddyPathProvenance,
) -> Result<TrustedConfigFile> {
  #[cfg(windows)]
  platform::reject_final_reparse_point(path, TrustedPathKind::Config)?;
  let canonical_path = canonicalize_absolute(path, TrustedPathKind::Config.description())?;
  let file = platform::open_trusted_config(&canonical_path)?;
  platform::validate_config_file(&file, &canonical_path, provenance)?;
  Ok(TrustedConfigFile {
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

fn validate_trusted_path(
  path: &Path,
  provenance: CaddyPathProvenance,
  kind: TrustedPathKind,
) -> Result<PathBuf> {
  #[cfg(windows)]
  platform::reject_final_reparse_point(path, kind)?;
  let canonical = canonicalize_absolute(path, kind.description())?;
  platform::validate_canonical_path(&canonical, provenance, kind)?;
  Ok(canonical)
}

fn canonicalize_absolute(path: &Path, description: &str) -> Result<PathBuf> {
  ensure!(
    path.is_absolute(),
    "trusted {description} must use an absolute path"
  );
  fs::canonicalize(path)
    .with_context(|| format!("canonicalize trusted {description} {}", path.display()))
}

impl TrustedPathKind {
  fn description(self) -> &'static str {
    match self {
      Self::Executable => "Caddy executable",
      Self::Config => "Caddy configuration",
    }
  }
}

#[cfg(windows)]
fn reject_windows_command_script(path: &Path) -> Result<()> {
  let is_command_script = path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| {
      extension.eq_ignore_ascii_case("bat") || extension.eq_ignore_ascii_case("cmd")
    });
  ensure!(
    !is_command_script,
    "trusted Caddy executable must be a native executable, not a batch script"
  );
  Ok(())
}

#[cfg(unix)]
mod platform {
  use super::{CaddyPathProvenance, TrustedPathKind};
  use anyhow::{Context, Result, ensure};
  use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
  };

  pub(super) fn validate_canonical_path(
    path: &Path,
    provenance: CaddyPathProvenance,
    kind: TrustedPathKind,
  ) -> Result<()> {
    validate_component(path, provenance, false)?;
    let metadata = fs::symlink_metadata(path)
      .with_context(|| format!("inspect trusted {} {}", kind.description(), path.display()))?;
    ensure!(
      metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
      "trusted {} must resolve to a regular file",
      kind.description()
    );
    if kind == TrustedPathKind::Executable {
      ensure!(
        metadata.permissions().mode() & 0o111 != 0,
        "trusted Caddy executable is not executable"
      );
    }

    for parent in path.ancestors().skip(1) {
      validate_component(parent, provenance, true)?;
    }
    Ok(())
  }

  pub(super) fn same_file_identity(left: &Path, right: &Path) -> Result<bool> {
    let left = fs::metadata(left)
      .with_context(|| format!("inspect first file identity {}", left.display()))?;
    let right = fs::metadata(right)
      .with_context(|| format!("inspect second file identity {}", right.display()))?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
  }

  pub(super) fn open_trusted_config(path: &Path) -> Result<File> {
    OpenOptions::new()
      .read(true)
      .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
      .open(path)
      .with_context(|| format!("open trusted Caddy configuration {}", path.display()))
  }

  pub(super) fn validate_config_file(
    file: &File,
    path: &Path,
    provenance: CaddyPathProvenance,
  ) -> Result<()> {
    let metadata = file
      .metadata()
      .with_context(|| format!("inspect trusted Caddy configuration {}", path.display()))?;
    ensure!(
      metadata.is_file(),
      "trusted Caddy configuration must resolve to a regular file"
    );
    validate_metadata(path, &metadata, provenance, false)?;
    for parent in path.ancestors().skip(1) {
      validate_component(parent, provenance, true)?;
    }
    Ok(())
  }

  fn validate_component(
    path: &Path,
    provenance: CaddyPathProvenance,
    directory: bool,
  ) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
      .with_context(|| format!("inspect trusted path component {}", path.display()))?;
    validate_metadata(path, &metadata, provenance, directory)
  }

  fn validate_metadata(
    path: &Path,
    metadata: &fs::Metadata,
    provenance: CaddyPathProvenance,
    directory: bool,
  ) -> Result<()> {
    ensure!(
      !metadata.file_type().is_symlink(),
      "trusted path component is a symbolic link: {}",
      path.display()
    );
    ensure!(
      !directory || metadata.is_dir(),
      "trusted path parent is not a directory: {}",
      path.display()
    );
    ensure!(
      trusted_owner(metadata.uid(), provenance),
      "trusted path component has an untrusted owner: {}",
      path.display()
    );
    ensure!(
      metadata.permissions().mode() & 0o022 == 0,
      "trusted path component is writable by a less-trusted principal: {}",
      path.display()
    );
    reject_macos_extended_acl(path)?;
    Ok(())
  }

  fn trusted_owner(owner: u32, provenance: CaddyPathProvenance) -> bool {
    owner == 0
      || provenance == CaddyPathProvenance::UserOwned
        && owner == crate::ipc_unix_security::current_euid()
  }

  #[cfg(not(target_os = "macos"))]
  fn reject_macos_extended_acl(_path: &Path) -> Result<()> {
    Ok(())
  }

  #[cfg(target_os = "macos")]
  fn reject_macos_extended_acl(path: &Path) -> Result<()> {
    use anyhow::{bail, ensure};
    use std::{
      ffi::{CString, c_char, c_int},
      os::unix::ffi::OsStrExt,
    };

    unsafe extern "C" {
      fn acl_extended_file_np(path: *const c_char) -> c_int;
    }

    let path_bytes = CString::new(path.as_os_str().as_bytes())
      .context("trusted path contains an interior NUL byte")?;
    // SAFETY: `path_bytes` is NUL-terminated and remains live for the call.
    let status = unsafe { acl_extended_file_np(path_bytes.as_ptr()) };
    if status < 0 {
      bail!(
        "inspect extended ACL on trusted path component {}: {}",
        path.display(),
        std::io::Error::last_os_error()
      );
    }
    ensure!(
      status == 0,
      "trusted path component has an extended ACL: {}",
      path.display()
    );
    Ok(())
  }
}

#[cfg(windows)]
mod platform {
  use super::{CaddyPathProvenance, TrustedPathKind};
  use anyhow::{Context, Result, bail, ensure};
  use std::{
    ffi::c_void,
    fs::File,
    mem::size_of,
    os::windows::{
      ffi::OsStrExt,
      io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{NonNull, null_mut},
  };
  use widestring::U16CStr;
  use windows_sys::{
    Win32::{
      Foundation::{
        ERROR_SUCCESS, GENERIC_ALL, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree,
      },
      Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT},
        DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, INHERIT_ONLY_ACE, IsValidSid,
        OWNER_SECURITY_INFORMATION, PSID,
      },
      Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, DELETE, FILE_APPEND_DATA,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_DELETE_CHILD,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
        FILE_WRITE_DATA, FILE_WRITE_EA, GetFileInformationByHandle, OPEN_EXISTING, READ_CONTROL,
        WRITE_DAC, WRITE_OWNER,
      },
    },
    core::PWSTR,
  };

  const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
  const ACCESS_ALLOWED_COMPOUND_ACE_TYPE: u8 = 4;
  const ACCESS_ALLOWED_OBJECT_ACE_TYPE: u8 = 5;
  const ACCESS_ALLOWED_CALLBACK_ACE_TYPE: u8 = 9;
  const ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE: u8 = 11;
  const SYSTEM_SID: &str = "S-1-5-18";
  const ADMINISTRATORS_SID: &str = "S-1-5-32-544";
  const TRUSTED_INSTALLER_SID: &str =
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

  pub(super) fn validate_canonical_path(
    path: &Path,
    provenance: CaddyPathProvenance,
    kind: TrustedPathKind,
  ) -> Result<()> {
    validate_component(path, provenance, false, false, kind)?;
    for (index, parent) in path.ancestors().skip(1).enumerate() {
      validate_component(parent, provenance, true, index == 0, kind)?;
    }
    Ok(())
  }

  pub(super) fn reject_final_reparse_point(path: &Path, kind: TrustedPathKind) -> Result<()> {
    ensure!(
      path.is_absolute(),
      "trusted {} must use an absolute path",
      kind.description()
    );
    let handle = open_path(path, true)?;
    let information = file_information(&handle)?;
    ensure!(
      information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
      "trusted {} must not be a Windows reparse point: {}",
      kind.description(),
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

  pub(super) fn open_trusted_config(path: &Path) -> Result<File> {
    let encoded = wide_path(path)?;
    // SAFETY: `encoded` is NUL-terminated. The final component is opened without following a
    // reparse point, and a successful handle is adopted exactly once below. Read-only sharing
    // prevents a later writer or deleter from opening this identity while it is parsed.
    let handle = unsafe {
      CreateFileW(
        encoded.as_ptr(),
        GENERIC_READ | READ_CONTROL | FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ,
        null_mut(),
        OPEN_EXISTING,
        FILE_FLAG_OPEN_REPARSE_POINT,
        null_mut(),
      )
    };
    if handle == INVALID_HANDLE_VALUE {
      return Err(std::io::Error::last_os_error())
        .with_context(|| format!("open trusted Caddy configuration {}", path.display()));
    }
    // SAFETY: `CreateFileW` returned a fresh owned file handle.
    Ok(unsafe { File::from_raw_handle(handle) })
  }

  pub(super) fn validate_config_file(
    file: &File,
    path: &Path,
    provenance: CaddyPathProvenance,
  ) -> Result<()> {
    let information = file_information(file)?;
    ensure!(
      information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
      "trusted Caddy configuration must not be a Windows reparse point: {}",
      path.display()
    );
    ensure!(
      information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0,
      "trusted Caddy configuration must resolve to a regular file"
    );
    validate_security(file, path, provenance, false, false)?;
    for (index, parent) in path.ancestors().skip(1).enumerate() {
      validate_component(
        parent,
        provenance,
        true,
        index == 0,
        TrustedPathKind::Config,
      )?;
    }
    Ok(())
  }

  fn validate_component(
    path: &Path,
    provenance: CaddyPathProvenance,
    directory: bool,
    inspect_children: bool,
    kind: TrustedPathKind,
  ) -> Result<()> {
    let handle = open_path(path, true)?;
    let information = file_information(&handle)?;
    ensure!(
      information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
      "trusted path component is a Windows reparse point: {}",
      path.display()
    );
    let is_directory = information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    ensure!(
      is_directory == directory,
      "trusted {} has an unexpected file type: {}",
      kind.description(),
      path.display()
    );
    validate_security(&handle, path, provenance, directory, inspect_children)
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
        READ_CONTROL | FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        null_mut(),
        OPEN_EXISTING,
        flags,
        null_mut(),
      )
    };
    if handle == INVALID_HANDLE_VALUE {
      return Err(std::io::Error::last_os_error())
        .with_context(|| format!("open trusted path component {}", path.display()));
    }
    // SAFETY: `CreateFileW` returned a fresh owned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
  }

  fn file_information(handle: &impl AsRawHandle) -> Result<BY_HANDLE_FILE_INFORMATION> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is live and `information` is a correctly sized initialized out buffer.
    if unsafe { GetFileInformationByHandle(handle.as_raw_handle(), &mut information) } == 0 {
      return Err(std::io::Error::last_os_error()).context("inspect trusted file identity");
    }
    Ok(information)
  }

  fn validate_security(
    handle: &impl AsRawHandle,
    path: &Path,
    provenance: CaddyPathProvenance,
    directory: bool,
    inspect_children: bool,
  ) -> Result<()> {
    let mut owner: PSID = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: `handle` is live and all requested output pointers reference initialized storage.
    let status = unsafe {
      GetSecurityInfo(
        handle.as_raw_handle(),
        SE_FILE_OBJECT,
        OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
        &mut owner,
        null_mut(),
        &mut dacl,
        null_mut(),
        &mut descriptor,
      )
    };
    ensure!(
      status == ERROR_SUCCESS,
      "inspect security descriptor for trusted path component {}: {}",
      path.display(),
      std::io::Error::from_raw_os_error(status as i32)
    );
    let _descriptor = LocalSecurityDescriptor::new(descriptor)?;
    ensure!(
      !owner.is_null() && !dacl.is_null(),
      "trusted path component has no owner or protected access list: {}",
      path.display()
    );

    let current_owner = crate::ipc_windows_security::current_process_sid()
      .context("resolve the runtime-owner SID for Caddy path validation")?;
    let owner = sid_to_string(owner)?;
    ensure!(
      trusted_sid(&owner, &current_owner, provenance),
      "trusted path component has an untrusted owner: {}",
      path.display()
    );
    validate_dacl(
      dacl,
      path,
      &current_owner,
      provenance,
      directory,
      inspect_children,
    )
  }

  fn validate_dacl(
    dacl: *mut ACL,
    path: &Path,
    current_owner: &str,
    provenance: CaddyPathProvenance,
    directory: bool,
    inspect_children: bool,
  ) -> Result<()> {
    let mut information = ACL_SIZE_INFORMATION::default();
    // SAFETY: `dacl` belongs to the live security descriptor and `information` is a correctly
    // sized initialized output buffer.
    if unsafe {
      GetAclInformation(
        dacl,
        (&mut information as *mut ACL_SIZE_INFORMATION).cast(),
        size_of::<ACL_SIZE_INFORMATION>() as u32,
        AclSizeInformation,
      )
    } == 0
    {
      return Err(std::io::Error::last_os_error())
        .with_context(|| format!("inspect access list for {}", path.display()));
    }

    for index in 0..information.AceCount {
      let mut raw_ace = null_mut();
      // SAFETY: `dacl` and its descriptor remain live, and `raw_ace` is a valid out pointer.
      if unsafe { GetAce(dacl, index, &mut raw_ace) } == 0 {
        return Err(std::io::Error::last_os_error())
          .with_context(|| format!("inspect access entry for {}", path.display()));
      }
      validate_ace(
        raw_ace,
        path,
        current_owner,
        provenance,
        directory,
        inspect_children,
      )?;
    }
    Ok(())
  }

  fn validate_ace(
    raw_ace: *mut c_void,
    path: &Path,
    current_owner: &str,
    provenance: CaddyPathProvenance,
    directory: bool,
    inspect_children: bool,
  ) -> Result<()> {
    ensure!(!raw_ace.is_null(), "Windows returned a null access entry");
    // SAFETY: `GetAce` returned a live ACE pointer whose fixed header is always present.
    let header = unsafe { &*raw_ace.cast::<ACE_HEADER>() };
    if u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0 || !allowed_ace_type(header.AceType) {
      return Ok(());
    }
    ensure!(
      usize::from(header.AceSize) >= size_of::<ACE_HEADER>() + size_of::<u32>(),
      "trusted path contains a truncated access entry: {}",
      path.display()
    );
    // SAFETY: The size check proves that the mask directly following the ACE header is present.
    let mask = unsafe {
      *raw_ace
        .cast::<u8>()
        .add(size_of::<ACE_HEADER>())
        .cast::<u32>()
    };
    if mask & mutation_mask(directory, inspect_children) == 0 {
      return Ok(());
    }

    if header.AceType != ACCESS_ALLOWED_ACE_TYPE {
      bail!(
        "trusted path grants mutation rights through an unsupported access entry: {}",
        path.display()
      );
    }
    ensure!(
      usize::from(header.AceSize) >= size_of::<ACCESS_ALLOWED_ACE>(),
      "trusted path contains a truncated allow entry: {}",
      path.display()
    );
    // SAFETY: The ACE type and size checks prove the standard allow-ACE layout and its SID field.
    let ace = unsafe { &*raw_ace.cast::<ACCESS_ALLOWED_ACE>() };
    let trustee = (&raw const ace.SidStart).cast_mut().cast::<c_void>();
    ensure!(
      unsafe { IsValidSid(trustee) } != 0,
      "trusted path contains an invalid trustee identity: {}",
      path.display()
    );
    let trustee = sid_to_string(trustee)?;
    ensure!(
      trusted_sid(&trustee, current_owner, provenance),
      "trusted path is mutable by less-trusted principal {trustee} with access mask 0x{mask:08x}: {}",
      path.display(),
    );
    Ok(())
  }

  fn allowed_ace_type(ace_type: u8) -> bool {
    matches!(
      ace_type,
      ACCESS_ALLOWED_ACE_TYPE
        | ACCESS_ALLOWED_COMPOUND_ACE_TYPE
        | ACCESS_ALLOWED_OBJECT_ACE_TYPE
        | ACCESS_ALLOWED_CALLBACK_ACE_TYPE
        | ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE
    )
  }

  pub(super) fn mutation_mask(directory: bool, inspect_children: bool) -> u32 {
    let replacement = DELETE | WRITE_DAC | WRITE_OWNER | GENERIC_ALL;
    if directory && !inspect_children {
      return replacement | FILE_DELETE_CHILD;
    }

    let content_mutation = replacement
      | GENERIC_WRITE
      | FILE_WRITE_DATA
      | FILE_APPEND_DATA
      | FILE_WRITE_EA
      | FILE_WRITE_ATTRIBUTES;
    if directory {
      content_mutation | FILE_DELETE_CHILD
    } else {
      content_mutation
    }
  }

  pub(super) fn trusted_sid(
    sid: &str,
    current_owner: &str,
    provenance: CaddyPathProvenance,
  ) -> bool {
    matches!(sid, SYSTEM_SID | ADMINISTRATORS_SID | TRUSTED_INSTALLER_SID)
      || provenance == CaddyPathProvenance::UserOwned && sid == current_owner
  }

  fn sid_to_string(sid: PSID) -> Result<Box<str>> {
    ensure!(!sid.is_null(), "Windows returned a null SID");
    let mut encoded: PWSTR = null_mut();
    // SAFETY: `sid` points into a live security descriptor and `encoded` is a valid out pointer.
    if unsafe { ConvertSidToStringSidW(sid, &mut encoded) } == 0 {
      return Err(std::io::Error::last_os_error()).context("format Windows SID");
    }
    let encoded = LocalWideString::new(encoded)?;
    // SAFETY: Windows returned a NUL-terminated UTF-16 string owned by `encoded`.
    let value = unsafe { U16CStr::from_ptr_str(encoded.as_ptr()) }
      .to_string()
      .context("decode Windows SID")?;
    Ok(value.into_boxed_str())
  }

  fn wide_path(path: &Path) -> Result<Vec<u16>> {
    let mut encoded: Vec<u16> = path.as_os_str().encode_wide().collect();
    ensure!(
      !encoded.contains(&0),
      "trusted Windows path contains an interior NUL byte"
    );
    encoded.push(0);
    Ok(encoded)
  }

  struct LocalSecurityDescriptor(NonNull<c_void>);

  impl LocalSecurityDescriptor {
    fn new(pointer: *mut c_void) -> Result<Self> {
      NonNull::new(pointer).map(Self).context(
        "Windows returned a null security descriptor while validating a trusted Caddy path",
      )
    }
  }

  impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) {
      // SAFETY: `GetSecurityInfo` allocated this descriptor and this guard releases it once.
      let _ = unsafe { LocalFree(self.0.as_ptr()) };
    }
  }

  struct LocalWideString(NonNull<u16>);

  impl LocalWideString {
    fn new(pointer: PWSTR) -> Result<Self> {
      NonNull::new(pointer)
        .map(Self)
        .context("Windows returned a null SID string")
    }

    fn as_ptr(&self) -> *const u16 {
      self.0.as_ptr()
    }
  }

  impl Drop for LocalWideString {
    fn drop(&mut self) {
      // SAFETY: `ConvertSidToStringSidW` allocated this string and this guard releases it once.
      let _ = unsafe { LocalFree(self.0.as_ptr().cast()) };
    }
  }
}

#[cfg(not(any(unix, windows)))]
mod platform {
  use super::{CaddyPathProvenance, TrustedPathKind};
  use anyhow::{Result, bail};
  use std::{fs::File, path::Path};

  pub(super) fn validate_canonical_path(
    _path: &Path,
    _provenance: CaddyPathProvenance,
    _kind: TrustedPathKind,
  ) -> Result<()> {
    bail!("trusted Caddy path validation is unsupported on this platform")
  }

  pub(super) fn same_file_identity(_left: &Path, _right: &Path) -> Result<bool> {
    bail!("trusted Caddy file identity is unsupported on this platform")
  }

  pub(super) fn open_trusted_config(_path: &Path) -> Result<File> {
    bail!("trusted Caddy configuration is unsupported on this platform")
  }

  pub(super) fn validate_config_file(
    _file: &File,
    _path: &Path,
    _provenance: CaddyPathProvenance,
  ) -> Result<()> {
    bail!("trusted Caddy configuration is unsupported on this platform")
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{env, fs};

  #[test]
  fn trusted_caddy_source_requires_absolute_executable_path() {
    let error =
      validate_trusted_executable(Path::new("caddy"), CaddyPathProvenance::UserOwned).unwrap_err();

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

    let error = validate_trusted_executable(&script, CaddyPathProvenance::UserOwned).unwrap_err();

    assert!(error.to_string().contains("batch script"));
  }

  #[cfg(windows)]
  #[test]
  fn trusted_caddy_source_windows_accepts_system_executable() {
    let executable = PathBuf::from(env::var_os("SystemRoot").unwrap())
      .join("System32")
      .join("where.exe");

    let canonical =
      validate_trusted_executable(&executable, CaddyPathProvenance::SystemOwned).unwrap();

    assert_eq!(canonical, fs::canonicalize(executable).unwrap());
  }

  #[cfg(windows)]
  #[test]
  fn trusted_caddy_source_windows_trusts_runtime_owner_only_for_user_provenance() {
    let current_owner = crate::ipc_windows_security::current_process_sid().unwrap();

    assert!(platform::trusted_sid(
      &current_owner,
      &current_owner,
      CaddyPathProvenance::UserOwned
    ));
    assert!(!platform::trusted_sid(
      &current_owner,
      &current_owner,
      CaddyPathProvenance::SystemOwned
    ));
  }

  #[cfg(windows)]
  #[test]
  fn trusted_caddy_source_windows_directory_mask_covers_content_mutation_rights() {
    use windows_sys::Win32::Storage::FileSystem::{
      FILE_APPEND_DATA, FILE_DELETE_CHILD, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA,
    };

    let mask = platform::mutation_mask(true, true);
    for right in [
      FILE_WRITE_DATA,
      FILE_APPEND_DATA,
      FILE_WRITE_EA,
      FILE_WRITE_ATTRIBUTES,
      FILE_DELETE_CHILD,
    ] {
      assert_eq!(
        mask & right,
        right,
        "directory mutation mask misses {right:#x}"
      );
    }
  }

  #[cfg(windows)]
  #[test]
  fn trusted_caddy_source_windows_ancestor_mask_allows_unrelated_child_creation() {
    use windows_sys::Win32::Storage::FileSystem::{FILE_DELETE_CHILD, FILE_WRITE_DATA};

    let mask = platform::mutation_mask(true, false);

    assert_eq!(mask & FILE_WRITE_DATA, 0);
    assert_eq!(mask & FILE_DELETE_CHILD, FILE_DELETE_CHILD);
  }

  #[cfg(windows)]
  #[test]
  fn trusted_caddy_source_windows_accepts_system_config() {
    let config = PathBuf::from(env::var_os("SystemRoot").unwrap()).join("win.ini");

    let trusted = validate_trusted_config(&config, CaddyPathProvenance::SystemOwned).unwrap();

    assert_eq!(trusted.canonical_path(), fs::canonicalize(config).unwrap());
    assert!(trusted.into_file().metadata().unwrap().is_file());
  }

  #[cfg(windows)]
  #[test]
  fn trusted_caddy_source_windows_rejects_untrusted_ancestor() {
    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join("caddy.exe");
    fs::write(&executable, b"fixture").unwrap();

    let error =
      validate_trusted_executable(&executable, CaddyPathProvenance::UserOwned).unwrap_err();

    assert!(
      error.to_string().contains("less-trusted principal")
        || error.to_string().contains("untrusted owner")
    );
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

    let error = validate_trusted_executable(&alias, CaddyPathProvenance::UserOwned).unwrap_err();

    assert!(error.to_string().contains("reparse point"));
  }

  #[cfg(unix)]
  fn trusted_unix_fixture() -> (tempfile::TempDir, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let home = PathBuf::from(env::var_os("HOME").expect("Unix test requires HOME"));
    let temp = tempfile::Builder::new()
      .prefix("cadder-path-trust-")
      .tempdir_in(home)
      .unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let executable = temp.path().join("caddy");
    fs::write(&executable, b"fixture").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    (temp, executable)
  }

  #[cfg(unix)]
  #[test]
  fn trusted_caddy_source_unix_accepts_owner_executable() {
    let (_temp, executable) = trusted_unix_fixture();

    let canonical =
      validate_trusted_executable(&executable, CaddyPathProvenance::UserOwned).unwrap();

    assert_eq!(canonical, fs::canonicalize(executable).unwrap());
  }

  #[cfg(unix)]
  #[test]
  fn trusted_caddy_source_unix_rejects_non_executable_file() {
    use std::os::unix::fs::PermissionsExt;

    let (_temp, executable) = trusted_unix_fixture();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o600)).unwrap();

    let error =
      validate_trusted_executable(&executable, CaddyPathProvenance::UserOwned).unwrap_err();

    assert!(error.to_string().contains("not executable"));
  }

  #[cfg(unix)]
  #[test]
  fn trusted_caddy_source_unix_rejects_writable_ancestor() {
    use std::os::unix::fs::PermissionsExt;

    let (temp, executable) = trusted_unix_fixture();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o770)).unwrap();

    let error =
      validate_trusted_executable(&executable, CaddyPathProvenance::UserOwned).unwrap_err();

    assert!(error.to_string().contains("less-trusted principal"));
  }

  #[cfg(unix)]
  #[test]
  fn trusted_caddy_source_unix_same_file_identity_follows_symlink() {
    use std::os::unix::fs::symlink;

    let (_temp, executable) = trusted_unix_fixture();
    let alias = executable.with_file_name("caddy-alias");
    symlink(&executable, &alias).unwrap();

    assert!(same_file_identity(&executable, &alias).unwrap());
  }
}
