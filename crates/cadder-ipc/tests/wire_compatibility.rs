use std::collections::{BTreeMap, BTreeSet};

use cadder_ipc::*;
use schemars::schema_for;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

const WIRE_CONTRACT: &str = include_str!("fixtures/wire-contract-v1.json");
const ADDITIVE_RESPONSE: &str = include_str!("fixtures/additive-response-v1.json");
const MUTATION_CONTRACTS: &str = include_str!("fixtures/mutation-contracts-v1.json");
const MUTATION_SCHEMAS: &str = include_str!("fixtures/mutation-schemas-v1.json");
const UNKNOWN_DISCRIMINATORS: &str = include_str!("fixtures/unknown-discriminators-v1.json");

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct AdditiveResult {
  accepted: bool,
  summary: AdditiveSummary,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct AdditiveSummary {
  status: String,
}

#[test]
fn wire_compatibility_retained_contract_matches_the_checked_fixture() {
  let expected: Value = serde_json::from_str(WIRE_CONTRACT).unwrap();
  let actual = current_wire_contract();

  assert_eq!(
    actual,
    expected,
    "wire contract drift requires an IPC-010 compatibility review; current contract:\n{}",
    serde_json::to_string_pretty(&actual).unwrap()
  );
}

#[test]
fn wire_compatibility_additive_responses_and_capabilities_preserve_known_data() {
  let fixture: Value = serde_json::from_str(ADDITIVE_RESPONSE).unwrap();
  let hello: ServerHello = serde_json::from_value(fixture["serverHello"].clone()).unwrap();
  assert_eq!(hello.selected_version, ProtocolVersion::new(1, 1).unwrap());
  assert_eq!(
    hello
      .capabilities
      .iter()
      .map(CapabilityId::as_str)
      .collect::<Vec<_>>(),
    vec![capabilities::AUTOSTART, "future-observability"]
  );
  assert_eq!(
    OPERATION_REGISTRY
      .negotiate_capabilities(CURRENT_PROTOCOL_VERSION, &hello.capabilities)
      .unwrap()
      .iter()
      .map(CapabilityId::as_str)
      .collect::<Vec<_>>(),
    vec![capabilities::AUTOSTART]
  );

  let response: ResponseEnvelope<AdditiveResult> =
    serde_json::from_value(fixture["response"].clone()).unwrap();
  assert_eq!(
    response.protocol_version(),
    ProtocolVersion::new(1, 1).unwrap()
  );
  assert_eq!(response.operation(), message_types::QUERY_AUTOSTART_REQUEST);
  assert_eq!(response.request_id().as_str(), "additive-response-1");
  assert!(matches!(
    response.outcome(),
    ResponseOutcome::Success(SuccessOutcome {
      result: AdditiveResult {
        accepted: true,
        summary: AdditiveSummary { status }
      }
    }) if status == "enabled"
  ));
}

#[test]
fn wire_compatibility_public_response_dtos_accept_recursive_additions() {
  let fixture: Value = serde_json::from_str(MUTATION_CONTRACTS).unwrap();
  let registration = fixture["canonical"]
    .as_array()
    .unwrap()
    .iter()
    .find(|frame| frame["operation"] == message_types::REGISTER_ENTRYPOINT_REQUEST)
    .unwrap()["payload"]["registration"]
    .clone();
  let snapshot = json!({
    "capturedAtUtc": "2026-01-01T12:01:00Z",
    "registrations": [registration],
    "runtime": {
      "status": "idle",
      "binaryPath": null,
      "version": null,
      "processId": null,
      "adminEndpoint": null,
      "diagnostics": [{"code": "ready", "message": "Ready.", "operation": null}]
    },
    "config": {
      "status": "idle",
      "lastAttemptedAtUtc": null,
      "lastSuccessfulReloadAtUtc": null,
      "effectiveConfigHash": null,
      "diagnostics": [{
        "code": "ready",
        "message": "Ready.",
        "domainKey": null,
        "sourceConfigPaths": []
      }]
    },
    "storage": {
      "backend": "files",
      "path": null,
      "schemaVersion": 1,
      "diagnostics": []
    }
  });
  let stream = json!({
    "streamId": "runtime-control",
    "domainKey": null,
    "channel": "control"
  });
  assert_additive_json::<RegisterEntrypointResponse>(json!({
    "requestId": "register-response-1",
    "accepted": true,
    "message": "Registered.",
    "registrationId": "shim-1"
  }));
  assert_additive_json::<BasicResponse>(json!({
    "requestId": "basic-response-1",
    "accepted": true,
    "message": "Completed."
  }));
  assert_additive_json::<QueryStateResponse>(json!({
    "requestId": "state-response-1",
    "accepted": true,
    "message": "State returned.",
    "snapshot": snapshot.clone()
  }));
  assert_additive_json::<StateChangedEvent>(json!({
    "requestId": "state-event-1",
    "sequenceNumber": 1,
    "changeKind": "snapshot",
    "snapshot": snapshot,
    "registrationId": "shim-1"
  }));
  assert_additive_json::<QueryHistoryResponse>(json!({
    "requestId": "history-response-1",
    "accepted": true,
    "message": "History returned.",
    "records": [{
      "sequenceNumber": 1,
      "timestampUtc": "2026-01-01T12:00:00Z",
      "kind": "runtime",
      "summary": "Started.",
      "registrationId": null,
      "domainKey": null,
      "payload": "retained-payload"
    }],
    "storage": {
      "backend": "files",
      "path": null,
      "schemaVersion": 1,
      "diagnostics": []
    }
  }));
  assert_additive_json::<QueryAutostartResponse>(json!({
    "requestId": "autostart-query-1",
    "accepted": true,
    "message": "Autostart returned.",
    "mode": "daemon",
    "status": "enabled",
    "target": "cadderd",
    "diagnostics": [{"code": "enabled", "message": "Enabled."}]
  }));
  assert_additive_json::<SetAutostartResponse>(json!({
    "requestId": "autostart-set-1",
    "accepted": true,
    "message": "Autostart enabled.",
    "mode": "daemon",
    "status": "enabled",
    "target": "cadderd",
    "diagnostics": [{"code": "enabled", "message": "Enabled."}]
  }));
  assert_additive_json::<QueryLogsResponse>(json!({
    "requestId": "logs-response-1",
    "accepted": true,
    "message": "Logs returned.",
    "stream": stream.clone(),
    "streamStatus": "active",
    "entries": [{
      "sequenceNumber": 1,
      "timestampUtc": "2026-01-01T12:00:00Z",
      "severity": "info",
      "stream": stream,
      "attributionKind": "runtimeControl",
      "entryKind": "lifecycle",
      "rawMessage": "Started.",
      "domainKey": null,
      "sourceRegistrationId": null,
      "sourceInstanceId": null,
      "operation": "start"
    }],
    "nextCursor": null,
    "hasGap": false,
    "hasMoreBefore": false,
    "truncatedByRetention": false
  }));
  assert_additive_value(ProtocolErrorResponse::rejected(
    Some(RequestId::parse("protocol-error-1").unwrap()),
    ProtocolError::incompatible_payload_contract(None),
  ));
  assert_additive_value(ServerHandshakeFrame::rejected(
    RequestId::parse("hello-rejected-1").unwrap(),
    "runtime-1",
    "daemon-1",
    ProtocolError::incompatible_protocol_range(
      ProtocolVersionRange::exact(ProtocolVersion::new(2, 0).unwrap()),
      SUPPORTED_PROTOCOL_VERSIONS,
    ),
  ));
}

#[test]
fn wire_compatibility_mutation_schemas_match_the_checked_fixture() {
  let expected: Value = serde_json::from_str(MUTATION_SCHEMAS).unwrap();
  let schemas = mutation_schemas();
  let schema_operations = schemas
    .as_object()
    .unwrap()
    .keys()
    .map(String::as_str)
    .collect::<BTreeSet<_>>();
  let registered_mutations = OPERATION_REGISTRY
    .iter()
    .filter(|operation| operation.access() == OperationAccess::Mutation)
    .map(|operation| operation.name())
    .collect::<BTreeSet<_>>();
  assert_eq!(schema_operations, registered_mutations);
  assert_eq!(
    schemas,
    expected,
    "mutation schema drift requires a payload capability and IPC-010 review; current schemas:\n{}",
    serde_json::to_string_pretty(&schemas).unwrap()
  );
  assert_closed_mutation_schemas(&schemas, "$");
}

#[test]
fn wire_compatibility_mutation_fixtures_are_complete_closed_and_roundtrip() {
  let fixture: Value = serde_json::from_str(MUTATION_CONTRACTS).unwrap();
  let capabilities = OPERATION_REGISTRY
    .advertised_capabilities(CURRENT_PROTOCOL_VERSION)
    .unwrap();
  let canonical = fixture["canonical"].as_array().unwrap();
  let fixture_operations = canonical
    .iter()
    .map(|frame| frame["operation"].as_str().unwrap())
    .collect::<BTreeSet<_>>();
  let registered_mutations = OPERATION_REGISTRY
    .iter()
    .filter(|operation| operation.access() == OperationAccess::Mutation)
    .map(|operation| operation.name())
    .collect::<BTreeSet<_>>();
  assert_eq!(fixture_operations, registered_mutations);

  for frame in canonical {
    assert_mutation_roundtrip(frame, &capabilities);

    let mut unknown = frame.clone();
    unknown["payload"]["futureField"] = json!(true);
    assert_mutation_incompatible(&unknown, &capabilities);
  }

  let registration = canonical
    .iter()
    .find(|frame| frame["operation"] == message_types::REGISTER_ENTRYPOINT_REQUEST)
    .unwrap();
  let mut unknown_registration_activation = registration.clone();
  unknown_registration_activation["requestId"] = json!("registration-activation-unknown-1");
  unknown_registration_activation["payload"]["registration"]["activationState"] = json!("paused");
  assert_mutation_incompatible(&unknown_registration_activation, &capabilities);

  let mut unknown_domain_activation = registration.clone();
  unknown_domain_activation["requestId"] = json!("domain-activation-unknown-1");
  unknown_domain_activation["payload"]["registration"]["registeredDomains"][0]["activationState"] =
    json!("warming");
  assert_mutation_incompatible(&unknown_domain_activation, &capabilities);

  assert_mutation_incompatible(&fixture["ungatedUnknownField"], &capabilities);
  assert_mutation_incompatible(&fixture["ungatedUnknownVariant"], &capabilities);
  assert_mutation_incompatible(&fixture["nestedUnknownField"], &capabilities);

  let duplicate = r#"{"protocolVersion":{"major":1,"minor":0},"operation":"set-autostart-request","requestId":"duplicate-field-1","payloadCapabilities":[],"payload":{"mode":"daemon","mode":"disabled"}}"#;
  let raw: RawRequestEnvelope = serde_json::from_str(duplicate).unwrap();
  let error = OPERATION_REGISTRY
    .authorize_envelope(&raw, &capabilities)
    .unwrap()
    .decode::<SetAutostartPayload>()
    .unwrap_err();
  assert_invalid_payload(&error, "duplicate-field-1");

  for malformed in [
    r#"{"protocolVersion":{"major":1,"minor":0},"operation":"set-autostart-request","requestId":"wrong-scalar-1","payloadCapabilities":[],"payload":{"mode":7}}"#,
    r#"{"protocolVersion":{"major":1,"minor":0},"operation":"set-autostart-request","requestId":"missing-field-1","payloadCapabilities":[],"payload":{}}"#,
    r#"{"protocolVersion":{"major":1,"minor":0},"operation":"register-entrypoint-request","requestId":"invalid-timestamp-1","payloadCapabilities":[],"payload":{"registration":{"registrationId":"shim-1","entrypointInstance":{"instanceId":"shim-1","startedAtUtc":"not-a-timestamp","shimSessionNonce":"nonce-1"},"sourceWorkingDirectory":{"raw":"/workspace","canonical":"/workspace"},"sourceConfigPath":{"raw":"/workspace/Caddyfile","canonical":"/workspace/Caddyfile"},"registeredDomains":[],"activationState":"active","ownerProcess":{"processId":1,"processStartTimeUtc":"2026-01-01T12:00:00Z","shimSessionNonce":"nonce-1","executablePath":null},"logStream":{"streamId":"entrypoint-shim-1","domainKey":null,"channel":"caddy"},"shimRun":null,"createdAtUtc":"2026-01-01T12:00:00Z","lastHeartbeatUtc":"2026-01-01T12:00:00Z"}}}"#,
  ] {
    let raw: RawRequestEnvelope = serde_json::from_str(malformed).unwrap();
    let error = OPERATION_REGISTRY
      .authorize_envelope(&raw, &capabilities)
      .unwrap();
    let error = match raw.operation() {
      message_types::SET_AUTOSTART_REQUEST => error.decode::<SetAutostartPayload>().unwrap_err(),
      message_types::REGISTER_ENTRYPOINT_REQUEST => {
        error.decode::<RegisterEntrypointPayload>().unwrap_err()
      }
      operation => panic!("unexpected malformed operation `{operation}`"),
    };
    assert_invalid_payload(&error, raw.request_id().as_str());
  }
}

#[test]
fn wire_compatibility_payload_extension_is_rejected_before_typed_decode() {
  let fixture: Value = serde_json::from_str(MUTATION_CONTRACTS).unwrap();
  let capabilities = OPERATION_REGISTRY
    .advertised_capabilities(CURRENT_PROTOCOL_VERSION)
    .unwrap();

  for key in ["declaredUnknownField", "declaredUnknownVariant"] {
    let raw = parse_raw_envelope(&fixture[key]);
    let error = OPERATION_REGISTRY
      .authorize_envelope(&raw, &capabilities)
      .unwrap_err();
    assert_eq!(error.kind, ProtocolErrorKind::UnsupportedCapability);
    assert_eq!(error.code.as_str(), "unsupported_capability");
    assert_eq!(
      error.required_capability.as_deref(),
      Some("scheduled-autostart")
    );
    assert_eq!(
      error.request_id.as_ref().map(RequestId::as_str),
      Some(raw.request_id().as_str())
    );
  }
}

#[test]
fn wire_compatibility_unknown_discriminators_are_not_reinterpreted() {
  let fixture: Value = serde_json::from_str(UNKNOWN_DISCRIMINATORS).unwrap();
  macro_rules! reject_unknown_enum {
    ($($enum_type:ty),+ $(,)?) => {
      $(assert!(
        serde_json::from_value::<$enum_type>(fixture["protocolErrorKind"].clone()).is_err(),
        "{} accepted an unknown wire discriminator",
        stringify!($enum_type)
      );)+
    };
  }
  reject_unknown_enum!(
    ActivationState,
    AutostartMode,
    AutostartModePayload,
    AutostartStatus,
    ConfigApplyStatus,
    HistoryKind,
    LogAttributionKind,
    LogEntryKind,
    LogSeverity,
    LogStreamStatus,
    ProtocolErrorKind,
    RegistrationActivationState,
    RuntimeStatus,
    StateChangeKind,
  );
  assert!(
    serde_json::from_value::<ServerHandshakeFrame>(fixture["serverHandshakeFrame"].clone())
      .is_err()
  );
  assert!(
    serde_json::from_value::<ResponseOutcome<Value>>(fixture["responseOutcome"].clone()).is_err()
  );
}

#[test]
fn wire_compatibility_request_header_is_closed_and_requires_payload_capabilities() {
  let missing = r#"{"protocolVersion":{"major":1,"minor":0},"operation":"shutdown-daemon-request","requestId":"missing-capabilities-1","payload":{}}"#;
  assert!(serde_json::from_str::<RawRequestEnvelope>(missing).is_err());

  let unknown = r#"{"protocolVersion":{"major":1,"minor":0},"operation":"shutdown-daemon-request","requestId":"unknown-header-1","payloadCapabilities":[],"payload":{},"futureHeader":true}"#;
  assert!(serde_json::from_str::<RawRequestEnvelope>(unknown).is_err());
}

#[test]
fn wire_compatibility_rejects_unsafe_payload_paths_from_diagnostics() {
  let error = ProtocolError::incompatible_payload_contract(Some("field\u{1b}[31m"));
  assert!(!error.message.contains('\u{1b}'));
  assert!(!error.message.contains("field[31m"));
  assert_eq!(error.code.as_str(), "incompatible_payload");
}

#[test]
fn wire_compatibility_registered_payload_extensions_are_coherent() {
  assert_eq!(
    OPERATION_REGISTRY
      .advertised_capabilities(CURRENT_PROTOCOL_VERSION)
      .unwrap()
      .iter()
      .map(CapabilityId::as_str)
      .collect::<Vec<_>>(),
    capabilities::ALL
  );
  let future = OPERATION_REGISTRY
    .advertised_capabilities(ProtocolVersion::new(1, 1).unwrap())
    .unwrap_err();
  assert_eq!(future.kind, ProtocolErrorKind::IncompatibleProtocolVersion);

  for operation in OPERATION_REGISTRY.iter() {
    let mut unique = BTreeSet::new();
    for extension in operation.payload_extensions() {
      assert_eq!(operation.access(), OperationAccess::Mutation);
      assert!(
        capabilities::ALL.contains(&extension.capability()),
        "{} extension {} is not advertised",
        operation.name(),
        extension.capability()
      );
      assert_eq!(
        extension.minimum_version().major(),
        operation.minimum_version().major()
      );
      assert!(extension.minimum_version() >= operation.minimum_version());
      assert!(
        unique.insert(extension.capability()),
        "{} repeats payload extension {}",
        operation.name(),
        extension.capability()
      );
    }
  }
}

fn parse_raw_envelope(frame: &Value) -> RawRequestEnvelope {
  serde_json::from_str(&serde_json::to_string(frame).unwrap()).unwrap()
}

fn assert_additive_json<T>(value: Value)
where
  T: DeserializeOwned + PartialEq + Serialize + std::fmt::Debug,
{
  assert_additive_value(serde_json::from_value::<T>(value).unwrap());
}

fn assert_additive_value<T>(expected: T)
where
  T: DeserializeOwned + PartialEq + Serialize + std::fmt::Debug,
{
  let mut extended = serde_json::to_value(&expected).unwrap();
  add_future_fields(&mut extended);
  let decoded = serde_json::from_value::<T>(extended).unwrap();
  assert_eq!(decoded, expected);
}

fn add_future_fields(value: &mut Value) {
  match value {
    Value::Object(fields) => {
      for value in fields.values_mut() {
        add_future_fields(value);
      }
      fields.insert("futureField".to_string(), json!("ignored"));
    }
    Value::Array(values) => {
      for value in values {
        add_future_fields(value);
      }
    }
    _ => {}
  }
}

macro_rules! define_mutation_contract_helpers {
  ($(($operation:path, $payload:ty)),+ $(,)?) => {
    fn assert_mutation_roundtrip(frame: &Value, capabilities: &[CapabilityId]) {
      match frame["operation"].as_str().unwrap() {
        $(
          $operation => assert_payload_roundtrip::<$payload>(frame, capabilities),
        )+
        operation => panic!("fixture contains unknown mutation operation `{operation}`"),
      }
    }

    fn assert_mutation_incompatible(frame: &Value, capabilities: &[CapabilityId]) {
      match frame["operation"].as_str().unwrap() {
        $(
          $operation => assert_payload_incompatible::<$payload>(frame, capabilities),
        )+
        operation => panic!("fixture contains unknown mutation operation `{operation}`"),
      }
    }

    fn mutation_schemas() -> Value {
      let mut schemas = BTreeMap::new();
      $(
        schemas.insert(
          $operation,
          serde_json::to_value(schema_for!($payload)).unwrap(),
        );
      )+
      serde_json::to_value(schemas).unwrap()
    }
  };
}

define_mutation_contract_helpers!(
  (
    message_types::REGISTER_ENTRYPOINT_REQUEST,
    RegisterEntrypointPayload
  ),
  (
    message_types::UNREGISTER_ENTRYPOINT_REQUEST,
    UnregisterEntrypointPayload
  ),
  (
    message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
    HeartbeatEntrypointPayload
  ),
  (
    message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
    SetEntrypointEnabledPayload
  ),
  (
    message_types::SET_DOMAIN_ENABLED_REQUEST,
    SetDomainEnabledPayload
  ),
  (message_types::SET_AUTOSTART_REQUEST, SetAutostartPayload),
  (
    message_types::SHUTDOWN_DAEMON_REQUEST,
    ShutdownDaemonPayload
  ),
);

fn assert_payload_roundtrip<T>(frame: &Value, capabilities: &[CapabilityId])
where
  T: Clone + OperationPayload + Serialize,
{
  let raw = parse_raw_envelope(frame);
  let authorized = OPERATION_REGISTRY
    .authorize_envelope(&raw, capabilities)
    .unwrap();
  let payload = authorized.decode::<T>().unwrap();
  let outgoing = RequestEnvelope::new(raw.protocol_version(), raw.request_id().clone(), payload);
  assert_eq!(serde_json::to_value(outgoing).unwrap(), *frame);
}

fn assert_payload_incompatible<T>(frame: &Value, capabilities: &[CapabilityId])
where
  T: OperationPayload + std::fmt::Debug,
{
  let raw = parse_raw_envelope(frame);
  let error = OPERATION_REGISTRY
    .authorize_envelope(&raw, capabilities)
    .unwrap()
    .decode::<T>()
    .unwrap_err();
  assert_incompatible_payload(&error, raw.request_id().as_str());
}

fn assert_invalid_payload(error: &ProtocolError, request_id: &str) {
  assert_eq!(error.kind, ProtocolErrorKind::PayloadDecodeFailed);
  assert_eq!(error.code.as_str(), "invalid_payload");
  assert!(!error.retryable);
  assert_eq!(
    error.request_id.as_ref().map(RequestId::as_str),
    Some(request_id)
  );
}

fn assert_incompatible_payload(error: &ProtocolError, request_id: &str) {
  assert_eq!(error.kind, ProtocolErrorKind::IncompatibleProtocolVersion);
  assert_eq!(error.code.as_str(), "incompatible_payload");
  assert!(!error.retryable);
  assert_eq!(
    error.request_id.as_ref().map(RequestId::as_str),
    Some(request_id)
  );
}

fn current_wire_contract() -> Value {
  let operations = OPERATION_REGISTRY
    .iter()
    .map(|operation| {
      json!({
        "access": operation_access(operation.access()),
        "deadline": operation_deadline(operation.deadline()),
        "minimumVersion": operation.minimum_version(),
        "name": operation.name(),
        "payloadExtensions": operation.payload_extensions().iter().map(|extension| json!({
          "capability": extension.capability(),
          "minimumVersion": extension.minimum_version(),
        })).collect::<Vec<_>>(),
        "requiredCapability": operation.required_capability(),
        "shape": operation_shape(operation.shape()),
        "timeoutRetryable": operation.timeout_retryable(),
      })
    })
    .collect::<Vec<_>>();
  let envelope = RequestEnvelope::new(
    CURRENT_PROTOCOL_VERSION,
    RequestId::parse("contract-envelope-1").unwrap(),
    SetDomainEnabledPayload {
      domain_key: "app.localhost".to_string(),
      enabled: true,
      registration_id: "entrypoint-1".to_string(),
    },
  );

  json!({
    "schemaVersion": 1,
    "protocolVersion": CURRENT_PROTOCOL_VERSION,
    "capabilities": OPERATION_REGISTRY.advertised_capabilities(CURRENT_PROTOCOL_VERSION).unwrap(),
    "coreErrorCodes": core_error_codes(),
    "errorKindCodes": protocol_error_kind_codes(),
    "requestEnvelope": envelope,
    "operations": operations,
    "discriminators": wire_discriminators(),
  })
}

fn core_error_codes() -> Vec<String> {
  [
    ProtocolError::incompatible_payload_contract(None),
    ProtocolError::incompatible_protocol_range(
      ProtocolVersionRange::exact(ProtocolVersion::new(2, 0).unwrap()),
      SUPPORTED_PROTOCOL_VERSIONS,
    ),
    ProtocolError::payload_decode_failed(serde_json::from_str::<Value>("{").unwrap_err()),
    ProtocolError::access_denied("contract", "denied", None),
    ProtocolError::unsupported_capability("future-capability", Box::default()),
    ProtocolError::unsupported_operation("future-operation", Box::default()),
  ]
  .into_iter()
  .map(|error| error.code.to_string())
  .collect()
}

fn protocol_error_kind_codes() -> BTreeMap<String, &'static str> {
  protocol_error_kinds()
    .into_iter()
    .map(|kind| {
      let serialized = serde_json::to_value(&kind).unwrap();
      (
        serialized.as_str().unwrap().to_string(),
        kind.default_code(),
      )
    })
    .collect()
}

