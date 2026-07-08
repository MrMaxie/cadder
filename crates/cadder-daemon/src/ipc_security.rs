use anyhow::{Context, Result};
use cadder_protocol::{MIN_COMPATIBLE_PROTOCOL_VERSION, PROTOCOL_VERSION, ProtocolCapabilities};
use chrono::{DateTime, Utc};
use interprocess::local_socket::PeerCreds;
use serde::{Deserialize, Serialize};
use std::{
  env,
  fs::{self, OpenOptions},
  io::Write,
  path::{Path, PathBuf},
};

use crate::{PrivilegeStatus, RuntimePaths, current_privilege_status};

const IPC_ENDPOINT_METADATA_VERSION: u16 = 1;
const IPC_SECURITY_POLICY_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IpcPrincipal {
  account: String,
  privilege_status: PrivilegeStatus,
}

impl IpcPrincipal {
  pub fn new(account: impl Into<String>, privilege_status: PrivilegeStatus) -> Self {
    Self {
      account: normalize_account(account.into()),
      privilege_status,
    }
  }

  pub fn current_process(privilege_status: PrivilegeStatus) -> Self {
    Self::new(current_account_label(), privilege_status)
  }

  pub fn from_peer_credentials(credentials: Option<PeerCreds>) -> Self {
    let account = credentials
      .and_then(peer_account_label)
      .unwrap_or_else(|| "unknown".to_string());
    Self::new(account, PrivilegeStatus::Unknown)
  }

  pub fn account(&self) -> &str {
    &self.account
  }

  pub fn privilege_status(&self) -> PrivilegeStatus {
    self.privilege_status
  }

  fn is_same_account(&self, other: &Self) -> bool {
    self.account.eq_ignore_ascii_case(&other.account)
  }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IpcOperationKind {
  ReadOnly,
  StateChanging,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IpcOperation {
  name: String,
  kind: IpcOperationKind,
}

impl IpcOperation {
  pub fn read_only(name: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      kind: IpcOperationKind::ReadOnly,
    }
  }

  pub fn state_changing(name: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      kind: IpcOperationKind::StateChanging,
    }
  }

  pub fn name(&self) -> &str {
    &self.name
  }

