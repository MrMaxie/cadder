//! Windows ownership and peer-authentication primitives for local IPC.

use std::{
  ffi::c_void,
  fs::File,
  io,
  mem::size_of,
  os::windows::ffi::OsStrExt,
  os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
  path::Path,
  process,
  ptr::{NonNull, null_mut},
  time::Duration,
};

use interprocess::{
  local_socket::{ListenerOptions, tokio::Stream},
  os::windows::{local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor},
};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  time::timeout,
};
use widestring::{U16CStr, U16CString};
use windows_sys::{
  Win32::{
    Foundation::{
      ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
      LocalFree,
    },
    Security::{
      ACL,
      Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
      },
      DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetTokenInformation,
      OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSID, RevertToSelf,
      SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
      BY_HANDLE_FILE_INFORMATION, CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
      FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
      FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
      FlushFileBuffers, GetFileInformationByHandle, GetFullPathNameW, MOVEFILE_WRITE_THROUGH,
      MoveFileExW, OPEN_EXISTING, READ_CONTROL, ReplaceFileW, WRITE_DAC,
    },
    System::{
      Pipes::ImpersonateNamedPipeClient,
      Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
    },
  },
  core::PWSTR,
};

#[cfg(test)]
use windows_sys::Win32::Security::SetFileSecurityW;

const AUTHENTICATION_PREFACE: [u8; 1] = [b' '];
const AUTHENTICATION_PREFACE_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const FORCE_REVERT_FAILURE_MARKER: &str = "CADDER_TEST_FORCE_REVERT_FAILURE_MARKER";

/// Returns the canonical SID of the process token that owns the daemon runtime.
///
/// # Errors
///
/// Returns an error when Windows does not expose the process token or its user SID.
pub(crate) fn current_process_sid() -> io::Result<Box<str>> {
  let token = open_process_token()?;
  token_sid(&token)
}

/// Returns the canonical SID of the named-pipe client whose preface was just read.
///
/// The caller must invoke [`receive_authentication_preface`] on this stream immediately before
/// this function. No await point may occur between the two calls.
///
/// # Errors
///
/// Returns an error when the stream is not backed by a Windows named pipe, impersonation fails,
/// or Windows does not expose the client's user SID.
pub(crate) fn peer_sid_after_preface(stream: &Stream) -> io::Result<Box<str>> {
  with_impersonated_peer(stream, || {
    let token = open_thread_token()?;
    token_sid(&token)
  })
}

fn with_impersonated_peer<T>(
  stream: &Stream,
  operation: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
  let Stream::NamedPipe(pipe) = stream;
  let handle = pipe.as_handle().as_raw_handle();

  let impersonation = ImpersonationGuard::begin(handle)?;
  let result = operation();
  impersonation.finish();
  result
}

/// Writes the fixed transport authentication preface within five seconds.
///
/// # Errors
///
/// Returns an I/O error when the preface cannot be written and [`io::ErrorKind::TimedOut`] when
/// the operation exceeds its deadline.
pub(crate) async fn send_authentication_preface(stream: &mut Stream) -> io::Result<()> {
  timeout(AUTHENTICATION_PREFACE_TIMEOUT, async {
    stream.write_all(&AUTHENTICATION_PREFACE).await?;
    stream.flush().await
  })
  .await
  .map_err(|_| authentication_preface_timeout("send", AUTHENTICATION_PREFACE_TIMEOUT))?
}

/// Reads and validates the fixed transport authentication preface within five seconds.
///
/// # Errors
///
/// Returns an I/O error when the preface cannot be read, [`io::ErrorKind::InvalidData`] when its
/// byte is invalid, and [`io::ErrorKind::TimedOut`] when the operation exceeds its deadline.
pub(crate) async fn receive_authentication_preface(stream: &mut Stream) -> io::Result<()> {
  receive_authentication_preface_with_timeout(stream, AUTHENTICATION_PREFACE_TIMEOUT).await
}

async fn receive_authentication_preface_with_timeout(
  stream: &mut Stream,
  deadline: Duration,
) -> io::Result<()> {
  let mut preface = [0_u8; AUTHENTICATION_PREFACE.len()];
  timeout(deadline, stream.read_exact(&mut preface))
    .await
    .map_err(|_| authentication_preface_timeout("receive", deadline))??;

  if preface != AUTHENTICATION_PREFACE {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "invalid IPC authentication preface",
    ));
  }
  Ok(())
}

/// Restricts the named-pipe listener to the supplied runtime-owner SID.
///
/// # Errors
///
/// Returns an error when `owner_sid` is not a canonical SID string or Windows cannot construct
/// the protected owner-only security descriptor.
pub(crate) fn secure_listener_options<'a>(
  options: ListenerOptions<'a>,
  owner_sid: &str,
) -> io::Result<ListenerOptions<'a>> {
  if !is_canonical_sid(owner_sid) {
    return Err(io::Error::new(
      io::ErrorKind::InvalidInput,
      "runtime owner SID is not canonical",
    ));
  }

  let sddl = U16CString::from_str(format!("O:{owner_sid}D:P(A;;GA;;;{owner_sid})"))
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
  let descriptor = SecurityDescriptor::deserialize(sddl.as_ucstr())?;
  Ok(options.security_descriptor(descriptor))
}

