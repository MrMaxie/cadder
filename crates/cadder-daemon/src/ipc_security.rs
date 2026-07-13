use anyhow::{Context, Result};
use cadder_protocol::{MIN_COMPATIBLE_PROTOCOL_VERSION, PROTOCOL_VERSION, ProtocolCapabilities};
use chrono::{DateTime, Utc};
use interprocess::local_socket::ListenerOptions;
use interprocess::local_socket::tokio::Stream;
use serde::{Deserialize, Serialize};
use std::{
  env, fs,
  io::{self, Write},
  path::PathBuf,
};

use crate::{
  IpcClientError, IpcClientPhase, IpcClientResult, LocalIpcErrorCode, LocalIpcErrorKind,
  PrivilegeStatus, RuntimePaths, current_privilege_status, ipc_client_error::LocalIpcErrorContext,
};

const IPC_ENDPOINT_METADATA_VERSION: u16 = 1;
const IPC_SECURITY_POLICY_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcPrincipal {
  identity: Option<IpcOsIdentity>,
  privilege_status: PrivilegeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum IpcOsIdentity {
  #[cfg(unix)]
  UnixUid(u32),
  #[cfg(windows)]
  WindowsSid(Box<str>),
  #[cfg(test)]
  Test(Box<str>),
}

impl IpcPrincipal {
  pub fn current_process(privilege_status: PrivilegeStatus) -> io::Result<Self> {
    current_process_identity().map(|identity| Self {
      identity: Some(identity),
      privilege_status,
    })
  }

  pub fn unknown(privilege_status: PrivilegeStatus) -> Self {
    Self {
      identity: None,
      privilege_status,
    }
  }

  #[cfg(test)]
  pub(crate) fn test(identity: impl Into<Box<str>>, privilege_status: PrivilegeStatus) -> Self {
    Self {
      identity: Some(IpcOsIdentity::Test(identity.into())),
      privilege_status,
    }
  }

  pub fn is_authenticated(&self) -> bool {
    self.identity.is_some()
  }

  pub fn identity_kind(&self) -> &'static str {
    match self.identity.as_ref() {
      #[cfg(unix)]
      Some(IpcOsIdentity::UnixUid(_)) => "unixUid",
      #[cfg(windows)]
      Some(IpcOsIdentity::WindowsSid(_)) => "windowsSid",
      #[cfg(test)]
      Some(IpcOsIdentity::Test(_)) => "test",
      None => "unknown",
    }
  }

  pub fn privilege_status(&self) -> PrivilegeStatus {
    self.privilege_status
  }

  fn is_same_identity(&self, other: &Self) -> bool {
    matches!(
      (self.identity.as_ref(), other.identity.as_ref()),
      (Some(owner), Some(peer)) if owner == peer
    )
  }

  #[cfg(windows)]
  fn windows_sid(&self) -> Option<&str> {
    match self.identity.as_ref() {
      Some(IpcOsIdentity::WindowsSid(sid)) => Some(sid),
      _ => None,
    }
  }
}

impl Default for IpcPrincipal {
  fn default() -> Self {
    Self::unknown(PrivilegeStatus::Unknown)
  }
}

#[derive(Debug, Clone, Default)]
pub(crate) enum IpcPeerIdentityResolver {
  #[default]
  System,
  #[cfg(test)]
  Fixed(IpcPrincipal),
  #[cfg(test)]
  Failure(io::ErrorKind),
  #[cfg(test)]
  Counting {
    principal: IpcPrincipal,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
  },
}

impl IpcPeerIdentityResolver {
  pub(crate) fn resolve(&self, stream: &Stream) -> io::Result<IpcPrincipal> {
    match self {
      Self::System => peer_process_identity(stream).map(|identity| IpcPrincipal {
        identity: Some(identity),
        privilege_status: PrivilegeStatus::Unknown,
      }),
      #[cfg(test)]
      Self::Fixed(principal) => Ok(principal.clone()),
      #[cfg(test)]
      Self::Failure(kind) => Err(io::Error::new(*kind, "test peer identity failure")),
      #[cfg(test)]
      Self::Counting { principal, calls } => {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(principal.clone())
      }
    }
  }
}

#[cfg(windows)]
pub(crate) async fn send_peer_authentication_preface(stream: &mut Stream) -> io::Result<()> {
  crate::ipc_windows_security::send_authentication_preface(stream).await
}

