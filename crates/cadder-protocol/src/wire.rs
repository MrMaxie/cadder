use crate::{CapabilityId, ProtocolError, ProtocolResult, ProtocolVersion, RequestId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, de::DeserializeOwned};
use serde_json::value::RawValue;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// A closed operation request sent after a successful handshake.
pub struct RequestEnvelope<T> {
  protocol_version: ProtocolVersion,
  operation: Box<str>,
  request_id: RequestId,
  /// Payload extensions used by this request. The daemon gates them before typed decoding.
  payload_capabilities: Box<[CapabilityId]>,
  payload: T,
}

pub(crate) mod operation_payload_sealed {
  pub trait Sealed {}
}

/// A protocol-owned payload bound to one request operation and extension set.
///
/// The trait is sealed so a dispatcher cannot substitute an ad hoc permissive decoder for a
/// closed wire contract.
pub trait OperationPayload: operation_payload_sealed::Sealed + DeserializeOwned {
  /// The only operation that may decode this payload type.
  const OPERATION: &'static str;
  /// Every payload extension that this decoder understands and requires in the request header.
  const PAYLOAD_CAPABILITIES: &'static [&'static str] = &[];
  /// Identifies an unknown enum or union discriminator without classifying ordinary bad input as
  /// a version mismatch.
  fn incompatible_discriminator(_payload: &serde_json::Value) -> Option<String> {
    None
  }
}

impl<T> RequestEnvelope<T>
where
  T: OperationPayload,
{
  /// Creates a request whose operation and extension header come from its sealed payload type.
  pub fn new(protocol_version: ProtocolVersion, request_id: RequestId, payload: T) -> Self {
    Self {
      protocol_version,
      operation: T::OPERATION.into(),
      request_id,
      payload_capabilities: T::PAYLOAD_CAPABILITIES
        .iter()
        .map(|capability| CapabilityId::known(capability))
        .collect(),
      payload,
    }
  }

  /// Returns the protocol version selected for this request.
  pub const fn protocol_version(&self) -> ProtocolVersion {
    self.protocol_version
  }

  /// Returns the operation fixed by the sealed payload type.
  pub fn operation(&self) -> &str {
    &self.operation
  }

  /// Returns the request correlation ID.
  pub fn request_id(&self) -> &RequestId {
    &self.request_id
  }

  /// Returns the payload capabilities fixed by the sealed payload type.
  pub fn payload_capabilities(&self) -> &[CapabilityId] {
    &self.payload_capabilities
  }

  /// Returns the typed payload.
  pub fn payload(&self) -> &T {
    &self.payload
  }
}

/// A closed request header that preserves the exact payload until authorization succeeds.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RawRequestEnvelope {
  protocol_version: ProtocolVersion,
  operation: Box<str>,
  request_id: RequestId,
  payload_capabilities: Box<[CapabilityId]>,
  payload: Box<RawValue>,
}

impl RawRequestEnvelope {
  /// Returns the protocol version selected for this request.
  pub const fn protocol_version(&self) -> ProtocolVersion {
    self.protocol_version
  }

  /// Returns the exact operation label.
  pub fn operation(&self) -> &str {
    &self.operation
  }

  /// Returns the correlation ID supplied by the client.
  pub fn request_id(&self) -> &RequestId {
    &self.request_id
  }

  /// Returns payload extensions that must be authorized before decoding.
  pub fn payload_capabilities(&self) -> &[CapabilityId] {
    &self.payload_capabilities
  }

