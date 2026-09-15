#[cfg(any(test, debug_assertions))]
use crate::caddy_image::{MINIMUM_CADDY_VERSION, required_caddy_modules};
use crate::{
  caddy_image::{
    CADDY_COMPATIBILITY_PROBE_REVISION, CaddyImageSource, OpenedCaddyImage, PinnedCaddyImage,
    VerifiedCaddyImage,
  },
  caddy_path_trust::{open_caddy_config, same_file_identity, validate_caddy_executable},
  config::{CONFIG_FILE_NAME, CadderConfig, RealCaddySelection},
  logs::CaddyLogStore,
  paths::RuntimePaths,
  runtime::{CaddyRuntime, ProcessRuntime},
};
use anyhow::{Context, Result, anyhow};
use cadder_ipc::{
  ConfigApplyStatus, ConfigDiagnostic, ConfigState, EntrypointRegistration, LogAttributionKind,
  LogSeverity, RegisteredDomain,
};
use chrono::Utc;
use semver::Version;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
  collections::{BTreeMap, BTreeSet},
  env, fmt,
  path::{Path, PathBuf},
  process::Stdio,
  str::FromStr,
  sync::{Arc, OnceLock},
  time::Duration,
};
use tokio::sync::OnceCell;

pub const CADDER_CADDY_BACKEND_ENV: &str = "CADDER_CADDY_BACKEND";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CaddyBackendMode {
  #[default]
  Real,
  Mock,
}

impl CaddyBackendMode {
  pub fn from_env() -> Result<Self> {
    env::var(CADDER_CADDY_BACKEND_ENV)
      .ok()
      .map_or(Ok(Self::Real), |value| value.parse())
  }

  pub fn parse_cli(value: &str) -> std::result::Result<Self, String> {
    value.parse::<Self>().map_err(|error| error.to_string())
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::Real => "real",
      Self::Mock => "mock",
    }
  }
}

impl fmt::Display for CaddyBackendMode {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for CaddyBackendMode {
  type Err = anyhow::Error;

  fn from_str(value: &str) -> Result<Self> {
    match value.trim().to_ascii_lowercase().as_str() {
      "" | "real" | "default" => Ok(Self::Real),
      "mock" | "dev" => Ok(Self::Mock),
      other => Err(anyhow!(
        "unknown Cadder Caddy backend `{other}`; expected `real` or `mock`"
      )),
    }
  }
}

mod configuration;
mod resolver;

#[cfg(test)]
use configuration::*;
pub use configuration::{
  CaddyApplyAction, CaddyConfigAdapter, CaddyConfigCoordinator, CaddyRegistrationAdapter,
};
pub use resolver::RealCaddyResolver;
#[cfg(test)]
use resolver::*;

#[cfg(test)]
mod tests;