#[cfg(not(windows))]
pub(crate) async fn send_peer_authentication_preface(_stream: &mut Stream) -> io::Result<()> {
  Ok(())
}

#[cfg(windows)]
pub(crate) async fn receive_peer_authentication_preface(stream: &mut Stream) -> io::Result<()> {
  crate::ipc_windows_security::receive_authentication_preface(stream).await
}

#[cfg(not(windows))]
pub(crate) async fn receive_peer_authentication_preface(_stream: &mut Stream) -> io::Result<()> {
  Ok(())
}

#[cfg(windows)]
pub(crate) fn secure_listener_options<'a>(
  options: ListenerOptions<'a>,
  owner: &IpcPrincipal,
) -> io::Result<ListenerOptions<'a>> {
  let owner_sid = owner.windows_sid().ok_or_else(|| {
    io::Error::new(
      io::ErrorKind::PermissionDenied,
      "the Cadder runtime owner does not have an authenticated Windows SID",
    )
  })?;
  crate::ipc_windows_security::secure_listener_options(options, owner_sid)
}

#[cfg(not(windows))]
#[cfg(not(unix))]
pub(crate) fn secure_listener_options<'a>(
  options: ListenerOptions<'a>,
  _owner: &IpcPrincipal,
) -> io::Result<ListenerOptions<'a>> {
  Ok(options)
}

#[cfg(unix)]
pub(crate) fn secure_listener_options<'a>(
  options: ListenerOptions<'a>,
  _owner: &IpcPrincipal,
) -> io::Result<ListenerOptions<'a>> {
  crate::ipc_unix_security::secure_listener_options(options)
}

#[cfg(unix)]
pub(crate) fn secure_bound_socket(paths: &RuntimePaths) -> io::Result<()> {
  crate::ipc_unix_security::secure_bound_socket(paths)
}

#[cfg(not(unix))]
pub(crate) fn secure_bound_socket(_paths: &RuntimePaths) -> io::Result<()> {
  Ok(())
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
      allowed_principal: "same-runtime-owner-identity".to_string(),
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
  pub privilege_status: PrivilegeStatus,
  pub security_policy: IpcSecurityPolicySummary,
}

impl IpcEndpointMetadata {
  pub fn current(paths: &RuntimePaths) -> io::Result<Self> {
    let privilege_status = current_privilege_status();
    IpcPrincipal::current_process(privilege_status).map(|_| Self::new(paths, privilege_status))
  }

  pub fn new(paths: &RuntimePaths, privilege_status: PrivilegeStatus) -> Self {
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
      privilege_status,
      security_policy: IpcSecurityPolicySummary::current(),
    }
  }
}

#[derive(Debug, Clone, Default)]
pub struct IpcSecurityPolicy;

impl IpcSecurityPolicy {
  pub fn authenticate_peer(&self, owner: &IpcPrincipal, peer: &IpcPrincipal) -> IpcAccessDecision {
    if owner.is_same_identity(peer) {
      return IpcAccessDecision::allowed("same-runtime-owner-identity");
    }

    IpcAccessDecision::denied(
      "principal-outside-runtime-owner",
      "Cadder denied a local connection whose operating-system identity does not match the runtime owner."
        .to_string(),
      "Use the same local account that owns the Cadder runtime, or start a separate runtime profile for this account."
        .to_string(),
    )
  }

