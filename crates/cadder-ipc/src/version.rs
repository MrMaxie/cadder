use serde::{Deserialize, Deserializer, Serialize, de};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolVersion {
  major: u16,
  minor: u16,
}

impl ProtocolVersion {
  pub const fn new(major: u16, minor: u16) -> Result<Self, ProtocolVersionError> {
    if major == 0 {
      return Err(ProtocolVersionError::ZeroMajor);
    }
    Ok(Self { major, minor })
  }

  pub const fn major(self) -> u16 {
    self.major
  }

  pub const fn minor(self) -> u16 {
    self.minor
  }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct WireVersion {
      major: u16,
      minor: u16,
    }

    let version = WireVersion::deserialize(deserializer)?;
    Self::new(version.major, version.minor).map_err(de::Error::custom)
  }
}

impl fmt::Display for ProtocolVersion {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(formatter, "{}.{}", self.major, self.minor)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolVersionError {
  ZeroMajor,
}

impl fmt::Display for ProtocolVersionError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str("a protocol version major number must be greater than zero")
  }
}

impl std::error::Error for ProtocolVersionError {}

pub const PROTOCOL_VERSION_1_0: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };
pub const CURRENT_PROTOCOL_VERSION: ProtocolVersion = PROTOCOL_VERSION_1_0;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn protocol_version_is_closed_and_rejects_zero() {
    assert!(ProtocolVersion::new(0, 1).is_err());
    assert!(serde_json::from_str::<ProtocolVersion>(r#"{"major":0,"minor":1}"#).is_err());
    assert!(
      serde_json::from_str::<ProtocolVersion>(r#"{"major":1,"minor":0,"future":true}"#).is_err()
    );
  }
}
