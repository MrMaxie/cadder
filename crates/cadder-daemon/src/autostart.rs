use crate::paths::RuntimePaths;
use cadder_ipc::{AutostartDiagnostic, AutostartMode, AutostartStatus};
#[cfg(unix)]
use directories::BaseDirs;
#[cfg(unix)]
use std::fs;
#[cfg(all(test, unix, not(target_os = "macos")))]
use std::os::unix::fs as unix_fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(all(test, unix))]
use std::path::Path;
#[cfg(windows)]
use std::process::Command;
use std::{env, io, path::PathBuf};

#[cfg(target_os = "macos")]
const APP_ID: &str = "dev.cadder";
#[cfg(windows)]
const RUN_VALUE_NAME: &str = "Cadder";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone)]
pub struct AutostartManager {
  daemon_path: Option<PathBuf>,
  #[cfg(unix)]
  base_dirs: Option<BaseDirs>,
  #[cfg(all(unix, not(target_os = "macos")))]
  config_dir_override: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AutostartView {
  pub mode: AutostartMode,
  pub status: AutostartStatus,
  pub target: Option<String>,
  pub diagnostics: Vec<AutostartDiagnostic>,
}

impl AutostartManager {
  pub fn disabled() -> Self {
    Self {
      daemon_path: None,
      #[cfg(unix)]
      base_dirs: None,
      #[cfg(all(unix, not(target_os = "macos")))]
      config_dir_override: None,
    }
  }

  pub fn new(_paths: &RuntimePaths) -> Self {
    let daemon_path = env::current_exe().ok();
    Self {
      daemon_path,
      #[cfg(unix)]
      base_dirs: BaseDirs::new(),
      #[cfg(all(unix, not(target_os = "macos")))]
      config_dir_override: None,
    }
  }

  pub fn query(&self) -> AutostartView {
    platform_query(self)
  }

  fn target_for_mode(&self, mode: AutostartMode) -> Option<String> {
    match mode {
      AutostartMode::Disabled => None,
      AutostartMode::Daemon => self.daemon_command().ok(),
    }
  }

  fn daemon_command(&self) -> io::Result<String> {
    let daemon = self.daemon_path.as_ref().ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::NotFound,
        "could not resolve cadderd executable for autostart",
      )
    })?;
    let background_arg = daemon_background_arg();
    Ok(format!("\"{}\"{background_arg}", daemon.display()))
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  fn config_dir(&self) -> io::Result<PathBuf> {
    if let Some(config_dir) = &self.config_dir_override {
      return Ok(config_dir.clone());
    }
    self
      .base_dirs
      .as_ref()
      .map(|dirs| dirs.config_dir().to_path_buf())
      .ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::Unsupported,
          "could not resolve config directory",
        )
      })
  }

  #[cfg(target_os = "macos")]
  fn home_dir(&self) -> io::Result<PathBuf> {
    self
      .base_dirs
      .as_ref()
      .map(|dirs| dirs.home_dir().to_path_buf())
      .ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::Unsupported,
          "could not resolve home directory",
        )
      })
  }
}

#[cfg(windows)]
fn platform_query(manager: &AutostartManager) -> AutostartView {
  let output = hidden_windows_command("reg")
    .args([
      "query",
      r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
      "/v",
      RUN_VALUE_NAME,
    ])
    .output();
  match output {
    Ok(output) if output.status.success() => AutostartView {
      mode: AutostartMode::Daemon,
      status: AutostartStatus::Enabled,
      target: manager.target_for_mode(AutostartMode::Daemon),
      diagnostics: Vec::new(),
    },
    _ => AutostartView {
      mode: AutostartMode::Disabled,
      status: AutostartStatus::Disabled,
      target: None,
      diagnostics: Vec::new(),
    },
  }
}

#[cfg(windows)]
fn hidden_windows_command(program: &str) -> Command {
  let mut command = Command::new(program);
  command.creation_flags(CREATE_NO_WINDOW);
  command
}

#[cfg(target_os = "macos")]
fn platform_query(manager: &AutostartManager) -> AutostartView {
  let daemon = macos_daemon_plist(manager);
  if daemon.is_file() {
    autostart_enabled(manager, AutostartMode::Daemon)
  } else {
    autostart_disabled()
  }
}