  pub fn evaluate(
    &self,
    owner: &IpcPrincipal,
    peer: &IpcPrincipal,
    operation: &IpcOperation,
  ) -> IpcAccessDecision {
    if self.authenticate_peer(owner, peer).is_allowed() {
      return IpcAccessDecision::allowed("same-runtime-owner-identity");
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
    let metadata = IpcEndpointMetadata::current(paths)
      .context("authenticate the Cadder runtime-owner identity")?;
    Self::publish(paths, &metadata)
  }

  pub fn publish(paths: &RuntimePaths, metadata: &IpcEndpointMetadata) -> Result<Self> {
    let path = paths.ipc_endpoint_path();
    write_ipc_endpoint_metadata(paths, metadata)
      .with_context(|| format!("write IPC endpoint metadata {}", path.display()))?;
    Ok(Self { path })
  }
}

impl Drop for IpcEndpointPublication {
  fn drop(&mut self) {
    let _ = fs::remove_file(&self.path);
  }
}

pub fn discover_ipc_endpoint(paths: &RuntimePaths) -> IpcClientResult<IpcEndpointMetadata> {
  let path = paths.ipc_endpoint_path();
  let content = fs::read(&path).map_err(discovery_read_error)?;
  serde_json::from_slice(&content).map_err(|error| {
    IpcClientError::local(LocalIpcErrorContext {
      kind: LocalIpcErrorKind::Discovery,
      phase: IpcClientPhase::DiscoveryDecode,
      code: LocalIpcErrorCode::InvalidDiscovery,
      message: "Cadder IPC discovery is invalid; no request was sent.".into(),
      guidance: Some(
        "Restart the Cadder daemon for this runtime. If the error remains, inspect the discovery diagnostics."
          .into(),
      ),
      retryable: false,
      request_id: None,
      operation: Some("discover-ipc-endpoint".into()),
      source: Some(Box::new(error)),
    })
  })
}

fn discovery_read_error(error: std::io::Error) -> IpcClientError {
  let (code, message, guidance, retryable) = match error.kind() {
    std::io::ErrorKind::PermissionDenied => (
      LocalIpcErrorCode::PermissionDenied,
      "Cadder cannot read IPC discovery for this runtime; no request was sent.",
      "Use the account that owns this Cadder runtime or select an accessible profile.",
      false,
    ),
    std::io::ErrorKind::NotFound => (
      LocalIpcErrorCode::DiscoveryUnavailable,
      "Cadder IPC discovery is unavailable; no request was sent.",
      "Start the Cadder daemon for this runtime, then retry.",
      true,
    ),
    std::io::ErrorKind::Interrupted => (
      LocalIpcErrorCode::DiscoveryReadFailed,
      "Cadder could not finish reading IPC discovery; no request was sent.",
      "Retry once. If the error remains, inspect the runtime-directory diagnostics.",
      true,
    ),
    _ => (
      LocalIpcErrorCode::DiscoveryReadFailed,
      "Cadder could not read IPC discovery; no request was sent.",
      "Inspect the runtime directory and local filesystem diagnostics before retrying.",
      false,
    ),
  };
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Discovery,
    phase: IpcClientPhase::DiscoveryRead,
    code,
    message: message.into(),
    guidance: Some(guidance.into()),
    retryable,
    request_id: None,
    operation: Some("discover-ipc-endpoint".into()),
    source: Some(Box::new(error)),
  })
}

fn write_ipc_endpoint_metadata(paths: &RuntimePaths, metadata: &IpcEndpointMetadata) -> Result<()> {
  let path = paths.ipc_endpoint_path();
  #[cfg(unix)]
  let mut file = {
    crate::ipc_unix_security::secure_runtime_paths(paths).with_context(|| {
      format!(
        "secure IPC runtime directory {}",
        paths.runtime_dir().display()
      )
    })?;
    crate::ipc_unix_security::open_discovery_file_for_write(paths)
      .with_context(|| format!("open IPC endpoint metadata {}", path.display()))?
  };
  #[cfg(not(unix))]
  let mut file = {
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent).with_context(|| {
        format!(
          "create IPC endpoint metadata directory {}",
          parent.display()
        )
      })?;
    }
    std::fs::OpenOptions::new()
      .write(true)
      .create(true)
      .truncate(true)
      .open(&path)
      .with_context(|| format!("open IPC endpoint metadata {}", path.display()))?
  };
  serde_json::to_writer_pretty(&mut file, metadata)?;
  file.write_all(b"\n")?;
  file.sync_data()?;
  Ok(())
}

#[cfg(unix)]
fn current_process_identity() -> io::Result<IpcOsIdentity> {
  Ok(IpcOsIdentity::UnixUid(
    crate::ipc_unix_security::current_euid(),
  ))
}

#[cfg(windows)]
fn current_process_identity() -> io::Result<IpcOsIdentity> {
  crate::ipc_windows_security::current_process_sid().map(IpcOsIdentity::WindowsSid)
}

#[cfg(unix)]
fn peer_process_identity(stream: &Stream) -> io::Result<IpcOsIdentity> {
  crate::ipc_unix_security::peer_euid(stream).map(IpcOsIdentity::UnixUid)
}

#[cfg(windows)]
fn peer_process_identity(stream: &Stream) -> io::Result<IpcOsIdentity> {
  crate::ipc_windows_security::peer_sid_after_preface(stream).map(IpcOsIdentity::WindowsSid)
}

#[cfg(not(any(unix, windows)))]
fn current_process_identity() -> io::Result<IpcOsIdentity> {
  Err(io::Error::new(
    io::ErrorKind::Unsupported,
    "Cadder cannot authenticate the runtime owner on this platform",
  ))
}

