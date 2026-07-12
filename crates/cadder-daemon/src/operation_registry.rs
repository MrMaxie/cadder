use cadder_protocol::{
  IpcEnvelope, OPERATION_REGISTRY, OperationDefinition, ProtocolError, ProtocolResult, RequestId,
};
use serde::de::DeserializeOwned;

pub(crate) fn authorize_legacy(
  envelope: &IpcEnvelope,
) -> ProtocolResult<AuthorizedLegacyEnvelope<'_>> {
  let definition = OPERATION_REGISTRY.authorize_legacy(
    &envelope.message_type,
    envelope.protocol_version,
    envelope.capabilities.as_ref(),
  )?;
  Ok(AuthorizedLegacyEnvelope {
    definition,
    envelope,
    request_id: envelope
      .payload
      .get("requestId")
      .and_then(|value| value.as_str())
      .and_then(|request_id| RequestId::parse(request_id).ok()),
  })
}

#[derive(Debug)]
pub(crate) struct AuthorizedLegacyEnvelope<'a> {
  definition: &'static OperationDefinition,
  envelope: &'a IpcEnvelope,
  request_id: Option<RequestId>,
}

impl AuthorizedLegacyEnvelope<'_> {
  pub(crate) fn definition(&self) -> &'static OperationDefinition {
    self.definition
  }

  pub(crate) fn decode<T>(&self) -> ProtocolResult<T>
  where
    T: DeserializeOwned,
  {
    serde_json::from_value(self.envelope.payload.clone())
      .map_err(ProtocolError::payload_decode_failed)
  }

  pub(crate) fn request_id(&self) -> Option<RequestId> {
    self.request_id.clone()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_protocol::{
    CURRENT_PROTOCOL_VERSION, CapabilityId, OperationAccess, OperationDeadlineClass,
    OperationShape, PROTOCOL_VERSION_1_0, ProtocolCapabilities, ProtocolErrorKind, ProtocolVersion,
    capabilities, message_types,
  };
  use serde::Deserialize;
  use std::collections::BTreeSet;

  #[derive(Debug, Deserialize, PartialEq, Eq)]
  #[serde(rename_all = "camelCase", deny_unknown_fields)]
  struct TestPayload {
    request_id: String,
    value: u16,
  }

  #[test]
  fn operation_registry_contains_every_request_once_and_no_response() {
    let registered = OPERATION_REGISTRY
      .iter()
      .map(|operation| operation.name())
      .collect::<BTreeSet<_>>();

    assert_eq!(registered.len(), message_types::REQUEST_MESSAGE_TYPES.len());
    assert_eq!(
      registered,
      message_types::REQUEST_MESSAGE_TYPES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
    );
    assert!(
      [
        message_types::QUERY_STATE_RESPONSE,
        message_types::STATE_CHANGED_EVENT,
        message_types::PROTOCOL_ERROR_RESPONSE,
      ]
      .into_iter()
      .all(|message| OPERATION_REGISTRY.lookup(message).is_none())
    );
    assert!(OPERATION_REGISTRY.iter().all(|operation| {
      CapabilityId::parse(operation.required_capability()).is_ok()
        && operation.minimum_version() == PROTOCOL_VERSION_1_0
    }));
  }

  #[test]
  fn operation_registry_classifies_mutations_streams_deadlines_and_retry() {
    let unary = OperationShape::Unary;
    let ordinary = OperationDeadlineClass::Ordinary;
    let reload = OperationDeadlineClass::Reload;
    let expected = [
      (
        message_types::REGISTER_ENTRYPOINT_REQUEST,
        capabilities::ENTRYPOINT_REGISTRATION,
        OperationAccess::Mutation,
        unary,
        reload,
        false,
      ),
      (
        message_types::UNREGISTER_ENTRYPOINT_REQUEST,
        capabilities::ENTRYPOINT_REGISTRATION,
        OperationAccess::Mutation,
        unary,
        reload,
        false,
      ),
      (
        message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
        capabilities::ENTRYPOINT_REGISTRATION,
        OperationAccess::Mutation,
        unary,
        ordinary,
        true,
      ),
      (
        message_types::QUERY_STATE_REQUEST,
        capabilities::RUNTIME_STATE,
        OperationAccess::ReadOnly,
        unary,
        ordinary,
        true,
      ),
      (
        message_types::SUBSCRIBE_STATE_REQUEST,
        capabilities::STATE_SUBSCRIPTION,
        OperationAccess::ReadOnly,
        OperationShape::ServerStream,
        OperationDeadlineClass::Stream,
        true,
      ),
      (
        message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
        capabilities::ACTIVATION_CONTROL,
        OperationAccess::Mutation,
        unary,
        reload,
        false,
      ),
      (
        message_types::SET_DOMAIN_ENABLED_REQUEST,
        capabilities::ACTIVATION_CONTROL,
        OperationAccess::Mutation,
        unary,
        reload,
        false,
      ),
      (
        message_types::QUERY_IIS_BINDINGS_REQUEST,
        capabilities::IIS_HANDOFF,
        OperationAccess::ReadOnly,
        unary,
        ordinary,
        true,
      ),
      (
        message_types::SET_IIS_HANDOFF_REQUEST,
        capabilities::IIS_HANDOFF,
        OperationAccess::Mutation,
        unary,
        reload,
        false,
      ),
      (
        message_types::QUERY_LOGS_REQUEST,
        capabilities::LOGS,
        OperationAccess::ReadOnly,
        unary,
        ordinary,
        true,
      ),
      (
        message_types::QUERY_HISTORY_REQUEST,
        capabilities::HISTORY,
        OperationAccess::ReadOnly,
        unary,
        ordinary,
        true,
      ),
      (
        message_types::QUERY_AUTOSTART_REQUEST,
        capabilities::AUTOSTART,
        OperationAccess::ReadOnly,
        unary,
        ordinary,
        true,
      ),
      (
        message_types::SET_AUTOSTART_REQUEST,
        capabilities::AUTOSTART,
        OperationAccess::Mutation,
        unary,
        ordinary,
        false,
      ),
      (
        message_types::SHUTDOWN_DAEMON_REQUEST,
        capabilities::DAEMON_LIFECYCLE,
        OperationAccess::Mutation,
        unary,
        OperationDeadlineClass::Shutdown,
        false,
      ),
    ];

    for (name, capability, access, shape, deadline, retryable) in expected {
      let operation = OPERATION_REGISTRY.lookup(name).unwrap();
      assert_eq!(operation.required_capability(), capability, "{name}");
      assert_eq!(operation.access(), access, "{name}");
      assert_eq!(operation.shape(), shape, "{name}");
      assert_eq!(operation.deadline(), deadline, "{name}");
      assert_eq!(operation.timeout_retryable(), retryable, "{name}");
    }
  }

  #[test]
  fn operation_registry_negotiates_stable_known_capabilities() {
    let requested = [
      CapabilityId::parse(capabilities::LOGS).unwrap(),
      CapabilityId::parse("future-surface").unwrap(),
      CapabilityId::parse(capabilities::RUNTIME_STATE).unwrap(),
    ];
    let negotiated = OPERATION_REGISTRY
      .negotiate_capabilities(CURRENT_PROTOCOL_VERSION, &requested)
      .unwrap();

    assert_eq!(
      negotiated
        .iter()
        .map(CapabilityId::as_str)
        .collect::<Vec<_>>(),
      vec![capabilities::LOGS, capabilities::RUNTIME_STATE]
    );
  }

  #[test]
  fn operation_registry_decodes_payload_only_after_gate() {
    let mut envelope = IpcEnvelope {
      protocol_version: 2,
      capabilities: Some(ProtocolCapabilities {
        protocol_version: 2,
        minimum_compatible_protocol_version: 1,
        supported_capabilities: Box::default(),
        supported_capability_versions: Box::default(),
      }),
      message_type: message_types::QUERY_LOGS_REQUEST.to_string(),
      payload: serde_json::json!({"requestId":"logs-1","value":"not-a-number"}),
    };
    let error = authorize_legacy(&envelope).unwrap_err();
    assert_eq!(error.kind, ProtocolErrorKind::UnsupportedCapability);

    envelope.capabilities = Some(ProtocolCapabilities::current());
    let authorized = authorize_legacy(&envelope).unwrap();
    let error = authorized.decode::<TestPayload>().unwrap_err();
    assert_eq!(error.kind, ProtocolErrorKind::PayloadDecodeFailed);

    envelope.payload = serde_json::json!({"requestId":"logs-1","value":7});
    let authorized = authorize_legacy(&envelope).unwrap();
    assert_eq!(
      authorized.request_id().as_ref().map(RequestId::as_str),
      Some("logs-1")
    );
    assert_eq!(
      authorized.definition().name(),
      message_types::QUERY_LOGS_REQUEST
    );
    assert_eq!(
      authorized.decode::<TestPayload>().unwrap(),
      TestPayload {
        request_id: "logs-1".to_string(),
        value: 7,
      }
    );
  }

  #[test]
  fn operation_registry_rejects_wrong_session_version_and_legacy_capability_first() {
    let unknown = OPERATION_REGISTRY
      .authorize(
        "unknown-operation",
        CURRENT_PROTOCOL_VERSION,
        &OPERATION_REGISTRY
          .advertised_capabilities(CURRENT_PROTOCOL_VERSION)
          .unwrap(),
      )
      .unwrap_err();
    assert_eq!(unknown.code.as_str(), "unsupported_operation");
    assert_eq!(unknown.required_capability, None);

    let error = OPERATION_REGISTRY
      .authorize(
        message_types::QUERY_STATE_REQUEST,
        ProtocolVersion::new(2, 0).unwrap(),
        &[CapabilityId::parse(capabilities::RUNTIME_STATE).unwrap()],
      )
      .unwrap_err();
    assert_eq!(error.kind, ProtocolErrorKind::IncompatibleProtocolVersion);
    let future_minor = OPERATION_REGISTRY
      .authorize(
        message_types::QUERY_STATE_REQUEST,
        ProtocolVersion::new(1, 1).unwrap(),
        &[CapabilityId::parse(capabilities::RUNTIME_STATE).unwrap()],
      )
      .unwrap_err();
    assert_eq!(
      future_minor.kind,
      ProtocolErrorKind::IncompatibleProtocolVersion
    );

    let legacy = IpcEnvelope {
      protocol_version: 2,
      capabilities: Some(ProtocolCapabilities {
        protocol_version: 2,
        minimum_compatible_protocol_version: 1,
        supported_capabilities: Box::default(),
        supported_capability_versions: Box::default(),
      }),
      message_type: message_types::QUERY_LOGS_REQUEST.to_string(),
      payload: serde_json::json!({"value":"invalid"}),
    };
    let error = authorize_legacy(&legacy).unwrap_err();
    assert_eq!(error.kind, ProtocolErrorKind::UnsupportedCapability);

    let mut too_old = legacy;
    too_old.protocol_version = 0;
    let error = authorize_legacy(&too_old).unwrap_err();
    assert_eq!(error.kind, ProtocolErrorKind::IncompatibleProtocolVersion);
  }
}