fn assert_closed_mutation_schemas(schema: &Value, path: &str) {
  match schema {
    Value::Object(object) => {
      if object.contains_key("properties")
        || object.get("type").and_then(Value::as_str) == Some("object")
      {
        assert_eq!(
          object.get("additionalProperties"),
          Some(&Value::Bool(false)),
          "mutation object schema `{path}` must be closed"
        );
      }
      for (key, value) in object {
        assert_closed_mutation_schemas(value, &format!("{path}.{key}"));
      }
    }
    Value::Array(values) => {
      for (index, value) in values.iter().enumerate() {
        assert_closed_mutation_schemas(value, &format!("{path}[{index}]"));
      }
    }
    _ => {}
  }
}

fn operation_access(access: OperationAccess) -> &'static str {
  match access {
    OperationAccess::ReadOnly => "readOnly",
    OperationAccess::Mutation => "mutation",
  }
}

fn operation_shape(shape: OperationShape) -> &'static str {
  match shape {
    OperationShape::Unary => "unary",
    OperationShape::ServerStream => "serverStream",
  }
}

fn operation_deadline(deadline: OperationDeadlineClass) -> &'static str {
  match deadline {
    OperationDeadlineClass::Ordinary => "ordinary",
    OperationDeadlineClass::Reload => "reload",
    OperationDeadlineClass::Stream => "stream",
    OperationDeadlineClass::Shutdown => "shutdown",
  }
}

