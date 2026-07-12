use crate::{ProtocolError, ProtocolVersion, RequestId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// A closed operation request sent after a successful handshake.
pub struct RequestEnvelope<T> {
  pub protocol_version: ProtocolVersion,
  pub operation: Box<str>,
  pub request_id: RequestId,
  pub payload: T,
}

impl<T> RequestEnvelope<T> {
  /// Creates a request with a validated version and correlation ID.
  pub fn new(
    protocol_version: ProtocolVersion,
    operation: impl Into<Box<str>>,
    request_id: RequestId,
    payload: T,
  ) -> Self {
    Self {
      protocol_version,
      operation: operation.into(),
      request_id,
      payload,
    }
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
