use serde::{Deserialize, Deserializer, Serialize, de};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
/// A validated major/minor control-plane protocol version.
pub struct ProtocolVersion {
  major: u16,
  minor: u16,
}

impl ProtocolVersion {
  /// Creates a version and rejects the reserved major version zero.
  pub fn new(major: u16, minor: u16) -> Result<Self, ProtocolVersionError> {
    if major == 0 {
      return Err(ProtocolVersionError::ZeroMajor);
    }
    Ok(Self { major, minor })
  }

  /// Returns the compatibility-breaking version component.
  pub const fn major(self) -> u16 {
    self.major
  }

  /// Returns the additive compatibility version component.
  pub const fn minor(self) -> u16 {
    self.minor
  }

  /// Reports whether two versions can participate in minor-version negotiation.
  pub const fn is_same_major(self, other: Self) -> bool {
    self.major == other.major
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
/// A protocol-version validation error.
pub enum ProtocolVersionError {
  /// Major version zero is reserved and cannot appear on the wire.
  ZeroMajor,
}

impl fmt::Display for ProtocolVersionError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str("a protocol version major number must be greater than zero")
  }
}

impl std::error::Error for ProtocolVersionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
/// An inclusive, single-major range offered during the handshake.
pub struct ProtocolVersionRange {
  minimum: ProtocolVersion,
  maximum: ProtocolVersion,
}

impl ProtocolVersionRange {
  /// Creates a range containing one validated version.
  pub const fn exact(version: ProtocolVersion) -> Self {
    Self {
      minimum: version,
      maximum: version,
    }
  }

  /// Creates an ordered range that cannot cross a major-version boundary.
  pub fn new(
    minimum: ProtocolVersion,
    maximum: ProtocolVersion,
  ) -> Result<Self, VersionRangeError> {
    if minimum.major() != maximum.major() {
      return Err(VersionRangeError::DifferentMajors);
    }
    if minimum > maximum {
      return Err(VersionRangeError::Inverted);
    }
    Ok(Self { minimum, maximum })
  }

  /// Selects the highest mutually supported version, or returns `None` when ranges do not overlap.
  pub fn negotiate(self, other: Self) -> Option<ProtocolVersion> {
    if self.maximum.major() != other.maximum.major() {
      return None;
    }
    let minimum = self.minimum.max(other.minimum);
    let maximum = self.maximum.min(other.maximum);
    (minimum <= maximum).then_some(maximum)
  }

  /// Returns the inclusive lower bound.
  pub const fn minimum(self) -> ProtocolVersion {
    self.minimum
  }

  /// Returns the inclusive upper bound.
  pub const fn maximum(self) -> ProtocolVersion {
    self.maximum
  }
}

impl<'de> Deserialize<'de> for ProtocolVersionRange {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct WireRange {
      minimum: ProtocolVersion,
      maximum: ProtocolVersion,
    }

    let range = WireRange::deserialize(deserializer)?;
    Self::new(range.minimum, range.maximum).map_err(de::Error::custom)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// A protocol-version range validation error.
pub enum VersionRangeError {
  /// The range attempts to span compatibility-breaking major versions.
  DifferentMajors,
  /// The minimum version is greater than the maximum version.
  Inverted,
}

impl fmt::Display for VersionRangeError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(match self {
      Self::DifferentMajors => "a protocol range cannot span major versions",
      Self::Inverted => "the minimum protocol version exceeds the maximum",
    })
  }
}

impl std::error::Error for VersionRangeError {}

/// The version that introduces the Cadder 1.0 operation set.
pub const PROTOCOL_VERSION_1_0: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };
/// The protocol version implemented by this build.
pub const CURRENT_PROTOCOL_VERSION: ProtocolVersion = PROTOCOL_VERSION_1_0;
/// The inclusive protocol range implemented by this build.
pub const SUPPORTED_PROTOCOL_VERSIONS: ProtocolVersionRange =
  ProtocolVersionRange::exact(CURRENT_PROTOCOL_VERSION);

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn protocol_version_negotiates_only_an_overlapping_major_range() {
    let client = ProtocolVersionRange::new(
      ProtocolVersion::new(1, 0).unwrap(),
      ProtocolVersion::new(1, 4).unwrap(),
    )
    .unwrap();
    let daemon = ProtocolVersionRange::new(
      ProtocolVersion::new(1, 2).unwrap(),
      ProtocolVersion::new(1, 3).unwrap(),
    )
    .unwrap();

    assert_eq!(
      client.negotiate(daemon),
      Some(ProtocolVersion::new(1, 3).unwrap())
    );
    assert_eq!(
      client.negotiate(ProtocolVersionRange::exact(
        ProtocolVersion::new(2, 0).unwrap()
      )),
      None
    );
    assert!(
      ProtocolVersionRange::new(
        ProtocolVersion::new(1, 2).unwrap(),
        ProtocolVersion::new(1, 1).unwrap()
      )
      .is_err()
    );
    assert!(ProtocolVersion::new(0, 1).is_err());
    assert!(serde_json::from_str::<ProtocolVersion>(r#"{"major":0,"minor":1}"#).is_err());
    assert!(
      serde_json::from_str::<ProtocolVersionRange>(
        r#"{"minimum":{"major":1,"minor":2},"maximum":{"major":1,"minor":1}}"#
      )
      .is_err()
    );
  }
}
