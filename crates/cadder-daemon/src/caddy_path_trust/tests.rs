use super::*;
#[cfg(windows)]
use std::env;
use std::fs;

#[test]
fn trusted_caddy_source_requires_absolute_executable_path() {
  let error = validate_caddy_executable(Path::new("caddy")).unwrap_err();

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

  let error = validate_caddy_executable(&script).unwrap_err();

  assert!(error.to_string().contains("batch script"));
}

#[cfg(windows)]
#[test]
fn trusted_caddy_source_windows_accepts_system_executable() {
  let executable = PathBuf::from(env::var_os("SystemRoot").unwrap())
    .join("System32")
    .join("where.exe");

  let canonical = validate_caddy_executable(&executable).unwrap();

  assert_eq!(canonical, fs::canonicalize(executable).unwrap());
}

#[cfg(windows)]
#[test]
fn trusted_caddy_source_windows_accepts_user_writable_config() {
  let temp = tempfile::tempdir().unwrap();
  let config = temp.path().join("config.toml");
  fs::write(&config, "[defaults]").unwrap();

  let config_file = open_caddy_config(&config).unwrap();

  assert_eq!(
    config_file.canonical_path(),
    fs::canonicalize(config).unwrap()
  );
  assert!(config_file.into_file().metadata().unwrap().is_file());
}

#[cfg(windows)]
#[test]
fn trusted_caddy_source_windows_accepts_user_writable_ancestor() {
  let temp = tempfile::tempdir().unwrap();
  let executable = temp.path().join("caddy.exe");
  fs::write(&executable, b"fixture").unwrap();

  let canonical = validate_caddy_executable(&executable).unwrap();

  assert_eq!(canonical, fs::canonicalize(executable).unwrap());
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

  let error = validate_caddy_executable(&alias).unwrap_err();

  assert!(error.to_string().contains("reparse point"));
}

#[cfg(unix)]
fn executable_fixture() -> (tempfile::TempDir, PathBuf) {
  use std::os::unix::fs::PermissionsExt;

  let temp = tempfile::tempdir().unwrap();
  let executable = temp.path().join("executable");
  fs::write(&executable, b"fixture").unwrap();
  fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
  (temp, executable)
}

#[cfg(unix)]
#[test]
fn trusted_caddy_source_unix_accepts_executable() {
  let (_temp, executable) = executable_fixture();

  let canonical = validate_caddy_executable(&executable).unwrap();

  assert_eq!(canonical, fs::canonicalize(executable).unwrap());
}

#[cfg(unix)]
#[test]
fn trusted_caddy_source_unix_rejects_non_executable_file() {
  use std::os::unix::fs::PermissionsExt;

  let (_temp, executable) = executable_fixture();
  fs::set_permissions(&executable, fs::Permissions::from_mode(0o600)).unwrap();

  let error = validate_caddy_executable(&executable).unwrap_err();

  assert!(error.to_string().contains("not executable"));
}

#[cfg(unix)]
#[test]
fn trusted_caddy_source_unix_same_file_identity_follows_symlink() {
  use std::os::unix::fs::symlink;

  let (_temp, executable) = executable_fixture();
  let alias = executable.with_file_name("caddy-alias");
  symlink(&executable, &alias).unwrap();

  assert!(same_file_identity(&executable, &alias).unwrap());
}
