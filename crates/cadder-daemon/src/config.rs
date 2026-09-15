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
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct CaddyConfig {
  pub real_command: Option<String>,
  pub real_path: Option<PathBuf>,
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
    let command = &self.caddy.real_command;
    let path = &self.caddy.real_path;

    if command.is_some() && path.is_some() {
      anyhow::bail!("caddy.real_command and caddy.real_path cannot both be configured");
    }

    if let Some(command) = command {
      return Ok(Some(RealCaddySelection::Command(command.clone())));
    }

    Ok(path.as_ref().cloned().map(RealCaddySelection::Path))
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RealCaddySelection {
  Command(String),
  Path(PathBuf),
}

#[cfg(test)]
mod tests;
