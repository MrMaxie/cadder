#[cfg(any(test, debug_assertions))]
use std::env;

use serde::{Deserialize, Serialize};

#[cfg(any(test, debug_assertions))]
const TEST_OVERRIDE_ENV: &str = "CADDER_TEST_ELEVATED_CONTEXT";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PrivilegeStatus {
  NormalUser,
  Elevated,
  Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegeDiagnostic {
  pub code: &'static str,
  pub message: String,
  pub guidance: String,
}

pub fn current_privilege_status() -> PrivilegeStatus {
  if let Some(status) = test_override_status() {
    return status;
  }

  platform_privilege_status()
}

pub fn management_surface_privilege_diagnostic(surface_label: &str) -> Option<PrivilegeDiagnostic> {
  (current_privilege_status() == PrivilegeStatus::Elevated)
    .then(|| elevated_management_surface_diagnostic(surface_label))
}

pub fn shim_privilege_diagnostic(surface_label: &str) -> Option<PrivilegeDiagnostic> {
  (current_privilege_status() == PrivilegeStatus::Elevated)
    .then(|| elevated_shim_diagnostic(surface_label))
}

pub fn elevated_management_surface_diagnostic(surface_label: &str) -> PrivilegeDiagnostic {
  PrivilegeDiagnostic {
    code: "least-privilege-elevated-context",
    message: format!(
      "{surface_label} is running with elevated privileges. Cadder management surfaces are designed to run as the normal user."
    ),
    guidance: "Close this process and reopen it from a normal user shell when possible."
      .to_string(),
  }
}

pub fn elevated_shim_diagnostic(surface_label: &str) -> PrivilegeDiagnostic {
  PrivilegeDiagnostic {
    code: "least-privilege-elevated-shim",
    message: format!(
      "{surface_label} is running with elevated privileges. Normal Cadder shim registrations should run as the user that owns the Cadder runtime."
    ),
    guidance:
      "Restart from a normal user shell unless the hosted project itself requires elevation."
        .to_string(),
  }
}

#[cfg(windows)]
fn platform_privilege_status() -> PrivilegeStatus {
  if is_elevated::is_elevated() {
    PrivilegeStatus::Elevated
  } else {
    PrivilegeStatus::NormalUser
  }
}

#[cfg(unix)]
fn platform_privilege_status() -> PrivilegeStatus {
  // SAFETY: `geteuid` has no preconditions and only reads process identity.
  if unsafe { libc::geteuid() } == 0 {
    PrivilegeStatus::Elevated
  } else {
    PrivilegeStatus::NormalUser
  }
}

#[cfg(not(any(unix, windows)))]
fn platform_privilege_status() -> PrivilegeStatus {
  PrivilegeStatus::Unknown
}

fn test_override_status() -> Option<PrivilegeStatus> {
  #[cfg(any(test, debug_assertions))]
  {
    let value = env::var(TEST_OVERRIDE_ENV).ok()?;
    if value.eq_ignore_ascii_case("elevated")
      || value.eq_ignore_ascii_case("admin")
      || value.eq_ignore_ascii_case("root")
      || value == "1"
    {
      return Some(PrivilegeStatus::Elevated);
    }
    if value.eq_ignore_ascii_case("normal") || value.eq_ignore_ascii_case("user") || value == "0" {
      return Some(PrivilegeStatus::NormalUser);
    }
    if value.eq_ignore_ascii_case("unknown") {
      return Some(PrivilegeStatus::Unknown);
    }
  }

  None
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn elevated_management_surface_diagnostic_names_surface_and_guidance() {
    let diagnostic = elevated_management_surface_diagnostic("cadder");

    assert_eq!(diagnostic.code, "least-privilege-elevated-context");
    assert!(diagnostic.message.contains("cadder"));
    assert!(diagnostic.guidance.contains("normal user shell"));
  }

  #[test]
  fn elevated_shim_diagnostic_uses_shim_specific_guidance() {
    let diagnostic = elevated_shim_diagnostic("caddy shim");

    assert_eq!(diagnostic.code, "least-privilege-elevated-shim");
    assert!(diagnostic.message.contains("caddy shim"));
    assert!(
      diagnostic
        .message
        .contains("user that owns the Cadder runtime")
    );
    assert!(
      diagnostic
        .guidance
        .contains("hosted project itself requires elevation")
    );
  }

  #[test]
  fn test_override_status_accepts_supported_values() {
    let _guard = crate::TEST_ENV_LOCK.lock().unwrap();

    unsafe { env::set_var(TEST_OVERRIDE_ENV, "elevated") };
    assert_eq!(current_privilege_status(), PrivilegeStatus::Elevated);

    unsafe { env::set_var(TEST_OVERRIDE_ENV, "normal") };
    assert_eq!(current_privilege_status(), PrivilegeStatus::NormalUser);

    unsafe { env::set_var(TEST_OVERRIDE_ENV, "unknown") };
    assert_eq!(current_privilege_status(), PrivilegeStatus::Unknown);

    unsafe { env::remove_var(TEST_OVERRIDE_ENV) };
  }
}
