use super::*;
use std::fs;

#[test]
fn real_caddy_reads_command_configuration() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join(CONFIG_FILE_NAME);
  fs::write(&path, "[caddy]\nreal_command = 'caddy-real'\n").unwrap();

  let config = CadderConfig::from_file(&path).unwrap();

  assert_eq!(
    config.real_caddy().unwrap(),
    Some(RealCaddySelection::Command("caddy-real".to_string()))
  );
}

#[test]
fn real_caddy_reads_path_configuration() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join(CONFIG_FILE_NAME);
  fs::write(&path, "[caddy]\nreal_path = '/default/caddy'\n").unwrap();

  let config = CadderConfig::from_file(&path).unwrap();

  assert_eq!(
    config.real_caddy().unwrap(),
    Some(RealCaddySelection::Path(PathBuf::from("/default/caddy")))
  );
}

#[test]
fn real_caddy_rejects_removed_defaults_configuration() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join(CONFIG_FILE_NAME);
  fs::write(&path, "[defaults]\nreal_caddy = '/default/caddy'\n").unwrap();

  assert!(CadderConfig::from_file(&path).is_err());
}

#[test]
fn real_caddy_rejects_command_and_path_together() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join(CONFIG_FILE_NAME);
  fs::write(
    &path,
    "[caddy]\nreal_command = 'caddy-real'\nreal_path = '/default/caddy'\n",
  )
  .unwrap();

  let config = CadderConfig::from_file(&path).unwrap();
  let error = config.real_caddy().unwrap_err();

  assert!(error.to_string().contains("cannot both be configured"));
}

#[test]
fn real_caddy_rejects_unknown_configuration_fields() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join(CONFIG_FILE_NAME);
  fs::write(&path, "[caddy]\nunknown = 'caddy'\n").unwrap();

  let error = CadderConfig::from_file(&path).unwrap_err();

  assert!(error.to_string().contains("load Cadder configuration from"));
}

#[test]
fn trusted_caddy_source_reports_invalid_toml() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join(CONFIG_FILE_NAME);
  fs::write(&path, "[defaults\n").unwrap();

  let error = CadderConfig::from_file(&path).unwrap_err();

  assert!(error.to_string().contains("load Cadder configuration from"));
}