macro_rules! enum_wire_values {
  ($enum_type:ty => [$($variant:path),+ $(,)?]) => {{
    let _exhaustive = |value: $enum_type| match value {
      $($variant => (),)+
    };
    serde_json::to_value([$($variant),+]).unwrap()
  }};
}

fn wire_discriminators() -> Value {
  json!({
    "activationState": enum_wire_values!(ActivationState => [
      ActivationState::Unknown,
      ActivationState::Registered,
      ActivationState::Activating,
      ActivationState::Active,
      ActivationState::Inactive,
      ActivationState::Faulted,
    ]),
    "autostartMode": enum_wire_values!(AutostartMode => [
      AutostartMode::Disabled,
      AutostartMode::Daemon,
    ]),
    "autostartModePayload": enum_wire_values!(AutostartModePayload => [
      AutostartModePayload::Disabled,
      AutostartModePayload::Daemon,
    ]),
    "autostartStatus": enum_wire_values!(AutostartStatus => [
      AutostartStatus::Unknown,
      AutostartStatus::Disabled,
      AutostartStatus::Enabled,
      AutostartStatus::Unsupported,
      AutostartStatus::Misconfigured,
    ]),
    "configApplyStatus": enum_wire_values!(ConfigApplyStatus => [
      ConfigApplyStatus::Unknown,
      ConfigApplyStatus::NotApplied,
      ConfigApplyStatus::Applied,
      ConfigApplyStatus::Failed,
      ConfigApplyStatus::Idle,
    ]),
    "historyKind": enum_wire_values!(HistoryKind => [
      HistoryKind::Registration,
      HistoryKind::Runtime,
      HistoryKind::Config,
      HistoryKind::Autostart,
      HistoryKind::Log,
    ]),
    "logAttributionKind": enum_wire_values!(LogAttributionKind => [
      LogAttributionKind::Unknown,
      LogAttributionKind::Runtime,
      LogAttributionKind::RuntimeControl,
      LogAttributionKind::Entrypoint,
      LogAttributionKind::Domain,
    ]),
    "logEntryKind": enum_wire_values!(LogEntryKind => [
      LogEntryKind::Normal,
      LogEntryKind::Lifecycle,
      LogEntryKind::IngestionOverflow,
      LogEntryKind::RetentionGap,
    ]),
    "logSeverity": enum_wire_values!(LogSeverity => [
      LogSeverity::Unknown,
      LogSeverity::Trace,
      LogSeverity::Debug,
      LogSeverity::Info,
      LogSeverity::Warn,
      LogSeverity::Error,
      LogSeverity::Fatal,
    ]),
    "logStreamStatus": enum_wire_values!(LogStreamStatus => [
      LogStreamStatus::Unknown,
      LogStreamStatus::Empty,
      LogStreamStatus::Active,
      LogStreamStatus::Stale,
      LogStreamStatus::Removed,
      LogStreamStatus::ReadError,
    ]),
    "protocolErrorKind": serde_json::to_value(protocol_error_kinds()).unwrap(),
    "runtimeStatus": enum_wire_values!(RuntimeStatus => [
      RuntimeStatus::Unknown,
      RuntimeStatus::NotResolved,
      RuntimeStatus::Resolved,
      RuntimeStatus::Running,
      RuntimeStatus::Unhealthy,
      RuntimeStatus::Idle,
    ]),
    "registrationActivationState": enum_wire_values!(RegistrationActivationState => [
      RegistrationActivationState::Unknown,
      RegistrationActivationState::Registered,
      RegistrationActivationState::Activating,
      RegistrationActivationState::Active,
      RegistrationActivationState::Inactive,
      RegistrationActivationState::Faulted,
    ]),
    "serverHandshakeStatus": server_handshake_discriminators(),
    "stateChangeKind": enum_wire_values!(StateChangeKind => [
      StateChangeKind::Snapshot,
      StateChangeKind::RegistrationsChanged,
      StateChangeKind::RuntimeChanged,
    ]),
  })
}

