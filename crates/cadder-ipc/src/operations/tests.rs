use super::*;
use crate::{ProtocolErrorKind, ShutdownDaemonPayload};

#[test]
fn registry_contains_only_the_eight_v1_operations() {
  let names = OPERATION_REGISTRY
    .iter()
    .map(|operation| operation.name())
    .collect::<Vec<_>>();
  assert_eq!(names, message_types::REQUEST_MESSAGE_TYPES);
  assert_eq!(names.len(), 8);
}

#[test]
fn exact_protocol_version_is_required() {
  let raw: RawRequestEnvelope = serde_json::from_str(
    r#"{"protocolVersion":{"major":1,"minor":1},"operation":"query-state-request","requestId":"state-1","payload":{"requestId":"state-1"}}"#,
  )
  .unwrap();
  assert_eq!(
    OPERATION_REGISTRY
      .authorize_envelope(&raw)
      .unwrap_err()
      .kind,
    ProtocolErrorKind::IncompatibleProtocolVersion
  );
}

#[test]
fn authorized_envelope_uses_the_operation_owned_decoder() {
  let raw: RawRequestEnvelope = serde_json::from_str(
    r#"{"protocolVersion":{"major":1,"minor":0},"operation":"shutdown-daemon-request","requestId":"shutdown-1","payload":{}}"#,
  )
  .unwrap();
  OPERATION_REGISTRY
    .authorize_envelope(&raw)
    .unwrap()
    .decode::<ShutdownDaemonPayload>()
    .unwrap();
}
