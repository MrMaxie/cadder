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
    GetSecurityDescriptorControl, IsValidSid, OWNER_SECURITY_INFORMATION, PSID, SE_DACL_PROTECTED,
  },
  System::SystemServices::ACCESS_ALLOWED_ACE_TYPE,
};

use super::{
  FORCE_REVERT_FAILURE_MARKER, current_process_sid, is_canonical_sid, open_thread_token,
  peer_sid_after_preface, receive_authentication_preface,
  receive_authentication_preface_with_timeout, secure_listener_options,
  secure_owner_only_runtime_directory, send_authentication_preface, sid_to_string,
  validate_owner_sid, wide_path, with_impersonated_peer,
};

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
    .join("runtime.data");
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

#[tokio::test]
async fn windows_ipc_security_listener_rejects_remote_clients() {
  let owner_sid = current_process_sid().unwrap();
  let sequence = SOCKET_SEQUENCE.fetch_add(1, Ordering::Relaxed);
  let socket = format!(
    "cadder-ipc-local-only-check-{}-{sequence}",
    std::process::id()
  );
  let listener_name = socket.to_ns_name::<GenericNamespaced>().unwrap();
  let options = ListenerOptions::new()
    .name(listener_name)
    .try_overwrite(true);
  let listener = secure_listener_options(options, &owner_sid)
    .unwrap()
    .create_tokio()
    .unwrap();
  let configuration = format!("{listener:?}");

  assert!(
    configuration.contains("accept_remote: false"),
    "the named-pipe listener must set PIPE_REJECT_REMOTE_CLIENTS: {configuration}"
  );
}

#[test]
fn windows_runtime_security_rejects_a_reparse_point_runtime_directory() {
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
  if unsafe { GetSecurityDescriptorControl(descriptor.as_ptr(), &mut control, &mut revision) } == 0
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