/// Creates a new regular runtime file with a protected owner-only DACL.
pub(crate) fn create_owner_only_runtime_file(path: &Path) -> io::Result<File> {
  let descriptor = owner_only_security_descriptor()?;
  let security_attributes = SECURITY_ATTRIBUTES {
    nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
    lpSecurityDescriptor: descriptor.as_ptr(),
    bInheritHandle: 0,
  };
  let path = wide_path(path)?;
  // SAFETY: `path` is NUL-terminated, `security_attributes` and its descriptor remain live for
  // the call, `CREATE_NEW` prevents replacement, and the returned handle is adopted exactly once.
  let handle = unsafe {
    CreateFileW(
      path.as_ptr(),
      GENERIC_READ | GENERIC_WRITE,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      &security_attributes,
      CREATE_NEW,
      FILE_ATTRIBUTE_NORMAL,
      null_mut(),
    )
  };
  if handle == INVALID_HANDLE_VALUE {
    return Err(io::Error::last_os_error());
  }
  // SAFETY: `CreateFileW` returned a fresh owned file handle and `File` closes it exactly once.
  Ok(unsafe { File::from_raw_handle(handle) })
}

/// Opens or creates the persistent owner-only discovery lock file.
pub(crate) fn open_owner_only_lock_file(path: &Path) -> io::Result<File> {
  match create_owner_only_runtime_file(path) {
    Ok(file) => Ok(file),
    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
      open_owner_only_runtime_file(path)
    }
    Err(error) => Err(error),
  }
}

/// Rejects reparse points and non-files before applying the owner-only DACL.
pub(crate) fn validate_owner_only_runtime_file(path: &Path) -> io::Result<()> {
  open_owner_only_runtime_file(path).map(drop)
}

/// Opens the runtime directory without following reparse points and applies owner-only security
/// through that verified handle.
pub(crate) fn secure_owner_only_runtime_directory(path: &Path) -> io::Result<()> {
  let path = wide_path(path)?;
  // SAFETY: `path` is NUL-terminated. Backup semantics permits a directory handle and
  // `OPEN_REPARSE_POINT` ensures a junction or symbolic link itself is inspected, not followed.
  let handle = unsafe {
    CreateFileW(
      path.as_ptr(),
      READ_CONTROL | WRITE_DAC,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      std::ptr::null(),
      OPEN_EXISTING,
      FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
      null_mut(),
    )
  };
  if handle == INVALID_HANDLE_VALUE {
    return Err(io::Error::last_os_error());
  }
  // SAFETY: `CreateFileW` returned a fresh owned directory handle.
  let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
  validate_runtime_directory_handle(&handle)?;
  validate_current_owner(handle.as_raw_handle())?;
  apply_owner_only_security(handle.as_raw_handle())?;
  validate_runtime_directory_handle(&handle)
}

/// Applies the current process owner's protected owner-only DACL to a path.
pub(crate) fn secure_owner_only_path(path: &Path) -> io::Result<()> {
  open_owner_only_runtime_file(path).map(drop)
}

pub(crate) fn open_owner_only_runtime_file(path: &Path) -> io::Result<File> {
  let path = wide_path(path)?;
  // SAFETY: `path` is NUL-terminated. `OPEN_REPARSE_POINT` ensures the final component itself is
  // inspected, and the returned handle is adopted exactly once.
  let handle = unsafe {
    CreateFileW(
      path.as_ptr(),
      GENERIC_READ | GENERIC_WRITE | READ_CONTROL | WRITE_DAC,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      std::ptr::null(),
      OPEN_EXISTING,
      FILE_FLAG_OPEN_REPARSE_POINT,
      null_mut(),
    )
  };
  if handle == INVALID_HANDLE_VALUE {
    return Err(io::Error::last_os_error());
  }
  // SAFETY: `CreateFileW` returned a fresh owned file handle.
  let file = unsafe { File::from_raw_handle(handle) };
  validate_runtime_file_handle(&file)?;
  validate_current_owner(file.as_raw_handle())?;
  apply_owner_only_security(file.as_raw_handle())?;
  validate_runtime_file_handle(&file)?;
  Ok(file)
}

/// Flushes a discovery file through the Windows file handle.
pub(crate) fn flush_file(file: &File) -> io::Result<()> {
  // SAFETY: The raw handle is borrowed from a live file and remains valid for the call.
  if unsafe { FlushFileBuffers(file.as_raw_handle()) } == 0 {
    return Err(io::Error::last_os_error());
  }
  Ok(())
}

