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
  let decoded_failure =
    serde_json::from_str::<ResponseEnvelope<serde_json::Value>>(&failure_json).unwrap();
  let decoded_error = decoded_failure.into_result().unwrap_err();
  assert_eq!(decoded_error.request_id.as_ref(), Some(&request_id));
  assert_eq!(
    success.clone().into_result().unwrap(),
    json!({"state":"ready"})
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
  assert!(serde_json::from_value::<ResponseEnvelope<serde_json::Value>>(mismatched_error).is_err());

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
  assert!(serde_json::from_value::<ResponseEnvelope<serde_json::Value>>(legacy_metadata).is_err());
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

#[test]
fn request_and_response_accessors_preserve_the_typed_wire_contract() {
  let request_id = RequestId::parse("request-2").unwrap();
  let request = RequestEnvelope::new(
    CURRENT_PROTOCOL_VERSION,
    request_id.clone(),
    crate::QueryStatePayload::default(),
  );
  assert_eq!(request.protocol_version(), CURRENT_PROTOCOL_VERSION);
  assert_eq!(
    request.operation(),
    crate::message_types::QUERY_STATE_REQUEST
  );
  assert_eq!(request.request_id(), &request_id);
  assert_eq!(request.payload(), &crate::QueryStatePayload::default());

  let raw: RawRequestEnvelope =
    serde_json::from_str(&serde_json::to_string(&request).unwrap()).unwrap();
  assert_eq!(raw.protocol_version(), CURRENT_PROTOCOL_VERSION);
  assert_eq!(raw.operation(), crate::message_types::QUERY_STATE_REQUEST);
  assert_eq!(raw.request_id(), &request_id);
  assert_eq!(
    raw
      .decode_payload::<crate::QueryStatePayload>(true)
      .unwrap(),
    crate::QueryStatePayload::default()
  );

  let success = ResponseEnvelope::success(
    CURRENT_PROTOCOL_VERSION,
    crate::message_types::QUERY_STATE_RESPONSE,
    request_id.clone(),
    42_u8,
  );
  assert_eq!(success.protocol_version(), CURRENT_PROTOCOL_VERSION);
  assert_eq!(
    success.operation(),
    crate::message_types::QUERY_STATE_RESPONSE
  );
  assert_eq!(success.request_id(), &request_id);
  assert!(matches!(success.outcome(), ResponseOutcome::Success(_)));
  assert!(matches!(
    success.clone().into_outcome(),
    ResponseOutcome::Success(_)
  ));

  let failure = ResponseEnvelope::<u8>::failure(
    CURRENT_PROTOCOL_VERSION,
    crate::message_types::QUERY_STATE_RESPONSE,
    request_id,
    ProtocolError::stale_instance(),
  );
  assert!(matches!(failure.outcome(), ResponseOutcome::Failure(_)));
  assert!(failure.into_result().is_err());
}
