use interprocess::local_socket::ListenerOptions;
use interprocess::local_socket::tokio::Stream;
use serde::{Deserialize, Serialize};
use std::io;

use crate::{PrivilegeStatus, RuntimePaths};

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
  fn peer_identity_missing_or_unknown_fails_closed() {
    let owner = IpcPrincipal::unknown(PrivilegeStatus::Unknown);
    let peer = IpcPrincipal::unknown(PrivilegeStatus::Unknown);

    let decision = IpcSecurityPolicy.authenticate_peer(&owner, &peer);

    assert!(!decision.is_allowed());
    assert_eq!(decision.reason_code(), "principal-outside-runtime-owner");
  }

  #[test]
  fn current_process_identity_is_authenticated() {
    let principal = IpcPrincipal::current_process(crate::current_privilege_status()).unwrap();

    assert!(principal.is_authenticated());
    assert!(matches!(
      principal.identity_kind(),
      "unixUid" | "windowsSid"
    ));
  }
}