/// Atomically installs a complete discovery file.
pub(crate) fn install_discovery_file(temporary: &Path, destination: &Path) -> io::Result<()> {
  let temporary_wide = wide_path(temporary)?;
  let destination_wide = wide_path(destination)?;
  let replaced = if destination.exists() {
    validate_owner_only_runtime_file(destination)?;
    // SAFETY: Both paths are NUL-terminated and point to files on the same runtime volume. No
    // backup, exclusion list, or reserved flags are supplied.
    unsafe {
      ReplaceFileW(
        destination_wide.as_ptr(),
        temporary_wide.as_ptr(),
        std::ptr::null(),
        0,
        std::ptr::null(),
        std::ptr::null(),
      )
    }
  } else {
    // SAFETY: Both paths are NUL-terminated. The write-through move is the first publication and
    // does not replace an existing destination.
    unsafe {
      MoveFileExW(
        temporary_wide.as_ptr(),
        destination_wide.as_ptr(),
        MOVEFILE_WRITE_THROUGH,
      )
    }
  };
  if replaced == 0 {
    return Err(io::Error::last_os_error());
  }
  Ok(())
}

fn owner_only_security_descriptor() -> io::Result<OwnedSecurityDescriptor> {
  let owner_sid = current_process_sid()?;
  security_descriptor_from_sddl(&format!("O:{owner_sid}D:P(A;;GA;;;{owner_sid})"))
}

fn security_descriptor_from_sddl(sddl: &str) -> io::Result<OwnedSecurityDescriptor> {
  let sddl = U16CString::from_str(sddl)
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
  let mut descriptor = null_mut();
  // SAFETY: The SDDL is NUL-terminated, the output pointer is valid, and Windows allocates the
  // returned self-relative descriptor with `LocalAlloc` semantics.
  if unsafe {
    ConvertStringSecurityDescriptorToSecurityDescriptorW(
      sddl.as_ptr(),
      SDDL_REVISION_1,
      &mut descriptor,
      null_mut(),
    )
  } == 0
  {
    return Err(io::Error::last_os_error());
  }
  OwnedSecurityDescriptor::new(descriptor)
}

#[cfg(test)]
fn apply_world_access_for_test(path: &Path) -> io::Result<()> {
  let descriptor = security_descriptor_from_sddl("D:P(A;;GA;;;WD)")?;
  let path = wide_path(path)?;
  // SAFETY: The test path is NUL-terminated and the descriptor remains live for the call.
  if unsafe {
    SetFileSecurityW(
      path.as_ptr(),
      DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
      descriptor.as_ptr(),
    )
  } == 0
  {
    return Err(io::Error::last_os_error());
  }
  Ok(())
}

fn validate_runtime_directory_handle(handle: &OwnedHandle) -> io::Result<()> {
  let mut information = BY_HANDLE_FILE_INFORMATION::default();
  // SAFETY: The handle remains live and the output pointer references the exact documented
  // structure for `GetFileInformationByHandle`.
  if unsafe { GetFileInformationByHandle(handle.as_raw_handle(), &mut information) } == 0 {
    return Err(io::Error::last_os_error());
  }
  let attributes = information.dwFileAttributes;
  if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
    return Err(io::Error::new(
      io::ErrorKind::PermissionDenied,
      "Cadder refuses to use a non-directory or reparse-point runtime path",
    ));
  }
  Ok(())
}

fn validate_runtime_file_handle(file: &File) -> io::Result<()> {
  let mut information = BY_HANDLE_FILE_INFORMATION::default();
  // SAFETY: The file handle remains live and the output points to the documented structure.
  if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
    return Err(io::Error::last_os_error());
  }
  let attributes = information.dwFileAttributes;
  if attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
    return Err(io::Error::new(
      io::ErrorKind::PermissionDenied,
      "Cadder refuses to use a non-regular or reparse-point runtime file",
    ));
  }
  Ok(())
}

fn validate_current_owner(handle: std::os::windows::io::RawHandle) -> io::Result<()> {
  let actual = handle_owner_sid(handle)?;
  let expected = current_process_sid()?;
  validate_owner_sid(&actual, &expected)
}

fn validate_owner_sid(actual: &str, expected: &str) -> io::Result<()> {
  if actual == expected {
    return Ok(());
  }
  Err(io::Error::new(
    io::ErrorKind::PermissionDenied,
    "Cadder refuses to use a runtime artifact owned by another Windows account",
  ))
}

fn handle_owner_sid(handle: std::os::windows::io::RawHandle) -> io::Result<Box<str>> {
  let mut owner: PSID = null_mut();
  let mut descriptor = null_mut();
  // SAFETY: The handle remains live, output pointers are valid, and Windows allocates the
  // returned descriptor with `LocalAlloc` semantics.
  let status = unsafe {
    GetSecurityInfo(
      handle,
      SE_FILE_OBJECT,
      OWNER_SECURITY_INFORMATION,
      &mut owner,
      null_mut(),
      null_mut(),
      null_mut(),
      &mut descriptor,
    )
  };
  if status != ERROR_SUCCESS {
    return Err(io::Error::from_raw_os_error(status as i32));
  }
  let _descriptor = OwnedSecurityDescriptor::new(descriptor)?;
  sid_to_string(owner)
}

