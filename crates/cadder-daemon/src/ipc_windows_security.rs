//! Windows ownership and peer-authentication primitives for local IPC.

use std::{
  io,
  mem::size_of,
  os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
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
    Foundation::{ERROR_INSUFFICIENT_BUFFER, LocalFree},
    Security::{
      Authorization::ConvertSidToStringSidW, GetTokenInformation, PSID, RevertToSelf, TOKEN_QUERY,
      TOKEN_USER, TokenUser,
    },
    System::{
      Pipes::ImpersonateNamedPipeClient,
      Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
    },
  },
  core::PWSTR,
};

const AUTHENTICATION_PREFACE: [u8; 1] = [b' '];
const AUTHENTICATION_PREFACE_TIMEOUT: Duration = Duration::from_secs(5);

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
  let Stream::NamedPipe(pipe) = stream;
  let handle = pipe.as_handle().as_raw_handle();

  let impersonation = ImpersonationGuard::begin(handle)?;
  let token = open_thread_token()?;
  let sid = token_sid(&token);
  impersonation.finish();
  sid
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
  // SAFETY: `sid` points into live `TOKEN_USER` storage and `string_sid` is a valid out pointer.
  // On success Windows allocates a NUL-terminated string with `LocalAlloc` semantics.
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
    if unsafe { RevertToSelf() } == 0 {
      // Continuing under an untrusted client's identity would violate the daemon trust boundary.
      process::abort();
    }
  }
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
    io,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
  };

  use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions, ToNsName,
    tokio::{Stream, prelude::*},
  };
  use tokio::io::AsyncWriteExt;
  use windows_sys::Win32::Foundation::ERROR_NO_TOKEN;

  use super::{
    current_process_sid, is_canonical_sid, open_thread_token, peer_sid_after_preface,
    receive_authentication_preface, receive_authentication_preface_with_timeout,
    secure_listener_options, send_authentication_preface,
  };

  static SOCKET_SEQUENCE: AtomicU64 = AtomicU64::new(0);

  #[test]
  fn windows_ipc_security_accepts_canonical_sid_text() {
    assert!(is_canonical_sid("S-1-5-21-1000-1001-1002-1003"));
  }

  #[test]
  fn windows_ipc_security_rejects_sddl_injection() {
    assert!(!is_canonical_sid("S-1-5-21-1)(A;;GA;;;WD"));
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

  fn assert_current_thread_is_not_impersonating() {
    let error = match open_thread_token() {
      Ok(_) => panic!("the IPC server thread retained an impersonation token"),
      Err(error) => error,
    };
    assert_eq!(error.raw_os_error(), Some(ERROR_NO_TOKEN as i32));
  }
}