  pub fn kind(&self) -> IpcOperationKind {
    self.kind
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IpcSecurityPolicySummary {
  pub policy_version: u16,
  pub allowed_principal: String,
  pub allowed_operations: Box<[String]>,
  pub denied_principals: Box<[String]>,
}

impl IpcSecurityPolicySummary {
  fn current() -> Self {
    Self {
      policy_version: IPC_SECURITY_POLICY_VERSION,
      allowed_principal: "same-runtime-owner-account".to_string(),
      allowed_operations: ["read-only", "state-changing"]
        .into_iter()
        .map(String::from)
        .collect(),
      denied_principals: ["different-local-account", "unknown-local-account"]
        .into_iter()
        .map(String::from)
        .collect(),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IpcEndpointMetadata {
  pub metadata_version: u16,
  pub cadder_version: String,
  pub protocol_version: u16,
  pub minimum_compatible_protocol_version: u16,
  pub capabilities: ProtocolCapabilities,
  pub runtime_profile: String,
  pub runtime_dir: String,
  pub instance_key: String,
  pub socket_name: String,
  pub process_id: u32,
  pub published_at_utc: DateTime<Utc>,
  pub executable_path: Option<String>,
  pub owner_principal: IpcPrincipal,
  pub privilege_status: PrivilegeStatus,
  pub security_policy: IpcSecurityPolicySummary,
}

impl IpcEndpointMetadata {
  pub fn current(paths: &RuntimePaths) -> Self {
    let privilege_status = current_privilege_status();
    Self::new(paths, IpcPrincipal::current_process(privilege_status))
  }

  pub fn new(paths: &RuntimePaths, owner_principal: IpcPrincipal) -> Self {
    Self {
      metadata_version: IPC_ENDPOINT_METADATA_VERSION,
      cadder_version: env!("CARGO_PKG_VERSION").to_string(),
      protocol_version: PROTOCOL_VERSION,
      minimum_compatible_protocol_version: MIN_COMPATIBLE_PROTOCOL_VERSION,
      capabilities: ProtocolCapabilities::current(),
      runtime_profile: paths.runtime_profile().to_string(),
      runtime_dir: paths.runtime_dir().display().to_string(),
      instance_key: paths.instance_key().to_string(),
      socket_name: paths.socket_name().to_string(),
      process_id: std::process::id(),
      published_at_utc: Utc::now(),
      executable_path: env::current_exe()
        .ok()
        .map(|path| path.display().to_string()),
      privilege_status: owner_principal.privilege_status(),
      owner_principal,
      security_policy: IpcSecurityPolicySummary::current(),
    }
  }
}

#[derive(Debug, Clone, Default)]
pub struct IpcSecurityPolicy;

impl IpcSecurityPolicy {
  pub fn evaluate(
    &self,
    endpoint: &IpcEndpointMetadata,
    peer: &IpcPrincipal,
    operation: &IpcOperation,
  ) -> IpcAccessDecision {
    if endpoint.owner_principal.is_same_account(peer) {
      return IpcAccessDecision::allowed("same-runtime-owner-account");
    }

    IpcAccessDecision::denied(
      "principal-outside-runtime-owner",
      format!(
        "Cadder IPC operation `{}` is not allowed for principals outside the runtime owner account.",
        operation.name()
      ),
      "Use the same local account that owns the Cadder runtime, or start a separate runtime profile for this account."
        .to_string(),
    )
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcAccessDecision {
  allowed: bool,
  reason_code: &'static str,
  message: String,
  guidance: Option<String>,
}

impl IpcAccessDecision {
  fn allowed(reason_code: &'static str) -> Self {
    Self {
      allowed: true,
      reason_code,
      message: "Cadder IPC operation is allowed by the local security policy.".to_string(),
      guidance: None,
    }
  }

  fn denied(reason_code: &'static str, message: String, guidance: String) -> Self {
    Self {
      allowed: false,
      reason_code,
      message,
      guidance: Some(guidance),
    }
  }

  pub fn is_allowed(&self) -> bool {
    self.allowed
  }

  pub fn reason_code(&self) -> &'static str {
    self.reason_code
  }

  pub fn message(&self) -> &str {
    &self.message
  }

  pub fn guidance(&self) -> Option<&str> {
    self.guidance.as_deref()
  }
}

#[derive(Debug)]
pub struct IpcEndpointPublication {
  path: PathBuf,
}

impl IpcEndpointPublication {
  pub fn publish_current(paths: &RuntimePaths) -> Result<Self> {
    let metadata = IpcEndpointMetadata::current(paths);
    Self::publish(paths, &metadata)
  }

  pub fn publish(paths: &RuntimePaths, metadata: &IpcEndpointMetadata) -> Result<Self> {
    let path = paths.ipc_endpoint_path();
    write_ipc_endpoint_metadata(&path, metadata)
      .with_context(|| format!("write IPC endpoint metadata {}", path.display()))?;
    Ok(Self { path })
  }
}

impl Drop for IpcEndpointPublication {
  fn drop(&mut self) {
    let _ = fs::remove_file(&self.path);
  }
}

pub fn discover_ipc_endpoint(paths: &RuntimePaths) -> Result<IpcEndpointMetadata> {
  let path = paths.ipc_endpoint_path();
  let content = fs::read_to_string(&path)
    .with_context(|| format!("read IPC endpoint metadata {}", path.display()))?;
  serde_json::from_str(content.trim())
    .with_context(|| format!("parse IPC endpoint metadata {}", path.display()))
}

fn write_ipc_endpoint_metadata(path: &Path, metadata: &IpcEndpointMetadata) -> Result<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).with_context(|| {
      format!(
        "create IPC endpoint metadata directory {}",
        parent.display()
      )
    })?;
  }
  let mut file = OpenOptions::new()
    .write(true)
    .create(true)
    .truncate(true)
    .open(path)
    .with_context(|| format!("open IPC endpoint metadata {}", path.display()))?;
  serde_json::to_writer_pretty(&mut file, metadata)?;
  file.write_all(b"\n")?;
  file.sync_data()?;
  Ok(())
}

#[cfg(windows)]
fn current_account_label() -> String {
  windows_current_process_account_label()
    .or_else(env_account_label)
    .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(unix)]
fn current_account_label() -> String {
  // SAFETY: `geteuid` has no preconditions and only reads process identity.
  format!("uid:{}", unsafe { libc::geteuid() })
}

#[cfg(not(any(unix, windows)))]
fn current_account_label() -> String {
  env_account_label().unwrap_or_else(|| "unknown".to_string())
}

#[cfg(unix)]
fn peer_account_label(credentials: PeerCreds) -> Option<String> {
  credentials.euid().map(|uid| format!("uid:{uid}"))
}

#[cfg(windows)]
fn peer_account_label(credentials: PeerCreds) -> Option<String> {
  windows_process_account_label(credentials.pid()?)
}

#[cfg(not(any(unix, windows)))]
fn peer_account_label(_credentials: PeerCreds) -> Option<String> {
  None
}

fn env_account_label() -> Option<String> {
  let domain = env_value("USERDOMAIN");
  let name = env_value("USERNAME").or_else(|| env_value("USER"));
  match (domain, name) {
    (Some(domain), Some(name)) if !domain.eq_ignore_ascii_case(&name) => {
      Some(normalize_account(format!("{domain}\\{name}")))
    }
    (_, Some(name)) => Some(normalize_account(name)),
    _ => None,
  }
}

#[cfg(windows)]
fn windows_current_process_account_label() -> Option<String> {
  use windows_sys::Win32::System::Threading::GetCurrentProcess;

  let mut token = std::ptr::null_mut();
  // SAFETY: `GetCurrentProcess` returns a pseudo-handle that is valid for
  // `OpenProcessToken`; `token` is initialized by Windows on success.
  if unsafe { windows_open_process_token(GetCurrentProcess(), &mut token) } {
    let label = windows_token_account_label(token);
    // SAFETY: `token` was returned by `OpenProcessToken` and must be closed.
    unsafe {
      windows_sys::Win32::Foundation::CloseHandle(token);
    }
    return label;
  }

  None
}

#[cfg(windows)]
fn windows_process_account_label(process_id: u32) -> Option<String> {
  use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

  // SAFETY: Opening another process is fallible; invalid or inaccessible PIDs
  // return a null handle, which is handled below.
  let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
  if process.is_null() {
    return None;
  }

  let mut token = std::ptr::null_mut();
  // SAFETY: `process` is a valid handle from `OpenProcess`; `token` is
  // initialized by Windows on success.
  let label = if unsafe { windows_open_process_token(process, &mut token) } {
    let label = windows_token_account_label(token);
    // SAFETY: `token` was returned by `OpenProcessToken` and must be closed.
    unsafe {
      windows_sys::Win32::Foundation::CloseHandle(token);
    }
    label
  } else {
    None
  };

  // SAFETY: `process` was returned by `OpenProcess` and must be closed.
  unsafe {
    windows_sys::Win32::Foundation::CloseHandle(process);
  }
  label
}

#[cfg(windows)]
unsafe fn windows_open_process_token(
  process: windows_sys::Win32::Foundation::HANDLE,
  token: *mut windows_sys::Win32::Foundation::HANDLE,
) -> bool {
  use windows_sys::Win32::{Security::TOKEN_QUERY, System::Threading::OpenProcessToken};

  // SAFETY: The caller guarantees `process` and `token` are valid for the
  // Windows API call.
  unsafe { OpenProcessToken(process, TOKEN_QUERY, token) != 0 }
}

#[cfg(windows)]
fn windows_token_account_label(token: windows_sys::Win32::Foundation::HANDLE) -> Option<String> {
  use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_USER, TokenUser};

  let mut required_len = 0;
  // SAFETY: The first call intentionally passes a null buffer to obtain the
  // required size for the token user record.
  unsafe {
    GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut required_len);
  }
  if required_len == 0 {
    return None;
  }

  let mut buffer = vec![0_u8; required_len as usize];
  // SAFETY: `buffer` has the length requested by Windows and is writable.
  let ok = unsafe {
    GetTokenInformation(
      token,
      TokenUser,
      buffer.as_mut_ptr().cast(),
      required_len,
      &mut required_len,
    ) != 0
  };
  if !ok {
    return None;
  }

  // SAFETY: A successful `GetTokenInformation(TokenUser)` fills the buffer
  // with a `TOKEN_USER`; `Vec<u8>` alignment is not guaranteed, so read the
  // header unaligned and copy it by value.
  let token_user = unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<TOKEN_USER>()) };
  windows_sid_account_label(token_user.User.Sid)
}

