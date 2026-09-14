use serde::{Deserialize, Serialize};

use crate::{EntrypointRegistration, OperationPayload, message_types, operation_payload_sealed};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegisterEntrypointPayload {
  pub registration: EntrypointRegistration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnregisterEntrypointPayload {
  pub registration_id: String,
  pub shim_session_nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatEntrypointPayload {
  pub registration_id: String,
  pub shim_session_nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetEntrypointEnabledPayload {
  pub registration_id: String,
  pub shim_session_nonce: Option<String>,
  pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetDomainEnabledPayload {
  pub registration_id: String,
  pub domain_key: String,
  pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownDaemonPayload {}

macro_rules! operation_payload {
  ($payload:ty, $operation:expr) => {
    impl operation_payload_sealed::Sealed for $payload {}

    impl OperationPayload for $payload {
      const OPERATION: &'static str = $operation;
    }
  };
}

operation_payload!(
  RegisterEntrypointPayload,
  message_types::REGISTER_ENTRYPOINT_REQUEST
);
operation_payload!(
  UnregisterEntrypointPayload,
  message_types::UNREGISTER_ENTRYPOINT_REQUEST
);
operation_payload!(
  HeartbeatEntrypointPayload,
  message_types::HEARTBEAT_ENTRYPOINT_REQUEST
);
operation_payload!(
  SetEntrypointEnabledPayload,
  message_types::SET_ENTRYPOINT_ENABLED_REQUEST
);
operation_payload!(
  SetDomainEnabledPayload,
  message_types::SET_DOMAIN_ENABLED_REQUEST
);
operation_payload!(
  ShutdownDaemonPayload,
  message_types::SHUTDOWN_DAEMON_REQUEST
);