fn apply_owner_only_security(handle: std::os::windows::io::RawHandle) -> io::Result<()> {
  let descriptor = owner_only_security_descriptor()?;
  let mut dacl: *mut ACL = null_mut();
  let mut dacl_present = 0;
  let mut dacl_defaulted = 0;
  // SAFETY: The descriptor remains live and every out pointer references initialized storage.
  if unsafe {
    GetSecurityDescriptorDacl(
      descriptor.as_ptr(),
      &mut dacl_present,
      &mut dacl,
      &mut dacl_defaulted,
    )
  } == 0
    || dacl_present == 0
    || dacl.is_null()
  {
    return Err(io::Error::last_os_error());
  }
  // SAFETY: The verified owner-controlled handle has `WRITE_DAC`, and the DACL points into the
  // live descriptor for the duration of the call. Ownership is deliberately not changed.
  let status = unsafe {
    SetSecurityInfo(
      handle,
      SE_FILE_OBJECT,
      DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
      null_mut(),
      null_mut(),
      dacl,
      std::ptr::null(),
    )
  };
  if status != ERROR_SUCCESS {
    return Err(io::Error::from_raw_os_error(status as i32));
  }
  Ok(())
}

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
  let encoded = normalized_absolute_path(path)?;
  const SEPARATOR: u16 = b'\\' as u16;
  const QUESTION_MARK: u16 = b'?' as u16;
  let verbatim_prefix = [SEPARATOR, SEPARATOR, QUESTION_MARK, SEPARATOR];
  let unc_prefix = [SEPARATOR, SEPARATOR];
  let mut result = if encoded.starts_with(&verbatim_prefix) {
    encoded
  } else if encoded.starts_with(&unc_prefix) {
    let mut path = "\\\\?\\UNC\\".encode_utf16().collect::<Vec<_>>();
    path.extend_from_slice(&encoded[unc_prefix.len()..]);
    path
  } else {
    let mut path = verbatim_prefix.to_vec();
    path.extend_from_slice(&encoded);
    path
  };
  result.push(0);
  Ok(result)
}

fn normalized_absolute_path(path: &Path) -> io::Result<Vec<u16>> {
  let mut input: Vec<u16> = path.as_os_str().encode_wide().collect();
  if input.contains(&0) {
    return Err(io::Error::new(
      io::ErrorKind::InvalidInput,
      "Windows runtime path contains a NUL character",
    ));
  }
  input.push(0);
  // SAFETY: `input` is NUL-terminated. A zero-length output query returns the required size.
  let required = unsafe { GetFullPathNameW(input.as_ptr(), 0, null_mut(), null_mut()) };
  if required == 0 {
    return Err(io::Error::last_os_error());
  }
  let mut output = vec![0_u16; required as usize];
  // SAFETY: Both buffers remain live, and the output capacity is the size returned above.
  let written = unsafe {
    GetFullPathNameW(
      input.as_ptr(),
      output.len() as u32,
      output.as_mut_ptr(),
      null_mut(),
    )
  };
  if written == 0 {
    return Err(io::Error::last_os_error());
  }
  if written as usize >= output.len() {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "Windows runtime path changed while it was normalized",
    ));
  }
  output.truncate(written as usize);
  Ok(output)
}

struct OwnedSecurityDescriptor(NonNull<c_void>);

impl OwnedSecurityDescriptor {
  fn new(pointer: *mut c_void) -> io::Result<Self> {
    NonNull::new(pointer).map(Self).ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::InvalidData,
        "Windows returned a null security descriptor",
      )
    })
  }

  fn as_ptr(&self) -> *mut c_void {
    self.0.as_ptr()
  }
}

impl Drop for OwnedSecurityDescriptor {
  fn drop(&mut self) {
    // SAFETY: The descriptor was allocated by the SDDL conversion API and remains owned here.
    let _ = unsafe { LocalFree(self.0.as_ptr()) };
  }
}

fn open_process_token() -> io::Result<OwnedHandle> {
  let mut token = null_mut();
  // SAFETY: `token` is a valid out pointer. `GetCurrentProcess` returns a process pseudo-handle
  // that remains valid for this call, and `TOKEN_QUERY` requests read-only token access.
  let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
  owned_handle_from_win32(opened, token)
}

fn open_thread_token() -> io::Result<OwnedHandle> {
  let mut token = null_mut();
  // SAFETY: `token` is a valid out pointer. The current thread is synchronously impersonating the
  // named-pipe client, and no await point can move this call to a different thread.
  let opened = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) };
  owned_handle_from_win32(opened, token)
}