fn protocol_error_kinds() -> [ProtocolErrorKind; 16] {
  let _exhaustive = |kind: &ProtocolErrorKind| match kind {
    ProtocolErrorKind::IncompatibleProtocolVersion => (),
    ProtocolErrorKind::UnsupportedCapability => (),
    ProtocolErrorKind::PayloadDecodeFailed => (),
    ProtocolErrorKind::AccessDenied => (),
    ProtocolErrorKind::InvalidInput => (),
    ProtocolErrorKind::Conflict => (),
    ProtocolErrorKind::Configuration => (),
    ProtocolErrorKind::CaddyRuntime => (),
    ProtocolErrorKind::Storage => (),
    ProtocolErrorKind::Busy => (),
    ProtocolErrorKind::Frame => (),
    ProtocolErrorKind::Timeout => (),
    ProtocolErrorKind::ProtocolViolation => (),
    ProtocolErrorKind::ShuttingDown => (),
    ProtocolErrorKind::StaleInstance => (),
    ProtocolErrorKind::Internal => (),
  };
  [
    ProtocolErrorKind::IncompatibleProtocolVersion,
    ProtocolErrorKind::UnsupportedCapability,
    ProtocolErrorKind::PayloadDecodeFailed,
    ProtocolErrorKind::AccessDenied,
    ProtocolErrorKind::InvalidInput,
    ProtocolErrorKind::Conflict,
    ProtocolErrorKind::Configuration,
    ProtocolErrorKind::CaddyRuntime,
    ProtocolErrorKind::Storage,
    ProtocolErrorKind::Busy,
    ProtocolErrorKind::Frame,
    ProtocolErrorKind::Timeout,
    ProtocolErrorKind::ProtocolViolation,
    ProtocolErrorKind::ShuttingDown,
    ProtocolErrorKind::StaleInstance,
    ProtocolErrorKind::Internal,
  ]
}

fn server_handshake_discriminators() -> [&'static str; 2] {
  let _exhaustive = |frame: &ServerHandshakeFrame| match frame {
    ServerHandshakeFrame::Accepted(_) => (),
    ServerHandshakeFrame::Rejected(_) => (),
  };
  ["accepted", "rejected"]
}
