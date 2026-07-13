use crate::paths::RuntimePaths;
use cadder_protocol::{AutostartDiagnostic, AutostartMode, AutostartStatus};
#[cfg(unix)]
use directories::BaseDirs;
#[cfg(all(unix, not(target_os = "macos")))]
use std::os::unix::fs as unix_fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::process::Command;
use std::{env, io, path::PathBuf};
#[cfg(unix)]
use std::{fs, path::Path};

#[cfg(target_os = "macos")]
const APP_ID: &str = "dev.cadder";
#[cfg(windows)]
const RUN_VALUE_NAME: &str = "Cadder";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone)]
pub struct AutostartManager {
  runtime_dir: PathBuf,
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
      runtime_dir: PathBuf::new(),
      daemon_path: None,
      #[cfg(unix)]
      base_dirs: None,
      #[cfg(all(unix, not(target_os = "macos")))]
      config_dir_override: None,
    }
  }

  pub fn new(paths: &RuntimePaths) -> Self {
    let daemon_path = env::current_exe().ok();
    Self {
      runtime_dir: paths.runtime_dir().to_path_buf(),
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

  pub fn set(&self, mode: AutostartMode) -> AutostartView {
    let result = match mode {
      AutostartMode::Disabled => platform_disable(self),
      AutostartMode::Daemon => platform_set_daemon(self),
    };
    match result {
      Ok(()) => self.query(),
      Err(error) => AutostartView {
        mode,
        status: AutostartStatus::Unsupported,
        target: self.target_for_mode(mode),
        diagnostics: vec![AutostartDiagnostic {
          code: "autostart-update-failed".to_string(),
          message: error.to_string(),
        }],
      },
    }
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
    Ok(format!(
      "\"{}\"{background_arg} --runtime-dir \"{}\"",
      daemon.display(),
      self.runtime_dir.display()
    ))
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
fn platform_disable(_manager: &AutostartManager) -> io::Result<()> {
  let _ = hidden_windows_command("reg")
    .args([
      "delete",
      r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
      "/v",
      RUN_VALUE_NAME,
      "/f",
    ])
    .status();
  Ok(())
}

#[cfg(windows)]
fn platform_set_daemon(manager: &AutostartManager) -> io::Result<()> {
  write_windows_run_value(&manager.daemon_command()?)
}

#[cfg(windows)]
fn write_windows_run_value(command: &str) -> io::Result<()> {
  let status = hidden_windows_command("reg")
    .args([
      "add",
      r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
      "/v",
      RUN_VALUE_NAME,
      "/t",
      "REG_SZ",
      "/d",
      command,
      "/f",
    ])
    .status()?;
  if status.success() {
    Ok(())
  } else {
    Err(io::Error::other("reg.exe rejected the autostart update"))
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
fn platform_disable(manager: &AutostartManager) -> io::Result<()> {
  remove_file_if_exists(macos_daemon_plist(manager))
}

#[cfg(target_os = "macos")]
fn platform_set_daemon(manager: &AutostartManager) -> io::Result<()> {
  platform_disable(manager)?;
  let path = macos_daemon_plist(manager);
  write_parented(&path, macos_plist(APP_ID, &manager.daemon_command()?))
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
fn platform_disable(manager: &AutostartManager) -> io::Result<()> {
  let paths = linux_autostart_paths(manager)?;
  remove_linux_autostart_files(&paths)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn remove_linux_autostart_files(paths: &LinuxAutostartPaths) -> io::Result<()> {
  remove_file_if_exists(paths.daemon_enablement_link.clone())?;
  remove_file_if_exists(paths.daemon_service.clone())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_set_daemon(manager: &AutostartManager) -> io::Result<()> {
  let daemon_command = manager.daemon_command()?;
  let paths = linux_autostart_paths(manager)?;
  remove_linux_autostart_files(&paths)?;
  write_linux_daemon_autostart(&paths, &daemon_command)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn write_linux_daemon_autostart(paths: &LinuxAutostartPaths, command: &str) -> io::Result<()> {
  write_parented(&paths.daemon_service, systemd_user_service(command))?;
  write_parented_symlink(
    linux_daemon_enablement_target(),
    &paths.daemon_enablement_link,
  )
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

#[cfg(not(any(windows, unix)))]
fn platform_disable(_manager: &AutostartManager) -> io::Result<()> {
  Ok(())
}

#[cfg(not(any(windows, unix)))]
fn platform_set_daemon(_manager: &AutostartManager) -> io::Result<()> {
  Err(io::Error::new(
    io::ErrorKind::Unsupported,
    "autostart is not supported on this platform",
  ))
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

#[cfg(unix)]
fn write_parented(path: &Path, contents: String) -> io::Result<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)?;
  }
  fs::write(path, contents)
}

#[cfg(all(unix, not(target_os = "macos")))]
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

#[cfg(unix)]
fn remove_file_if_exists(path: PathBuf) -> io::Result<()> {
  match fs::remove_file(path) {
    Ok(()) => Ok(()),
    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error),
  }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn systemd_user_service(command: &str) -> String {
  format!(
    "[Unit]\nDescription=Cadder daemon\n\n[Service]\nExecStart={command}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n"
  )
}

#[cfg(target_os = "macos")]
fn macos_plist(label: &str, command: &str) -> String {
  format!(
    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array><string>/bin/sh</string><string>-lc</string><string>{command}</string></array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#
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
      runtime_dir: PathBuf::from("D:/runtime"),
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
      runtime_dir: PathBuf::from("/tmp/cadder-runtime"),
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

  #[cfg(all(unix, not(target_os = "macos")))]
  fn linux_manager_without_daemon(config_dir: PathBuf) -> AutostartManager {
    AutostartManager {
      runtime_dir: PathBuf::from("/tmp/cadder-runtime"),
      daemon_path: None,
      base_dirs: None,
      config_dir_override: Some(config_dir),
    }
  }

  #[test]
  fn command_targets_include_runtime_dir_and_executable() {
    let manager = manager_with_targets();
    let expected_daemon = if cfg!(windows) {
      "\"D:/bin/cadderd.exe\" --background --runtime-dir \"D:/runtime\""
    } else {
      "\"D:/bin/cadderd.exe\" --runtime-dir \"D:/runtime\""
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

  #[test]
  fn missing_targets_make_set_fail_before_platform_write() {
    let manager = AutostartManager::disabled();

    let daemon = manager.set(AutostartMode::Daemon);

    assert_eq!(daemon.mode, AutostartMode::Daemon);
    assert_eq!(daemon.status, AutostartStatus::Unsupported);
    assert_eq!(daemon.target, None);
    assert_eq!(daemon.diagnostics[0].code, "autostart-update-failed");
    assert!(
      daemon.diagnostics[0]
        .message
        .contains("could not resolve cadderd executable")
    );
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  #[test]
  fn systemd_unit_contains_daemon_command() {
    let unit = systemd_user_service("\"/bin/cadderd\" --runtime-dir \"/tmp/cadder\"");

    assert!(unit.contains("ExecStart=\"/bin/cadderd\" --runtime-dir \"/tmp/cadder\""));
    assert!(unit.contains("Restart=on-failure"));
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  #[test]
  fn linux_set_daemon_creates_unit_and_enablement_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let manager = linux_manager(temp.path().join("config"));
    let paths = linux_autostart_paths(&manager).unwrap();

    let view = manager.set(AutostartMode::Daemon);

    assert_eq!(view.mode, AutostartMode::Daemon);
    assert_eq!(view.status, AutostartStatus::Enabled);
    assert!(view.diagnostics.is_empty());
    assert!(paths.daemon_service.is_file());
    assert_eq!(
      fs::read_link(&paths.daemon_enablement_link).unwrap(),
      linux_daemon_enablement_target()
    );
    let unit = fs::read_to_string(&paths.daemon_service).unwrap();
    assert!(unit.contains("WantedBy=default.target"));
    assert!(
      unit.contains("ExecStart=\"/opt/cadder/bin/cadderd\" --runtime-dir \"/tmp/cadder-runtime\"")
    );
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
  fn linux_set_daemon_validates_target_before_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let manager = linux_manager_without_daemon(temp.path().join("config"));
    let paths = linux_autostart_paths(&manager).unwrap();
    write_parented(
      &paths.daemon_service,
      systemd_user_service("\"/opt/cadder/bin/cadderd\" --runtime-dir \"/tmp/cadder-runtime\""),
    )
    .unwrap();
    write_parented_symlink(
      linux_daemon_enablement_target(),
      &paths.daemon_enablement_link,
    )
    .unwrap();

    let view = manager.set(AutostartMode::Daemon);

    assert_eq!(view.mode, AutostartMode::Daemon);
    assert_eq!(view.status, AutostartStatus::Unsupported);
    assert_eq!(view.diagnostics[0].code, "autostart-update-failed");
    assert!(paths.daemon_service.is_file());
    assert!(paths.daemon_enablement_link.exists());
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  #[test]
  fn linux_query_reports_generated_unit_without_enablement_as_misconfigured() {
    let temp = tempfile::tempdir().unwrap();
    let manager = linux_manager(temp.path().join("config"));
    let paths = linux_autostart_paths(&manager).unwrap();
    write_parented(
      &paths.daemon_service,
      systemd_user_service("\"/opt/cadder/bin/cadderd\" --runtime-dir \"/tmp/cadder-runtime\""),
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
      systemd_user_service("\"/opt/cadder/bin/cadderd\" --runtime-dir \"/tmp/cadder-runtime\""),
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
      systemd_user_service("\"/opt/cadder/bin/cadderd\" --runtime-dir \"/tmp/cadder-runtime\""),
    )
    .unwrap();
    write_parented_symlink(paths.daemon_service.clone(), &paths.daemon_enablement_link).unwrap();

    let view = manager.query();

    assert_eq!(view.mode, AutostartMode::Daemon);
    assert_eq!(view.status, AutostartStatus::Enabled);
    assert!(view.diagnostics.is_empty());
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  #[test]
  fn linux_disable_removes_daemon_enablement_and_unit() {
    let temp = tempfile::tempdir().unwrap();
    let manager = linux_manager(temp.path().join("config"));
    let paths = linux_autostart_paths(&manager).unwrap();

    manager.set(AutostartMode::Daemon);
    let view = manager.set(AutostartMode::Disabled);

    assert_eq!(view.mode, AutostartMode::Disabled);
    assert_eq!(view.status, AutostartStatus::Disabled);
    assert!(!paths.daemon_service.exists());
    assert!(!paths.daemon_enablement_link.exists());
  }
}