fn owned_handle_from_win32(
  opened: i32,
  handle: windows_sys::Win32::Foundation::HANDLE,
) -> io::Result<OwnedHandle> {
  if opened == 0 {
    return Err(io::Error::last_os_error());
  }
  if handle.is_null() {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "Windows returned a null token handle after a successful call",
    ));
  }

  // SAFETY: A successful token-opening call returns a fresh owned handle. `OwnedHandle` closes it
  // exactly once when the value is dropped.
  Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn token_sid(token: &OwnedHandle) -> io::Result<Box<str>> {
  let token = token.as_raw_handle();
  let mut required_bytes = 0_u32;
  // SAFETY: This documented sizing call intentionally supplies no output buffer and provides a
  // valid pointer for Windows to report the required byte count.
  let sized = unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut required_bytes) };
  if sized == 0 {
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
      return Err(error);
    }
  }
  if required_bytes < size_of::<TOKEN_USER>() as u32 {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "Windows returned an invalid token-user buffer length",
    ));
  }

  let word_size = size_of::<usize>();
  let word_count = (required_bytes as usize).div_ceil(word_size);
  let mut buffer = vec![0_usize; word_count];
  let mut returned_bytes = required_bytes;
  // SAFETY: The `usize` allocation is suitably aligned for `TOKEN_USER`, has at least the size
  // reported by the sizing call, and remains live while the returned SID pointer is consumed.
  let read = unsafe {
    GetTokenInformation(
      token,
      TokenUser,
      buffer.as_mut_ptr().cast(),
      required_bytes,
      &mut returned_bytes,
    )
  };
  if read == 0 {
    return Err(io::Error::last_os_error());
  }
  if returned_bytes < size_of::<TOKEN_USER>() as u32 {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "Windows returned incomplete token-user data",
    ));
  }

  // SAFETY: `GetTokenInformation(TokenUser)` initialized the aligned buffer as a `TOKEN_USER`,
  // and the size checks above ensure that its fixed header is present.
  let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
  sid_to_string(sid)
}

fn sid_to_string(sid: PSID) -> io::Result<Box<str>> {
  if sid.is_null() {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "Windows returned a null user SID",
    ));
  }

  let mut string_sid: PWSTR = null_mut();
  // SAFETY: `sid` points into live Windows-managed token or security-descriptor storage, and
  // `string_sid` is a valid out pointer. On success Windows allocates a NUL-terminated string
  // with `LocalAlloc` semantics.
  if unsafe { ConvertSidToStringSidW(sid, &mut string_sid) } == 0 {
    return Err(io::Error::last_os_error());
  }
  let string_sid = LocalWideString::new(string_sid)?;
  // SAFETY: `ConvertSidToStringSidW` guarantees a valid NUL-terminated UTF-16 allocation for the
  // lifetime of `string_sid`.
  let string = unsafe { U16CStr::from_ptr_str(string_sid.as_ptr()) }
    .to_string()
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
  Ok(string.into_boxed_str())
}

fn authentication_preface_timeout(direction: &str, deadline: Duration) -> io::Error {
  io::Error::new(
    io::ErrorKind::TimedOut,
    format!("IPC authentication preface {direction} timed out after {deadline:?}"),
  )
}

fn is_canonical_sid(sid: &str) -> bool {
  sid.strip_prefix("S-").is_some_and(|components| {
    components.split('-').count() >= 2
      && components.split('-').all(|component| {
        !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
      })
  })
}

struct ImpersonationGuard {
  active: bool,
}

impl ImpersonationGuard {
  fn begin(handle: std::os::windows::io::RawHandle) -> io::Result<Self> {
    // SAFETY: The handle is borrowed from the live local-socket stream and identifies the server
    // end of the named pipe whose client preface was just read on this thread.
    if unsafe { ImpersonateNamedPipeClient(handle) } == 0 {
      return Err(io::Error::last_os_error());
    }
    Ok(Self { active: true })
  }

  fn finish(mut self) {
    self.revert_or_abort();
  }

  fn revert_or_abort(&mut self) {
    if !self.active {
      return;
    }
    self.active = false;
    // SAFETY: This guard is created only after successful impersonation on the current thread and
    // is reverted synchronously on that same thread without an intervening await point.
    if revert_current_thread() == 0 {
      // Continuing under an untrusted client's identity would violate the daemon trust boundary.
      process::abort();
    }
  }
}

fn revert_current_thread() -> i32 {
  #[cfg(test)]
  if let Some(marker) = std::env::var_os(FORCE_REVERT_FAILURE_MARKER) {
    let _ = std::fs::write(marker, b"revert failure injected");
    return 0;
  }

  // SAFETY: The caller invokes this only for a guard created by successful impersonation on the
  // current thread, synchronously and without an intervening await point.
  unsafe { RevertToSelf() }
}

impl Drop for ImpersonationGuard {
  fn drop(&mut self) {
    self.revert_or_abort();
  }
}

struct LocalWideString(NonNull<u16>);

impl LocalWideString {
  fn new(pointer: PWSTR) -> io::Result<Self> {
    NonNull::new(pointer).map(Self).ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::InvalidData,
        "Windows returned a null SID string",
      )
    })
  }

  fn as_ptr(&self) -> *const u16 {
    self.0.as_ptr()
  }
}

impl Drop for LocalWideString {
  fn drop(&mut self) {
    // SAFETY: The pointer was allocated by `ConvertSidToStringSidW`, is still owned by this guard,
    // and is released exactly once through the matching `LocalFree` API.
    let _ = unsafe { LocalFree(self.0.as_ptr().cast()) };
  }
}

#[cfg(test)]
mod tests {
  use std::{
    ffi::c_void,
    fs, io,
    mem::size_of,
    os::windows::io::{AsHandle, AsRawHandle},
    path::Path,
    ptr::{NonNull, addr_of, null_mut},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
  };

