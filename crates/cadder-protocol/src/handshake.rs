//! Standalone frames used before versioned operation envelopes are available.

use crate::{CapabilityId, ProtocolError, ProtocolVersion, ProtocolVersionRange, RequestId};
use serde::{Deserialize, Deserializer, Serialize, de};

/// Operation label used for the first client frame on a connection.
pub const CLIENT_HELLO_OPERATION: &str = "client-hello";
/// Operation label used for the daemon's pre-negotiation response.
pub const SERVER_HELLO_OPERATION: &str = "server-hello";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// The closed first frame sent by a client before operation envelopes are valid.
pub struct ClientHello {
  pub request_id: RequestId,
  pub runtime_id: Box<str>,
  pub daemon_instance_id: Box<str>,
  pub supported_versions: ProtocolVersionRange,
  #[serde(default)]
  pub capabilities: Box<[CapabilityId]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
/// The additive successful handshake payload returned by the daemon.
pub struct ServerHello {
  pub request_id: RequestId,
  pub runtime_id: Box<str>,
  pub daemon_instance_id: Box<str>,
  pub selected_version: ProtocolVersion,
  #[serde(default)]
  pub capabilities: Box<[CapabilityId]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "data", rename_all = "camelCase")]
/// The daemon's standalone pre-negotiation response.
pub enum ServerHandshakeFrame {
  /// Negotiation succeeded and operation envelopes may follow.
  Accepted(ServerHello),
  /// Negotiation failed with a correlated typed error.
  Rejected(HandshakeRejection),
}

impl ServerHandshakeFrame {
  /// Wraps a successful handshake response.
  pub fn accepted(hello: ServerHello) -> Self {
    Self::Accepted(hello)
  }

  /// Builds a rejection and correlates the error to the client hello.
  pub fn rejected(
    request_id: RequestId,
    runtime_id: impl Into<Box<str>>,
    daemon_instance_id: impl Into<Box<str>>,
    error: ProtocolError,
  ) -> Self {
    Self::Rejected(HandshakeRejection {
      runtime_id: runtime_id.into(),
      daemon_instance_id: daemon_instance_id.into(),
      error: error.for_versioned_response(request_id),
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
/// A validated handshake rejection with no legacy flat-version metadata.
pub struct HandshakeRejection {
  runtime_id: Box<str>,
  daemon_instance_id: Box<str>,
  error: ProtocolError,
}

impl HandshakeRejection {
  /// Returns the runtime selected from discovery.
  pub fn runtime_id(&self) -> &str {
    &self.runtime_id
  }

  /// Returns the daemon instance selected from discovery.
  pub fn daemon_instance_id(&self) -> &str {
    &self.daemon_instance_id
  }

  /// Returns the correlated typed failure.
  pub fn error(&self) -> &ProtocolError {
    &self.error
  }
}

impl<'de> Deserialize<'de> for HandshakeRejection {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WireRejection {
      runtime_id: Box<str>,
      daemon_instance_id: Box<str>,
      error: ProtocolError,
    }

    let rejection = WireRejection::deserialize(deserializer)?;
    if rejection.error.request_id.is_none() {
      return Err(de::Error::custom(
        "a handshake rejection must contain a correlated request ID",
      ));
    }
    if rejection.error.has_legacy_version_metadata() {
      return Err(de::Error::custom(
        "a versioned handshake error cannot contain legacy protocol metadata",
      ));
    }
    Ok(Self {
      runtime_id: rejection.runtime_id,
      daemon_instance_id: rejection.daemon_instance_id,
      error: rejection.error,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{CURRENT_PROTOCOL_VERSION, SUPPORTED_PROTOCOL_VERSIONS};

  #[test]
  fn protocol_handshake_roundtrips_and_rejects_unknown_fields() {
    let hello = ClientHello {
      request_id: RequestId::parse("hello-1").unwrap(),
      runtime_id: "runtime-1".into(),
      daemon_instance_id: "instance-1".into(),
      supported_versions: SUPPORTED_PROTOCOL_VERSIONS,
      capabilities: vec![CapabilityId::parse("logs").unwrap()].into_boxed_slice(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let decoded: ClientHello = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded, hello);
    assert!(json.contains("\"major\":1"));
    assert_eq!(
      CURRENT_PROTOCOL_VERSION,
      ProtocolVersion::new(1, 0).unwrap()
    );
    assert!(
      serde_json::from_str::<ClientHello>(
        r#"{"requestId":"hello-1","runtimeId":"r","daemonInstanceId":"i","supportedVersions":{"minimum":{"major":1,"minor":0},"maximum":{"major":1,"minor":0}},"capabilities":[],"unexpected":true}"#
      )
      .is_err()
    );

    let server_with_additive_field = r#"{"requestId":"hello-1","runtimeId":"runtime-1","daemonInstanceId":"instance-1","selectedVersion":{"major":1,"minor":0},"capabilities":["logs"],"futureDiagnostic":"ready"}"#;
    assert!(serde_json::from_str::<ServerHello>(server_with_additive_field).is_ok());
  }

  #[test]
  fn protocol_handshake_rejects_an_incompatible_major_with_guidance() {
    let offered = ProtocolVersionRange::exact(ProtocolVersion::new(2, 0).unwrap());
    assert_eq!(offered.negotiate(SUPPORTED_PROTOCOL_VERSIONS), None);

    let frame = ServerHandshakeFrame::rejected(
      RequestId::parse("hello-major-2").unwrap(),
      "runtime-1",
      "instance-1",
      ProtocolError::incompatible_protocol_range(offered, SUPPORTED_PROTOCOL_VERSIONS),
    );
    let json = serde_json::to_string(&frame).unwrap();
    let decoded: ServerHandshakeFrame = serde_json::from_str(&json).unwrap();

    assert!(json.contains("\"status\":\"rejected\""));
    assert!(json.contains("\"code\":\"incompatible_protocol\""));
    assert!(!json.contains("currentProtocolVersion"));
    match decoded {
      ServerHandshakeFrame::Rejected(rejection) => {
        assert_eq!(rejection.runtime_id(), "runtime-1");
        assert_eq!(rejection.daemon_instance_id(), "instance-1");
        assert_eq!(
          rejection.error().request_id.as_ref().map(RequestId::as_str),
          Some("hello-major-2")
        );
        assert!(
          rejection
            .error()
            .guidance
            .as_deref()
            .is_some_and(|guidance| guidance.contains("Upgrade the older"))
        );
      }
      ServerHandshakeFrame::Accepted(_) => panic!("the incompatible major must be rejected"),
    }

    let newer_minor = ProtocolVersionRange::exact(ProtocolVersion::new(1, 1).unwrap());
    let minor_error =
      ProtocolError::incompatible_protocol_range(SUPPORTED_PROTOCOL_VERSIONS, newer_minor);
    assert!(minor_error.message.contains("does not overlap"));
    assert!(
      minor_error
        .guidance
        .as_deref()
        .is_some_and(|guidance| guidance.contains("Cadder client"))
    );
  }
}
