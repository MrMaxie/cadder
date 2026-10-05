use super::*;
use crate::CURRENT_PROTOCOL_VERSION;

#[test]
fn exact_handshake_roundtrips_and_rejects_unknown_fields() {
  let hello = ClientHello {
    request_id: RequestId::parse("hello-1").unwrap(),
    runtime_id: "runtime-1".into(),
    protocol_version: CURRENT_PROTOCOL_VERSION,
  };
  let json = serde_json::to_string(&hello).unwrap();
  assert_eq!(serde_json::from_str::<ClientHello>(&json).unwrap(), hello);
  assert!(
    serde_json::from_str::<ClientHello>(
      r#"{"requestId":"hello-1","runtimeId":"r","protocolVersion":{"major":1,"minor":0},"unexpected":true}"#,
    )
    .is_err()
  );
}

#[test]
fn rejection_retains_runtime_identity_and_request_correlation() {
  let frame = ServerHandshakeFrame::rejected(
    RequestId::parse("hello-rejected-1").unwrap(),
    "runtime-1",
    "instance-1",
    ProtocolError::stale_instance(),
  );
  let decoded: ServerHandshakeFrame =
    serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();
  let ServerHandshakeFrame::Rejected(rejection) = decoded else {
    panic!("expected a rejected handshake");
  };
  assert_eq!(rejection.runtime_id(), "runtime-1");
  assert_eq!(rejection.daemon_instance_id(), "instance-1");
  assert_eq!(
    rejection.error().request_id.as_ref().map(RequestId::as_str),
    Some("hello-rejected-1")
  );
}