#[cfg(not(any(unix, windows)))]
fn peer_process_identity(_stream: &Stream) -> io::Result<IpcOsIdentity> {
  Err(io::Error::new(
    io::ErrorKind::Unsupported,
    "Cadder cannot authenticate local IPC peers on this platform",
  ))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn typed_error_discovery_reads_distinguish_absent_permission_and_io_failures() {
    let cases = [
      (
        std::io::ErrorKind::NotFound,
        LocalIpcErrorCode::DiscoveryUnavailable,
        true,
      ),
      (
        std::io::ErrorKind::PermissionDenied,
        LocalIpcErrorCode::PermissionDenied,
        false,
      ),
      (
        std::io::ErrorKind::Other,
        LocalIpcErrorCode::DiscoveryReadFailed,
        false,
      ),
      (
        std::io::ErrorKind::Interrupted,
        LocalIpcErrorCode::DiscoveryReadFailed,
        true,
      ),
    ];

    for (kind, code, retryable) in cases {
      let error = discovery_read_error(std::io::Error::new(kind, "test discovery failure"));
      let local = error
        .local_error()
        .expect("expected a local discovery error");
      assert_eq!(local.kind(), LocalIpcErrorKind::Discovery);
      assert_eq!(local.phase(), IpcClientPhase::DiscoveryRead);
      assert_eq!(local.code(), code);
      assert_eq!(local.retryable(), retryable);
      assert!(std::error::Error::source(local).is_some());
    }
  }

  #[test]
  fn policy_allows_same_user_non_elevated_client_to_elevated_endpoint() {
    let owner = IpcPrincipal::test("owner-identity", PrivilegeStatus::Elevated);
    let peer = IpcPrincipal::test("owner-identity", PrivilegeStatus::NormalUser);
    let decision = IpcSecurityPolicy.evaluate(
      &owner,
      &peer,
      &IpcOperation::state_changing("set-autostart"),
    );

    assert!(decision.is_allowed());
  }

  #[test]
  fn policy_denies_different_user_without_leaking_peer_account() {
    let owner = IpcPrincipal::test("owner-identity", PrivilegeStatus::Elevated);
    let peer = IpcPrincipal::test("other-token=secret", PrivilegeStatus::NormalUser);
    let decision =
      IpcSecurityPolicy.evaluate(&owner, &peer, &IpcOperation::state_changing("shutdown"));

    assert!(!decision.is_allowed());
    assert_eq!(decision.reason_code(), "principal-outside-runtime-owner");
    assert!(!decision.message().contains("bob-token"));
    assert!(!decision.message().contains("secret"));
  }

  #[test]
  fn endpoint_metadata_roundtrips_policy_and_capabilities() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let endpoint = IpcEndpointMetadata::new(&paths, PrivilegeStatus::NormalUser);
    let json = serde_json::to_string(&endpoint).unwrap();
    let decoded: IpcEndpointMetadata = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded, endpoint);
    assert_eq!(decoded.socket_name, paths.socket_name());
    assert_eq!(
      decoded.security_policy.allowed_principal,
      "same-runtime-owner-identity"
    );
    assert!(decoded.capabilities.supports("logs"));
  }

  #[test]
  fn publication_writes_and_removes_endpoint_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let metadata = IpcEndpointMetadata::new(&paths, PrivilegeStatus::NormalUser);

    let publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();
    let discovered = discover_ipc_endpoint(&paths).unwrap();

    assert_eq!(discovered.socket_name, paths.socket_name());
    drop(publication);
    assert!(!paths.ipc_endpoint_path().exists());
  }

  #[test]
  fn peer_identity_missing_or_unknown_fails_closed() {
    let owner = IpcPrincipal::unknown(PrivilegeStatus::Unknown);
    let peer = IpcPrincipal::unknown(PrivilegeStatus::Unknown);

    let decision = IpcSecurityPolicy.authenticate_peer(&owner, &peer);

    assert!(!decision.is_allowed());
    assert_eq!(decision.reason_code(), "principal-outside-runtime-owner");
  }

  #[test]
  fn current_process_identity_is_authenticated() {
    let principal = IpcPrincipal::current_process(current_privilege_status()).unwrap();

    assert!(principal.is_authenticated());
    assert!(matches!(
      principal.identity_kind(),
      "unixUid" | "windowsSid"
    ));
  }
}