#[cfg(windows)]
fn windows_sid_account_label(sid: windows_sys::Win32::Security::PSID) -> Option<String> {
  use windows_sys::Win32::Security::{LookupAccountSidW, SID_NAME_USE};

  let mut name_len = 0;
  let mut domain_len = 0;
  let mut sid_name_use: SID_NAME_USE = 0;
  // SAFETY: This sizing call follows the documented `LookupAccountSidW`
  // pattern with null output buffers to retrieve required lengths.
  unsafe {
    LookupAccountSidW(
      std::ptr::null(),
      sid,
      std::ptr::null_mut(),
      &mut name_len,
      std::ptr::null_mut(),
      &mut domain_len,
      &mut sid_name_use,
    );
  }
  if name_len == 0 {
    return None;
  }

  let mut name = vec![0_u16; name_len as usize];
  let mut domain = vec![0_u16; domain_len as usize];
  let domain_ptr = if domain.is_empty() {
    std::ptr::null_mut()
  } else {
    domain.as_mut_ptr()
  };
  // SAFETY: `name` and `domain` buffers are sized from the previous Windows
  // API call and remain valid for the duration of this call.
  let ok = unsafe {
    LookupAccountSidW(
      std::ptr::null(),
      sid,
      name.as_mut_ptr(),
      &mut name_len,
      domain_ptr,
      &mut domain_len,
      &mut sid_name_use,
    ) != 0
  };
  if !ok {
    return None;
  }

  let name = String::from_utf16_lossy(&name[..name_len as usize]);
  let domain = String::from_utf16_lossy(&domain[..domain_len as usize]);
  if domain.is_empty() || domain.eq_ignore_ascii_case(&name) {
    Some(normalize_account(name))
  } else {
    Some(normalize_account(format!("{domain}\\{name}")))
  }
}

