//! Standalone frames used before versioned operation envelopes are available.

use crate::{ProtocolError, ProtocolVersion, RequestId};
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
  pub protocol_version: ProtocolVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
/// The additive successful handshake payload returned by the daemon.
pub struct ServerHello {
  pub request_id: RequestId,
  pub runtime_id: Box<str>,
  pub daemon_instance_id: Box<str>,
  pub protocol_version: ProtocolVersion,
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
  /// Returns the runtime selected by the local endpoint.
  pub fn runtime_id(&self) -> &str {
    &self.runtime_id
  }

  /// Returns the daemon instance that rejected the handshake.
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
    Ok(Self {
      runtime_id: rejection.runtime_id,
      daemon_instance_id: rejection.daemon_instance_id,
      error: rejection.error,
    })
  }
}

#[cfg(test)]
mod tests;