#[cfg(target_os = "macos")]
fn macos_daemon_plist(manager: &AutostartManager) -> PathBuf {
  manager
    .home_dir()
    .unwrap_or_else(|_| PathBuf::from("."))
    .join("Library")
    .join("LaunchAgents")
    .join(format!("{APP_ID}.daemon.plist"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_query(manager: &AutostartManager) -> AutostartView {
  match linux_autostart_paths(manager) {
    Ok(paths) => query_linux_autostart(manager, &paths),
    Err(error) => AutostartView {
      mode: AutostartMode::Disabled,
      status: AutostartStatus::Unsupported,
      target: None,
      diagnostics: vec![AutostartDiagnostic {
        code: "autostart-config-dir-unavailable".to_string(),
        message: error.to_string(),
      }],
    },
  }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn query_linux_autostart(manager: &AutostartManager, paths: &LinuxAutostartPaths) -> AutostartView {
  let daemon_unit_exists = paths.daemon_service.is_file();
  let enablement = linux_enablement_state(paths);

  if !daemon_unit_exists && !enablement.exists() {
    autostart_disabled()
  } else {
    let mode = AutostartMode::Daemon;
    let target = manager.target_for_mode(mode);
    let mut diagnostics = Vec::new();

    if !daemon_unit_exists {
      diagnostics.push(autostart_diagnostic(
        "autostart-daemon-unit-missing",
        "Cadder found startup enablement without the generated cadderd.service unit.",
      ));
    }

    match enablement {
      LinuxEnablementState::Valid => {}
      LinuxEnablementState::Missing if daemon_unit_exists => {
        diagnostics.push(autostart_diagnostic(
          "autostart-daemon-not-enabled",
          "Cadder found cadderd.service, but it is not enabled under default.target.",
        ))
      }
      LinuxEnablementState::Missing => {}
      LinuxEnablementState::Invalid { code, message } => {
        diagnostics.push(autostart_diagnostic(code, message));
      }
    }

    if target.is_none() {
      diagnostics.push(autostart_diagnostic(
        "autostart-target-missing",
        "Cadder could not resolve the configured autostart target.",
      ));
    }

    AutostartView {
      mode,
      status: if diagnostics.is_empty() {
        AutostartStatus::Enabled
      } else {
        AutostartStatus::Misconfigured
      },
      target,
      diagnostics,
    }
  }
}

#[cfg(all(unix, not(target_os = "macos")))]
#[derive(Debug, Clone)]
struct LinuxAutostartPaths {
  daemon_service: PathBuf,
  daemon_enablement_link: PathBuf,
}

#[cfg(all(unix, not(target_os = "macos")))]
impl LinuxAutostartPaths {
  fn from_config_dir(config_dir: PathBuf) -> Self {
    let systemd_user_dir = config_dir.join("systemd").join("user");
    Self {
      daemon_service: systemd_user_dir.join("cadderd.service"),
      daemon_enablement_link: systemd_user_dir
        .join("default.target.wants")
        .join("cadderd.service"),
    }
  }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_autostart_paths(manager: &AutostartManager) -> io::Result<LinuxAutostartPaths> {
  manager
    .config_dir()
    .map(LinuxAutostartPaths::from_config_dir)
}

#[cfg(all(unix, not(target_os = "macos")))]
#[derive(Debug, Clone, Copy)]
enum LinuxEnablementState {
  Missing,
  Valid,
  Invalid {
    code: &'static str,
    message: &'static str,
  },
}

#[cfg(all(unix, not(target_os = "macos")))]
impl LinuxEnablementState {
  fn exists(self) -> bool {
    !matches!(self, Self::Missing)
  }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_enablement_state(paths: &LinuxAutostartPaths) -> LinuxEnablementState {
  match fs::symlink_metadata(&paths.daemon_enablement_link) {
    Ok(metadata) if metadata.file_type().is_symlink() => {
      match fs::read_link(&paths.daemon_enablement_link) {
        Ok(target)
          if target == linux_daemon_enablement_target() || target == paths.daemon_service =>
        {
          LinuxEnablementState::Valid
        }
        Ok(_) => LinuxEnablementState::Invalid {
          code: "autostart-daemon-enablement-stale",
          message: "Cadder found a default.target enablement link that does not point at the generated cadderd.service unit.",
        },
        Err(_) => LinuxEnablementState::Invalid {
          code: "autostart-daemon-enablement-unreadable",
          message: "Cadder could not read the cadderd.service enablement link.",
        },
      }
    }
    Ok(_) => LinuxEnablementState::Invalid {
      code: "autostart-daemon-enablement-not-symlink",
      message: "Cadder found a default.target cadderd.service entry that is not a symlink.",
    },
    Err(error) if error.kind() == io::ErrorKind::NotFound => LinuxEnablementState::Missing,
    Err(_) => LinuxEnablementState::Invalid {
      code: "autostart-daemon-enablement-unreadable",
      message: "Cadder could not inspect the cadderd.service enablement link.",
    },
  }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_daemon_enablement_target() -> PathBuf {
  PathBuf::from("..").join("cadderd.service")
}

#[cfg(not(any(windows, unix)))]
fn platform_query(_manager: &AutostartManager) -> AutostartView {
  AutostartView {
    mode: AutostartMode::Disabled,
    status: AutostartStatus::Unsupported,
    target: None,
    diagnostics: vec![AutostartDiagnostic {
      code: "autostart-unsupported".to_string(),
      message: "Autostart is not supported on this platform.".to_string(),
    }],
  }
}

#[cfg(target_os = "macos")]
fn autostart_enabled(manager: &AutostartManager, mode: AutostartMode) -> AutostartView {
  let target = manager.target_for_mode(mode);
  let mut diagnostics = Vec::new();
  if target.is_none() {
    diagnostics.push(AutostartDiagnostic {
      code: "autostart-target-missing".to_string(),
      message: "Cadder could not resolve the configured autostart target.".to_string(),
    });
  }
  AutostartView {
    mode,
    status: if diagnostics.is_empty() {
      AutostartStatus::Enabled
    } else {
      AutostartStatus::Misconfigured
    },
    target,
    diagnostics,
  }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn autostart_diagnostic(code: &str, message: &str) -> AutostartDiagnostic {
  AutostartDiagnostic {
    code: code.to_string(),
    message: message.to_string(),
  }
}

#[cfg(unix)]
fn autostart_disabled() -> AutostartView {
  AutostartView {
    mode: AutostartMode::Disabled,
    status: AutostartStatus::Disabled,
    target: None,
    diagnostics: Vec::new(),
  }
}

#[cfg(all(test, unix))]
fn write_parented(path: &Path, contents: String) -> io::Result<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)?;
  }
  fs::write(path, contents)
}

#[cfg(all(test, unix, not(target_os = "macos")))]
fn write_parented_symlink(target: PathBuf, link: &Path) -> io::Result<()> {
  if let Some(parent) = link.parent() {
    fs::create_dir_all(parent)?;
  }
  match fs::remove_file(link) {
    Ok(()) => {}
    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
    Err(error) => return Err(error),
  }
  unix_fs::symlink(target, link)
}

#[cfg(all(test, unix, not(target_os = "macos")))]
fn systemd_user_service(command: &str) -> String {
  format!(
    "[Unit]\nDescription=Cadder daemon\n\n[Service]\nExecStart={command}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n"
  )
}

#[cfg(windows)]
fn daemon_background_arg() -> &'static str {
  " --background"
}

#[cfg(not(windows))]
fn daemon_background_arg() -> &'static str {
  ""
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;

  fn manager_with_targets() -> AutostartManager {
    AutostartManager {
      daemon_path: Some(PathBuf::from("D:/bin/cadderd.exe")),
      #[cfg(unix)]
      base_dirs: None,
      #[cfg(all(unix, not(target_os = "macos")))]
      config_dir_override: None,
    }
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  fn linux_manager(config_dir: PathBuf) -> AutostartManager {
    AutostartManager {
      daemon_path: Some(PathBuf::from("/opt/cadder/bin/cadderd")),
      base_dirs: None,
      config_dir_override: Some(config_dir),
    }
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  fn diagnostic_codes(view: &AutostartView) -> Vec<&str> {
    view
      .diagnostics
      .iter()
      .map(|diagnostic| diagnostic.code.as_str())
      .collect()
  }

  #[test]
  fn command_targets_include_portable_daemon_executable() {
    let manager = manager_with_targets();
    let expected_daemon = if cfg!(windows) {
      "\"D:/bin/cadderd.exe\" --background"
    } else {
      "\"D:/bin/cadderd.exe\""
    };

    assert_eq!(manager.daemon_command().unwrap(), expected_daemon);
    assert_eq!(
      manager.target_for_mode(AutostartMode::Daemon).as_deref(),
      Some(expected_daemon)
    );
    assert_eq!(manager.target_for_mode(AutostartMode::Disabled), None);
  }

  #[test]
  fn missing_targets_report_not_found_before_platform_write() {
    let manager = AutostartManager::disabled();

    assert_eq!(
      manager.daemon_command().unwrap_err().kind(),
      io::ErrorKind::NotFound
    );
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  #[test]
  fn systemd_unit_contains_daemon_command() {
    let unit = systemd_user_service("\"/bin/cadderd\"");

    assert!(unit.contains("ExecStart=\"/bin/cadderd\""));
    assert!(unit.contains("Restart=on-failure"));
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  #[test]
  fn linux_query_reports_unavailable_config_dir_as_unsupported() {
    let view = AutostartManager::disabled().query();

    assert_eq!(view.mode, AutostartMode::Disabled);
    assert_eq!(view.status, AutostartStatus::Unsupported);
    assert_eq!(
      diagnostic_codes(&view),
      vec!["autostart-config-dir-unavailable"]
    );
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  #[test]
  fn linux_query_reports_generated_unit_without_enablement_as_misconfigured() {
    let temp = tempfile::tempdir().unwrap();
    let manager = linux_manager(temp.path().join("config"));
    let paths = linux_autostart_paths(&manager).unwrap();
    write_parented(
      &paths.daemon_service,
      systemd_user_service("\"/opt/cadder/bin/cadderd\""),
    )
    .unwrap();

    let view = manager.query();

    assert_eq!(view.mode, AutostartMode::Daemon);
    assert_eq!(view.status, AutostartStatus::Misconfigured);
    assert_eq!(
      diagnostic_codes(&view),
      vec!["autostart-daemon-not-enabled"]
    );
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  #[test]
  fn linux_query_reports_stale_enablement_link_as_misconfigured() {
    let temp = tempfile::tempdir().unwrap();
    let manager = linux_manager(temp.path().join("config"));
    let paths = linux_autostart_paths(&manager).unwrap();
    write_parented(
      &paths.daemon_service,
      systemd_user_service("\"/opt/cadder/bin/cadderd\""),
    )
    .unwrap();
    write_parented_symlink(
      PathBuf::from("..").join("other.service"),
      &paths.daemon_enablement_link,
    )
    .unwrap();

    let view = manager.query();

    assert_eq!(view.mode, AutostartMode::Daemon);
    assert_eq!(view.status, AutostartStatus::Misconfigured);
    assert_eq!(
      diagnostic_codes(&view),
      vec!["autostart-daemon-enablement-stale"]
    );
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  #[test]
  fn linux_query_accepts_absolute_enablement_link_to_generated_unit() {
    let temp = tempfile::tempdir().unwrap();
    let manager = linux_manager(temp.path().join("config"));
    let paths = linux_autostart_paths(&manager).unwrap();
    write_parented(
      &paths.daemon_service,
      systemd_user_service("\"/opt/cadder/bin/cadderd\""),
    )
    .unwrap();
    write_parented_symlink(paths.daemon_service.clone(), &paths.daemon_enablement_link).unwrap();

    let view = manager.query();

    assert_eq!(view.mode, AutostartMode::Daemon);
    assert_eq!(view.status, AutostartStatus::Enabled);
    assert!(view.diagnostics.is_empty());
  }
}