  pub(crate) fn decode_payload<T>(&self, closed: bool) -> ProtocolResult<T>
  where
    T: OperationPayload,
  {
    if !closed {
      return serde_json::from_str(self.payload.get())
        .map_err(ProtocolError::payload_decode_failed);
    }

    let payload_value: serde_json::Value =
      serde_json::from_str(self.payload.get()).map_err(ProtocolError::payload_decode_failed)?;
    if let Some(path) = T::incompatible_discriminator(&payload_value) {
      return Err(ProtocolError::incompatible_payload_contract(Some(&path)));
    }

    let mut first_ignored = None;
    let mut deserializer = serde_json::Deserializer::from_str(self.payload.get());
    let decoded = serde_ignored::deserialize(&mut deserializer, |path| {
      if first_ignored.is_none() {
        first_ignored = Some(path.to_string());
      }
    });
    let value = match decoded {
      Ok(value) => value,
      Err(_) if first_ignored.is_some() => {
        return Err(ProtocolError::incompatible_payload_contract(
          first_ignored.as_deref(),
        ));
      }
      Err(error) => return Err(ProtocolError::payload_decode_failed(error)),
    };
    if deserializer.end().is_err() {
      return Err(ProtocolError::incompatible_payload_contract(None));
    }
    if let Some(path) = first_ignored {
      return Err(ProtocolError::incompatible_payload_contract(Some(&path)));
    }
    Ok(value)
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
/// The exactly-one result carried by a response envelope.
pub enum ResponseOutcome<T> {
  /// The operation completed with a typed result.
  Success(SuccessOutcome<T>),
  /// The operation returned a correlated typed error.
  Failure(FailureOutcome),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// A successful typed response value.
pub struct SuccessOutcome<T> {
  pub result: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// A failed typed response value.
pub struct FailureOutcome {
  pub error: ProtocolError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// An additive operation response that preserves version and request correlation.
///
/// Fields stay private so callers cannot create a result/error conflict or mismatched error ID.
pub struct ResponseEnvelope<T> {
  protocol_version: ProtocolVersion,
  operation: Box<str>,
  request_id: RequestId,
  outcome: ResponseOutcome<T>,
}

impl<T> Serialize for ResponseEnvelope<T>
where
  T: Serialize,
{
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct WireResponse<'a, T> {
      protocol_version: ProtocolVersion,
      operation: &'a str,
      request_id: &'a RequestId,
      #[serde(skip_serializing_if = "Option::is_none")]
      result: Option<&'a T>,
      #[serde(skip_serializing_if = "Option::is_none")]
      error: Option<&'a ProtocolError>,
    }

    let (result, error) = match &self.outcome {
      ResponseOutcome::Success(outcome) => (Some(&outcome.result), None),
      ResponseOutcome::Failure(outcome) => (None, Some(&outcome.error)),
    };
    WireResponse {
      protocol_version: self.protocol_version,
      operation: &self.operation,
      request_id: &self.request_id,
      result,
      error,
    }
    .serialize(serializer)
  }
}

impl<'de, T> Deserialize<'de> for ResponseEnvelope<T>
where
  T: Deserialize<'de>,
{
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", bound(deserialize = "T: Deserialize<'de>"))]
    struct WireResponse<T> {
      protocol_version: ProtocolVersion,
      operation: Box<str>,
      request_id: RequestId,
      #[serde(default)]
      result: Present<T>,
      #[serde(default)]
      error: Present<ProtocolError>,
    }

    let response = WireResponse::deserialize(deserializer)?;
    let outcome = match (response.result, response.error) {
      (Present::Value(result), Present::Missing) => {
        ResponseOutcome::Success(SuccessOutcome { result })
      }
      (Present::Missing, Present::Value(error)) => {
        if error.request_id.as_ref() != Some(&response.request_id) {
          return Err(de::Error::custom(
            "the response and protocol error request IDs must match",
          ));
        }
        if error.has_legacy_version_metadata() {
          return Err(de::Error::custom(
            "a versioned response error cannot contain legacy protocol metadata",
          ));
        }
        ResponseOutcome::Failure(FailureOutcome { error })
      }
      (Present::Missing, Present::Missing) => {
        return Err(de::Error::custom(
          "a response must contain `result` or `error`",
        ));
      }
      (Present::Value(_), Present::Value(_)) => {
        return Err(de::Error::custom(
          "a response cannot contain both `result` and `error`",
        ));
      }
    };
    Ok(Self {
      protocol_version: response.protocol_version,
      operation: response.operation,
      request_id: response.request_id,
      outcome,
    })
  }
}

#[derive(Default)]
enum Present<T> {
  #[default]
  Missing,
  Value(T),
}

impl<'de, T> Deserialize<'de> for Present<T>
where
  T: Deserialize<'de>,
{
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    T::deserialize(deserializer).map(Self::Value)
  }
}

impl<T> ResponseEnvelope<T> {
  /// Returns the negotiated protocol version.
  pub const fn protocol_version(&self) -> ProtocolVersion {
    self.protocol_version
  }

  /// Returns the operation label echoed by the daemon.
  pub fn operation(&self) -> &str {
    &self.operation
  }

  /// Returns the request ID echoed by the daemon.
  pub fn request_id(&self) -> &RequestId {
    &self.request_id
  }

  /// Returns the typed success or failure value.
  pub fn outcome(&self) -> &ResponseOutcome<T> {
    &self.outcome
  }

  /// Creates a correlated successful response.
  pub fn success(
    protocol_version: ProtocolVersion,
    operation: impl Into<Box<str>>,
    request_id: RequestId,
    result: T,
  ) -> Self {
    Self {
      protocol_version,
      operation: operation.into(),
      request_id,
      outcome: ResponseOutcome::Success(SuccessOutcome { result }),
    }
  }