fn env_value(key: &str) -> Option<String> {
  env::var(key)
    .ok()
    .map(normalize_account)
    .filter(|value| !value.is_empty())
}

fn normalize_account(account: String) -> String {
  let account = account.trim();
  if account.is_empty() {
    "unknown".to_string()
  } else {
    account.to_string()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn policy_allows_same_user_non_elevated_client_to_elevated_endpoint() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let endpoint = IpcEndpointMetadata::new(
      &paths,
      IpcPrincipal::new("DESKTOP\\alice", PrivilegeStatus::Elevated),
    );
    let peer = IpcPrincipal::new("desktop\\alice", PrivilegeStatus::NormalUser);
    let decision = IpcSecurityPolicy.evaluate(
      &endpoint,
      &peer,
      &IpcOperation::state_changing("set-autostart"),
    );

    assert!(decision.is_allowed());
  }

  #[test]
  fn policy_denies_different_user_without_leaking_peer_account() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let endpoint = IpcEndpointMetadata::new(
      &paths,
      IpcPrincipal::new("DESKTOP\\alice", PrivilegeStatus::Elevated),
    );
    let peer = IpcPrincipal::new("DESKTOP\\bob-token=secret", PrivilegeStatus::NormalUser);
    let decision =
      IpcSecurityPolicy.evaluate(&endpoint, &peer, &IpcOperation::state_changing("shutdown"));

    assert!(!decision.is_allowed());
    assert_eq!(decision.reason_code(), "principal-outside-runtime-owner");
    assert!(!decision.message().contains("bob-token"));
    assert!(!decision.message().contains("secret"));
  }

  #[test]
  fn endpoint_metadata_roundtrips_policy_and_capabilities() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let endpoint = IpcEndpointMetadata::new(
      &paths,
      IpcPrincipal::new("alice", PrivilegeStatus::NormalUser),
    );
    let json = serde_json::to_string(&endpoint).unwrap();
    let decoded: IpcEndpointMetadata = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.socket_name, paths.socket_name());
    assert_eq!(
      decoded.security_policy.allowed_principal,
      "same-runtime-owner-account"
    );
    assert!(decoded.capabilities.supports("logs"));
  }

  #[test]
  fn publication_writes_and_removes_endpoint_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let metadata = IpcEndpointMetadata::new(
      &paths,
      IpcPrincipal::new("alice", PrivilegeStatus::NormalUser),
    );

    let publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();
    let discovered = discover_ipc_endpoint(&paths).unwrap();

    assert_eq!(discovered.socket_name, paths.socket_name());
    drop(publication);
    assert!(!paths.ipc_endpoint_path().exists());
  }
}
