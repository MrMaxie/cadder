use crate::paths::RuntimeProfile;
use anyhow::{Context, Result};
use figment::{
  Figment,
  providers::{Format, Toml},
};
use serde::Deserialize;
use std::{
  collections::BTreeMap,
  fs::File,
  io::Read,
  path::{Path, PathBuf},
};

pub const CONFIG_FILE_NAME: &str = "cadder.toml";

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct CadderConfig {
  pub defaults: RuntimeConfig,
  pub profiles: BTreeMap<String, RuntimeConfig>,
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

  pub fn real_caddy_for_profile(&self, profile: RuntimeProfile) -> Option<&Path> {
    self
      .profiles
      .get(profile.as_str())
      .and_then(|config| config.real_caddy.as_deref())
      .or(self.defaults.real_caddy.as_deref())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;

  #[test]
  fn trusted_caddy_source_reads_profile_before_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    fs::write(
      &path,
      "[defaults]\nreal_caddy = '/default/caddy'\n[profiles.dev]\nreal_caddy = '/dev/caddy'\n",
    )
    .unwrap();

    let config = CadderConfig::from_file(&path).unwrap();

    assert_eq!(
      config.real_caddy_for_profile(RuntimeProfile::Dev),
      Some(Path::new("/dev/caddy"))
    );
    assert_eq!(
      config.real_caddy_for_profile(RuntimeProfile::Default),
      Some(Path::new("/default/caddy"))
    );
  }

  #[test]
  fn trusted_caddy_source_rejects_unknown_configuration_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    fs::write(&path, "[defaults]\nreal_command = 'caddy'\n").unwrap();

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