  /// Creates a correlated failure and removes legacy flat-version metadata.
  pub fn failure(
    protocol_version: ProtocolVersion,
    operation: impl Into<Box<str>>,
    request_id: RequestId,
    error: ProtocolError,
  ) -> Self {
    Self {
      protocol_version,
      operation: operation.into(),
      request_id: request_id.clone(),
      outcome: ResponseOutcome::Failure(FailureOutcome {
        error: error.for_versioned_response(request_id),
      }),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{CURRENT_PROTOCOL_VERSION, ProtocolError};
  use serde_json::json;

  #[test]
  fn protocol_response_contains_exactly_one_correlated_outcome() {
    let request_id = RequestId::parse("request-1").unwrap();
    let success = ResponseEnvelope::success(
      CURRENT_PROTOCOL_VERSION,
      "query-state",
      request_id.clone(),
      json!({"state":"ready"}),
    );
    let failure = ResponseEnvelope::<serde_json::Value>::failure(
      CURRENT_PROTOCOL_VERSION,
      "query-state",
      request_id.clone(),
      ProtocolError::payload_decode_failed(
        serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
      ),
    );
    let success_json = serde_json::to_string(&success).unwrap();
    let failure_json = serde_json::to_string(&failure).unwrap();

    assert!(success_json.contains("\"result\""));
    assert!(!success_json.contains("\"error\""));
    assert!(failure_json.contains("\"error\""));
    assert!(!failure_json.contains("\"result\""));
    assert!(!failure_json.contains("\"currentProtocolVersion\":2"));
    assert_eq!(
      serde_json::from_str::<ResponseEnvelope<serde_json::Value>>(&failure_json)
        .unwrap()
        .request_id(),
      &request_id
    );
    assert!(
      serde_json::from_str::<ResponseEnvelope<serde_json::Value>>(
        r#"{"protocolVersion":{"major":1,"minor":0},"operation":"x","requestId":"r1","result":{},"error":{"kind":"internal","code":"internal","message":"x","guidance":null,"retryable":false,"requestId":"r1","protocolVersion":null,"minimumCompatibleProtocolVersion":null,"currentProtocolVersion":null,"requiredCapability":null,"supportedCapabilities":[],"supportedCapabilityVersions":[]}}"#
      )
      .is_err()
    );
    let mut mismatched_error: serde_json::Value = serde_json::from_str(&failure_json).unwrap();
    assert!(
      [
        "protocolVersion",
        "minimumCompatibleProtocolVersion",
        "currentProtocolVersion"
      ]
      .iter()
      .all(|field| mismatched_error["error"].get(field).is_none())
    );
    mismatched_error["error"]["requestId"] = json!("other-request");
    assert!(
      serde_json::from_value::<ResponseEnvelope<serde_json::Value>>(mismatched_error).is_err()
    );

    let duplicate_code = failure_json.replacen(
      "\"code\":\"invalid_payload\"",
      "\"code\":\"invalid_payload\",\"code\":\"internal\"",
      1,
    );
    assert!(serde_json::from_str::<ResponseEnvelope<serde_json::Value>>(&duplicate_code).is_err());
    let duplicate_request_id = failure_json.replacen(
      "\"retryable\":false,\"requestId\":\"request-1\"",
      "\"retryable\":false,\"requestId\":\"request-1\",\"requestId\":\"other-request\"",
      1,
    );
    assert!(
      serde_json::from_str::<ResponseEnvelope<serde_json::Value>>(&duplicate_request_id).is_err()
    );

    let mut legacy_metadata: serde_json::Value = serde_json::from_str(&failure_json).unwrap();
    legacy_metadata["error"]["currentProtocolVersion"] = json!(2);
    assert!(
      serde_json::from_value::<ResponseEnvelope<serde_json::Value>>(legacy_metadata).is_err()
    );
    let mut null_legacy_metadata: serde_json::Value = serde_json::from_str(&failure_json).unwrap();
    null_legacy_metadata["error"]["currentProtocolVersion"] = serde_json::Value::Null;
    assert!(
      serde_json::from_value::<ResponseEnvelope<serde_json::Value>>(null_legacy_metadata).is_err()
    );

    let null_result = r#"{"protocolVersion":{"major":1,"minor":0},"operation":"x","requestId":"r1","result":null,"futureField":true}"#;
    assert!(
      matches!(
        serde_json::from_str::<ResponseEnvelope<serde_json::Value>>(null_result)
          .unwrap()
          .outcome(),
        ResponseOutcome::Success(SuccessOutcome {
          result: serde_json::Value::Null
        })
      ),
      "a null result remains present and additive response fields are ignored"
    );
  }
}
