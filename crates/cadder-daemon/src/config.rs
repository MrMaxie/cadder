use anyhow::{Context, Result};
use figment::{
  Figment,
  providers::{Env, Format, Toml},
};
use serde::Deserialize;
use std::{env, path::Path};

pub const CONFIG_FILE_NAME: &str = "cadder.toml";

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CadderConfig {
  pub caddy: CaddyRuntimeConfig,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CaddyRuntimeConfig {
  pub real_command: Option<String>,
}

impl CadderConfig {
  pub fn from_file(path: &Path) -> Result<Self> {
    Figment::new()
      .merge(Toml::file(path))
      .extract()
      .with_context(|| format!("load Cadder configuration from {}", path.display()))
  }

  pub fn from_environment() -> Result<Self> {
    let mut config: Self = Figment::new()
      .merge(Env::prefixed("CADDER_").split("__"))
      .extract()
      .context("load Cadder configuration from environment variables")?;

    if let Some(command) = env_value("CADDER_CADDY__REAL_COMMAND") {
      config.caddy.real_command = Some(command);
    }
    if let Some(command) = env_value("CADDER_CADDY_REAL_COMMAND") {
      config.caddy.real_command = Some(command);
    }

    Ok(config)
  }
}

pub fn configured_real_caddy_command(config: &CadderConfig) -> Option<String> {
  config
    .caddy
    .real_command
    .as_deref()
    .map(str::trim)
    .filter(|command| !command.is_empty())
    .map(ToOwned::to_owned)
}

fn env_value(key: &str) -> Option<String> {
  env::var(key)
    .ok()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{ffi::OsString, fs};

  const ENV_KEYS: [&str; 2] = ["CADDER_CADDY__REAL_COMMAND", "CADDER_CADDY_REAL_COMMAND"];

  struct EnvSnapshot {
    values: Vec<(&'static str, Option<OsString>)>,
  }

  impl EnvSnapshot {
    fn capture(keys: &[&'static str]) -> Self {
      Self {
        values: keys
          .iter()
          .copied()
          .map(|key| (key, env::var_os(key)))
          .collect(),
      }
    }
  }

  impl Drop for EnvSnapshot {
    fn drop(&mut self) {
      for (key, value) in &self.values {
        unsafe {
          match value {
            Some(value) => env::set_var(key, value),
            None => env::remove_var(key),
          }
        }
      }
    }
  }

  fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    crate::TEST_ENV_LOCK
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner())
  }

  #[test]
  fn reads_real_command_from_toml_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    fs::write(&path, "[caddy]\nreal_command = \"caddy-custom\"\n").unwrap();

    let config = CadderConfig::from_file(&path).unwrap();

    assert_eq!(
      configured_real_caddy_command(&config).as_deref(),
      Some("caddy-custom")
    );
  }

  #[test]
  fn ignores_blank_real_command_values() {
    let config = CadderConfig {
      caddy: CaddyRuntimeConfig {
        real_command: Some("  ".to_string()),
      },
    };

    assert_eq!(configured_real_caddy_command(&config), None);
  }

  #[test]
  fn trims_configured_real_command_values() {
    let config = CadderConfig {
      caddy: CaddyRuntimeConfig {
        real_command: Some("  caddy-custom  ".to_string()),
      },
    };

    assert_eq!(
      configured_real_caddy_command(&config).as_deref(),
      Some("caddy-custom")
    );
  }

  #[test]
  fn from_environment_reads_split_caddy_real_command() {
    let _lock = lock_env();
    let _snapshot = EnvSnapshot::capture(&ENV_KEYS);
    unsafe {
      env::remove_var("CADDER_CADDY_REAL_COMMAND");
      env::set_var("CADDER_CADDY__REAL_COMMAND", "env-caddy");
    }

    let config = CadderConfig::from_environment().unwrap();

    assert_eq!(
      configured_real_caddy_command(&config).as_deref(),
      Some("env-caddy")
    );
  }

  #[test]
  fn from_environment_legacy_real_command_overrides_split_key() {
    let _lock = lock_env();
    let _snapshot = EnvSnapshot::capture(&ENV_KEYS);
    unsafe {
      env::set_var("CADDER_CADDY__REAL_COMMAND", "split-caddy");
      env::set_var("CADDER_CADDY_REAL_COMMAND", "  legacy-caddy  ");
    }

    let config = CadderConfig::from_environment().unwrap();

    assert_eq!(
      configured_real_caddy_command(&config).as_deref(),
      Some("legacy-caddy")
    );
  }

  #[test]
  fn from_environment_ignores_blank_real_command_overrides() {
    let _lock = lock_env();
    let _snapshot = EnvSnapshot::capture(&ENV_KEYS);
    unsafe {
      env::set_var("CADDER_CADDY__REAL_COMMAND", "   ");
      env::set_var("CADDER_CADDY_REAL_COMMAND", "   ");
    }

    let config = CadderConfig::from_environment().unwrap();

    assert_eq!(configured_real_caddy_command(&config), None);
  }

  #[test]
  fn from_file_reports_invalid_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    fs::write(&path, "[caddy\n").unwrap();

    let error = CadderConfig::from_file(&path).unwrap_err();

    assert!(error.to_string().contains("load Cadder configuration from"));
  }
}