  use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions, ToNsName,
    tokio::{Stream, prelude::*},
  };
  use tokio::{io::AsyncWriteExt, process::Command};
  use windows_sys::Win32::{
    Foundation::{ERROR_NO_TOKEN, ERROR_SUCCESS, LocalFree},
    Security::{
      ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
      Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT},
      DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, GetLengthSid,
      GetSecurityDescriptorControl, IsValidSid, OWNER_SECURITY_INFORMATION, PSID,
      SE_DACL_PROTECTED,
    },
    System::SystemServices::ACCESS_ALLOWED_ACE_TYPE,
  };

  use super::{
    FORCE_REVERT_FAILURE_MARKER, apply_world_access_for_test, create_owner_only_runtime_file,
    current_process_sid, is_canonical_sid, open_thread_token, peer_sid_after_preface,
    receive_authentication_preface, receive_authentication_preface_with_timeout,
    secure_listener_options, secure_owner_only_path, secure_owner_only_runtime_directory,
    send_authentication_preface, sid_to_string, validate_owner_sid, wide_path,
    with_impersonated_peer,
  };
  use crate::{IpcEndpointMetadata, IpcEndpointPublication, RuntimePaths};

  // Named pipes use file-object generic mapping, so SDDL `GA` materializes as `FILE_ALL_ACCESS`.
  const NAMED_PIPE_ALL_ACCESS: u32 = 0x001F_01FF;
  static SOCKET_SEQUENCE: AtomicU64 = AtomicU64::new(0);

  #[test]
  fn windows_ipc_security_accepts_canonical_sid_text() {
    assert!(is_canonical_sid("S-1-5-21-1000-1001-1002-1003"));
  }

  #[test]
  fn windows_ipc_security_rejects_sddl_injection() {
    assert!(!is_canonical_sid("S-1-5-21-1)(A;;GA;;;WD"));
  }

  #[test]
  fn windows_runtime_security_rejects_a_different_owner_sid() {
    let error = validate_owner_sid("S-1-5-21-1", "S-1-5-21-2").unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
  }

  #[test]
  fn windows_runtime_paths_normalize_relative_components_before_verbatim_prefixing() {
    let path = Path::new(".")
      .join("target")
      .join("..")
      .join("runtime")
      .join("cadder-ipc.json");
    let encoded = wide_path(&path).unwrap();
    let normalized = String::from_utf16(&encoded[..encoded.len() - 1]).unwrap();

    assert!(normalized.starts_with(r"\\?\"));
    assert!(!normalized.contains(r"\.\"));
    assert!(!normalized.contains(r"\..\"));
  }

  #[tokio::test]
  async fn windows_ipc_security_authenticates_owner_on_owner_only_pipe() {
    let owner_sid = current_process_sid().unwrap();
    let (mut server, mut client) = connected_streams(Some(&owner_sid)).await;
    send_authentication_preface(&mut client).await.unwrap();
    receive_authentication_preface(&mut server).await.unwrap();

    assert_eq!(peer_sid_after_preface(&server).unwrap(), owner_sid);
    assert_current_thread_is_not_impersonating();
  }

  #[tokio::test]
  async fn windows_ipc_security_applies_a_protected_owner_only_dacl() {
    let owner_sid = current_process_sid().unwrap();
    let (server, _client) = connected_streams(Some(&owner_sid)).await;

    let evidence = inspect_pipe_security(&server).unwrap();

    assert!(evidence.dacl_protected);
    assert_eq!(evidence.owner_sid, owner_sid);
    assert_eq!(evidence.ace_flags, 0);
    assert_eq!(evidence.access_mask, NAMED_PIPE_ALL_ACCESS);
    assert_eq!(evidence.trustee_sid, owner_sid);
  }

  #[test]
  fn discovery_publication_windows_files_use_a_protected_owner_only_dacl() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cadder-ipc.json");
    let file = create_owner_only_runtime_file(&path).unwrap();
    secure_owner_only_path(&path).unwrap();

    let evidence = inspect_handle_security(file.as_raw_handle()).unwrap();
    let owner_sid = current_process_sid().unwrap();

    assert!(evidence.dacl_protected);
    assert_eq!(evidence.owner_sid, owner_sid);
    assert_eq!(evidence.ace_flags, 0);
    assert_eq!(evidence.access_mask, NAMED_PIPE_ALL_ACCESS);
    assert_eq!(evidence.trustee_sid, owner_sid);
  }

  #[test]
  fn discovery_publication_windows_final_target_replaces_a_wider_legacy_dacl() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let first = IpcEndpointMetadata::new(&paths).unwrap();
    let first_guard = IpcEndpointPublication::publish(&paths, &first).unwrap();
    let first_file = fs::File::open(paths.ipc_endpoint_path()).unwrap();
    assert_owner_only_security(inspect_handle_security(first_file.as_raw_handle()).unwrap());
    drop(first_file);
    apply_world_access_for_test(&paths.ipc_endpoint_path()).unwrap();
    let second = IpcEndpointMetadata::new(&paths).unwrap();
    let mut second_guard = IpcEndpointPublication::publish(&paths, &second).unwrap();
    let second_file = fs::File::open(paths.ipc_endpoint_path()).unwrap();

    assert_owner_only_security(inspect_handle_security(second_file.as_raw_handle()).unwrap());
    drop(second_file);
    drop(first_guard);
    second_guard.cleanup().unwrap();
  }

  #[test]
  fn discovery_publication_windows_rejects_a_reparse_point_runtime_directory() {
    use std::os::windows::fs::symlink_dir;

    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("runtime-link");
    fs::create_dir(&target).unwrap();
    symlink_dir(&target, &link).unwrap();

    let error = secure_owner_only_runtime_directory(&link).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
  }

  #[tokio::test]
  async fn windows_ipc_security_reverts_after_an_impersonated_operation_fails() {
    let (mut server, mut client) = connected_streams(None).await;
    send_authentication_preface(&mut client).await.unwrap();
    receive_authentication_preface(&mut server).await.unwrap();

    let error = with_impersonated_peer::<()>(&server, || {
      Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "forced identity-capture failure",
      ))
    })
    .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_current_thread_is_not_impersonating();
  }

  #[tokio::test]
  async fn windows_ipc_security_aborts_when_revert_to_self_fails() {
    if std::env::var_os(FORCE_REVERT_FAILURE_MARKER).is_some() {
      exercise_forced_revert_failure().await;
      panic!("forced RevertToSelf failure did not abort the process");
    }

    let sequence = SOCKET_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let marker = std::env::temp_dir().join(format!(
      "cadder-revert-failure-{}-{sequence}.marker",
      std::process::id()
    ));
    let _ = fs::remove_file(&marker);
    let output = Command::new(std::env::current_exe().unwrap())
      .arg("--exact")
      .arg("ipc_windows_security::tests::windows_ipc_security_aborts_when_revert_to_self_fails")
      .arg("--nocapture")
      .env(FORCE_REVERT_FAILURE_MARKER, &marker)
      .output()
      .await
      .unwrap();
    let marker_contents = fs::read_to_string(&marker).unwrap();
    fs::remove_file(marker).unwrap();

    assert_eq!(marker_contents, "revert failure injected");
    assert!(!output.status.success());
  }

  #[tokio::test]
  async fn windows_ipc_security_rejects_an_unexpected_preface_byte() {
    let (mut server, mut client) = connected_streams(None).await;
    client.write_all(b"!").await.unwrap();

    let error = receive_authentication_preface(&mut server)
      .await
      .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
  }

  #[tokio::test]
  async fn windows_ipc_security_rejects_eof_before_the_preface() {
    let (mut server, client) = connected_streams(None).await;
    drop(client);

    let error = receive_authentication_preface(&mut server)
      .await
      .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
  }

  #[tokio::test]
  async fn windows_ipc_security_bounds_the_preface_deadline() {
    let (mut server, _client) = connected_streams(None).await;

    let error = receive_authentication_preface_with_timeout(&mut server, Duration::from_millis(10))
      .await
      .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
  }

  async fn connected_streams(owner_sid: Option<&str>) -> (Stream, Stream) {
    let sequence = SOCKET_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let socket = format!(
      "cadder-ipc-security-check-{}-{sequence}",
      std::process::id()
    );
    let listener_name = socket.clone().to_ns_name::<GenericNamespaced>().unwrap();
    let client_name = socket.to_ns_name::<GenericNamespaced>().unwrap();
    let options = ListenerOptions::new()
      .name(listener_name)
      .try_overwrite(true);
    let options = match owner_sid {
      Some(sid) => secure_listener_options(options, sid),
      None => Ok(options),
    };
    let listener = options.unwrap().create_tokio().unwrap();
    let accepted = tokio::spawn(async move { listener.accept().await.unwrap() });
    let client = Stream::connect(client_name).await.unwrap();
    let server = accepted.await.unwrap();
    (server, client)
  }

  async fn exercise_forced_revert_failure() {
    let (mut server, mut client) = connected_streams(None).await;
    send_authentication_preface(&mut client).await.unwrap();
    receive_authentication_preface(&mut server).await.unwrap();
    let _ = peer_sid_after_preface(&server);
  }

  fn inspect_pipe_security(stream: &Stream) -> io::Result<PipeSecurityEvidence> {
    let Stream::NamedPipe(pipe) = stream;
    inspect_handle_security(pipe.as_handle().as_raw_handle())
  }

  fn inspect_handle_security(
    handle: std::os::windows::io::RawHandle,
  ) -> io::Result<PipeSecurityEvidence> {
    let mut owner_sid: PSID = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: The pipe handle is borrowed from a live accepted stream. All out pointers are valid,
    // and the returned descriptor is owned by `LocalSecurityDescriptor` for the full inspection.
    let status = unsafe {
      GetSecurityInfo(
        handle,
        SE_KERNEL_OBJECT,
        OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
        &mut owner_sid,
        null_mut(),
        &mut dacl,
        null_mut(),
        &mut descriptor,
      )
    };
    if status != ERROR_SUCCESS {
      return Err(io::Error::from_raw_os_error(status as i32));
    }
    let descriptor = LocalSecurityDescriptor::new(descriptor)?;
    if owner_sid.is_null() || dacl.is_null() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "named-pipe security descriptor omitted its owner or DACL",
      ));
    }

    let mut control = 0_u16;
    let mut revision = 0_u32;
    // SAFETY: `descriptor` owns a valid security descriptor returned by `GetSecurityInfo`, and
    // both output pointers reference initialized local variables.
    if unsafe { GetSecurityDescriptorControl(descriptor.as_ptr(), &mut control, &mut revision) }
      == 0
    {
      return Err(io::Error::last_os_error());
    }

    let mut acl_info = ACL_SIZE_INFORMATION::default();
    // SAFETY: `dacl` points into the live descriptor allocation and the output buffer matches the
    // requested `AclSizeInformation` structure exactly.
    if unsafe {
      GetAclInformation(
        dacl,
        (&mut acl_info as *mut ACL_SIZE_INFORMATION).cast(),
        size_of::<ACL_SIZE_INFORMATION>() as u32,
        AclSizeInformation,
      )
    } == 0
    {
      return Err(io::Error::last_os_error());
    }
    if acl_info.AceCount != 1 {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
          "named-pipe DACL contains {} ACEs instead of one",
          acl_info.AceCount
        ),
      ));
    }

    let mut ace_pointer: *mut c_void = null_mut();
    // SAFETY: The ACL remains live and reports exactly one ACE, so index zero is valid. Windows
    // initializes `ace_pointer` to storage within that ACL on success.
    if unsafe { GetAce(dacl, 0, &mut ace_pointer) } == 0 {
      return Err(io::Error::last_os_error());
    }
    if ace_pointer.is_null() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Windows returned a null ACE pointer",
      ));
    }

    // SAFETY: `GetAce` returned a pointer to a valid ACE. Reading its fixed header is valid for
    // every ACE type; the size check precedes interpreting the allow-ACE fields.
    let header = unsafe { &*ace_pointer.cast::<ACE_HEADER>() };
    if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
          "named-pipe DACL contains ACE type {} instead of an allow ACE",
          header.AceType
        ),
      ));
    }
    let sid_offset = size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>();
    const SID_HEADER_SIZE: usize = 8;
    if usize::from(header.AceSize) < sid_offset + SID_HEADER_SIZE {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "named-pipe DACL contains a truncated ACE",
      ));
    }
    // SAFETY: The descriptor contains an allow ACE produced from Cadder's SDDL, and the checked
    // ACE size covers the fixed `ACCESS_ALLOWED_ACE` fields including `SidStart`.
    let ace = unsafe { &*ace_pointer.cast::<ACCESS_ALLOWED_ACE>() };
    let trustee_sid = addr_of!(ace.SidStart).cast_mut().cast();
    // SAFETY: The ACE-size check above covers the fixed SID header at `trustee_sid`.
    if unsafe { IsValidSid(trustee_sid) } == 0 {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "named-pipe DACL contains an invalid trustee SID",
      ));
    }
    // SAFETY: `IsValidSid` accepted the SID stored within the live ACE.
    let trustee_sid_size = unsafe { GetLengthSid(trustee_sid) } as usize;
    if sid_offset + trustee_sid_size > usize::from(header.AceSize) {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "named-pipe DACL trustee SID exceeds its ACE",
      ));
    }

    Ok(PipeSecurityEvidence {
      dacl_protected: control & SE_DACL_PROTECTED != 0,
      owner_sid: sid_to_string(owner_sid)?,
      ace_flags: header.AceFlags,
      access_mask: ace.Mask,
      trustee_sid: sid_to_string(trustee_sid)?,
    })
  }

  fn assert_owner_only_security(evidence: PipeSecurityEvidence) {
    let owner_sid = current_process_sid().unwrap();
    assert!(evidence.dacl_protected);
    assert_eq!(evidence.owner_sid, owner_sid);
    assert_eq!(evidence.ace_flags, 0);
    assert_eq!(evidence.access_mask, NAMED_PIPE_ALL_ACCESS);
    assert_eq!(evidence.trustee_sid, owner_sid);
  }

  #[derive(Debug)]
  struct PipeSecurityEvidence {
    dacl_protected: bool,
    owner_sid: Box<str>,
    ace_flags: u8,
    access_mask: u32,
    trustee_sid: Box<str>,
  }

  struct LocalSecurityDescriptor(NonNull<c_void>);

  impl LocalSecurityDescriptor {
    fn new(pointer: *mut c_void) -> io::Result<Self> {
      NonNull::new(pointer).map(Self).ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::InvalidData,
          "Windows returned a null security descriptor",
        )
      })
    }

    fn as_ptr(&self) -> *mut c_void {
      self.0.as_ptr()
    }
  }

  impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) {
      // SAFETY: `GetSecurityInfo` allocated this descriptor with `LocalAlloc` semantics, and this
      // guard releases the still-owned pointer exactly once with the matching API.
      let _ = unsafe { LocalFree(self.0.as_ptr()) };
    }
  }

  fn assert_current_thread_is_not_impersonating() {
    let error = match open_thread_token() {
      Ok(_) => panic!("the IPC server thread retained an impersonation token"),
      Err(error) => error,
    };
    assert_eq!(error.raw_os_error(), Some(ERROR_NO_TOKEN as i32));
  }
}
