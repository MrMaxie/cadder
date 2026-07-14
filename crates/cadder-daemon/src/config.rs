use anyhow::{Context, Result};
use figment::{
  Figment,
  providers::{Format, Toml},
};
use serde::Deserialize;
use std::{
  fs::File,
  io::Read,
  path::{Path, PathBuf},
};

pub const CONFIG_FILE_NAME: &str = "cadder.toml";

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct CadderConfig {
  pub caddy: CaddyConfig,
  pub defaults: RuntimeConfig,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct CaddyConfig {
  pub real_command: Option<String>,
  pub real_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeConfig {
  pub real_caddy: Option<PathBuf>,
}

impl CadderConfig {
  pub fn from_file(path: &Path) -> Result<Self> {
    let file = File::open(path)
      .with_context(|| format!("open Cadder configuration from {}", path.display()))?;
    Self::from_reader(file, path)
  }

  pub fn from_reader(mut reader: impl Read, source: &Path) -> Result<Self> {
    let mut contents = String::new();
    reader
      .read_to_string(&mut contents)
      .with_context(|| format!("read Cadder configuration from {}", source.display()))?;
    Figment::new()
      .merge(Toml::string(&contents))
      .extract()
      .with_context(|| format!("load Cadder configuration from {}", source.display()))
  }

  pub(crate) fn real_caddy(&self) -> Result<Option<RealCaddySelection>> {
    match (
      &self.caddy.real_command,
      &self.caddy.real_path,
      &self.defaults.real_caddy,
    ) {
      (Some(_), Some(_), _) => Err(anyhow::anyhow!(
        "caddy.real_command and caddy.real_path cannot both be configured"
      )),
      (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => Err(anyhow::anyhow!(
        "[caddy] configuration and legacy defaults.real_caddy cannot both be configured"
      )),
      (Some(command), None, None) => Ok(Some(RealCaddySelection::Command(command.clone()))),
      (None, Some(path), None) => Ok(Some(RealCaddySelection::Path(path.clone()))),
      (None, None, Some(path)) => Ok(Some(RealCaddySelection::Path(path.clone()))),
      (None, None, None) => Ok(None),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RealCaddySelection {
  Command(String),
  Path(PathBuf),
}

#[cfg(test)]
mod tests {
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
  fn real_caddy_reads_legacy_path_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    fs::write(&path, "[defaults]\nreal_caddy = '/default/caddy'\n").unwrap();

    let config = CadderConfig::from_file(&path).unwrap();

    assert_eq!(
      config.real_caddy().unwrap(),
      Some(RealCaddySelection::Path(PathBuf::from("/default/caddy")))
    );
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
}
