use cadder_ipc::*;
use serde_json::Value;

#[test]
fn v1_contract_contains_exactly_eight_operations() {
  let names = OPERATION_REGISTRY
    .iter()
    .map(|operation| operation.name())
    .collect::<Vec<_>>();

  assert_eq!(names, message_types::REQUEST_MESSAGE_TYPES);
  assert_eq!(names.len(), 8);
  for removed in [
    "subscribe-state-request",
    "query-history-request",
    "query-autostart-request",
    "set-autostart-request",
  ] {
    assert!(!names.contains(&removed));
  }
}

#[test]
fn v1_request_header_and_mutation_payload_are_closed() {
  let unknown_header = r#"{"protocolVersion":{"major":1,"minor":0},"operation":"shutdown-daemon-request","requestId":"closed-1","payload":{},"future":true}"#;
  assert!(serde_json::from_str::<RawRequestEnvelope>(unknown_header).is_err());

  let unknown_payload = r#"{"protocolVersion":{"major":1,"minor":0},"operation":"shutdown-daemon-request","requestId":"closed-2","payload":{"future":true}}"#;
  let raw: RawRequestEnvelope = serde_json::from_str(unknown_payload).unwrap();
  let error = OPERATION_REGISTRY
    .authorize_envelope(&raw)
    .unwrap()
    .decode::<ShutdownDaemonPayload>()
    .unwrap_err();
  assert_eq!(error.code.as_str(), "incompatible_payload");
}

#[test]
fn v1_response_requires_exactly_one_correlated_outcome() {
  let request_id = RequestId::parse("response-1").unwrap();
  let response = ResponseEnvelope::success(
    CURRENT_PROTOCOL_VERSION,
    message_types::QUERY_STATE_REQUEST,
    request_id.clone(),
    Value::Bool(true),
  );
  let decoded: ResponseEnvelope<Value> =
    serde_json::from_value(serde_json::to_value(response).unwrap()).unwrap();
  assert_eq!(decoded.request_id(), &request_id);

  let both = r#"{"protocolVersion":{"major":1,"minor":0},"operation":"query-state-request","requestId":"response-1","result":true,"error":{"kind":"internal","code":"internal","message":"failed","guidance":null,"retryable":false,"requestId":"response-1"}}"#;
  assert!(serde_json::from_str::<ResponseEnvelope<Value>>(both).is_err());
}

#[test]
fn v1_rejects_every_non_exact_protocol_version() {
  for (major, minor) in [(1, 1), (2, 0)] {
    let raw: RawRequestEnvelope = serde_json::from_str(&format!(
      r#"{{"protocolVersion":{{"major":{major},"minor":{minor}}},"operation":"query-state-request","requestId":"state-{major}-{minor}","payload":{{"requestId":"state-{major}-{minor}"}}}}"#
    ))
    .unwrap();
    assert!(OPERATION_REGISTRY.authorize_envelope(&raw).is_err());
  }
}
