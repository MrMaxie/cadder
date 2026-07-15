//! Closed payload contracts for versioned mutation requests.
//!
//! Request correlation belongs to [`crate::RequestEnvelope`]. The older request DTOs retain their
//! embedded request IDs only for the transitional flat-envelope adapter.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
  ActivationState, AutostartMode, DomainName, EntrypointInstanceIdentity, EntrypointRegistration,
  LogStreamIdentity, OperationPayload, OwnerProcessIdentity, RegisteredDomain, ShimRunMetadata,
  SourcePath, message_types, operation_payload_sealed,
};

/// Activation values accepted by the immutable V1 registration input contract.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RegistrationActivationState {
  Unknown,
  Registered,
  Activating,
  Active,
  Inactive,
  Faulted,
}

impl From<RegistrationActivationState> for ActivationState {
  fn from(value: RegistrationActivationState) -> Self {
    match value {
      RegistrationActivationState::Unknown => Self::Unknown,
      RegistrationActivationState::Registered => Self::Registered,
      RegistrationActivationState::Activating => Self::Activating,
      RegistrationActivationState::Active => Self::Active,
      RegistrationActivationState::Inactive => Self::Inactive,
      RegistrationActivationState::Faulted => Self::Faulted,
    }
  }
}

/// Autostart values accepted by the immutable V1 mutation input contract.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AutostartModePayload {
  Disabled,
  Daemon,
}

impl From<AutostartModePayload> for AutostartMode {
  fn from(value: AutostartModePayload) -> Self {
    match value {
      AutostartModePayload::Disabled => Self::Disabled,
      AutostartModePayload::Daemon => Self::Daemon,
    }
  }
}

/// Closed identity supplied for one live shim instance.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct EntrypointInstancePayload {
  pub instance_id: String,
  pub started_at_utc: DateTime<Utc>,
  pub shim_session_nonce: String,
}

impl From<EntrypointInstancePayload> for EntrypointInstanceIdentity {
  fn from(value: EntrypointInstancePayload) -> Self {
    Self {
      instance_id: value.instance_id,
      started_at_utc: value.started_at_utc,
      shim_session_nonce: value.shim_session_nonce,
    }
  }
}

/// Closed source-path representation supplied during registration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct SourcePathPayload {
  pub raw: String,
  pub canonical: Option<String>,
}

impl From<SourcePathPayload> for SourcePath {
  fn from(value: SourcePathPayload) -> Self {
    Self {
      raw: value.raw,
      canonical: value.canonical,
    }
  }
}

/// Closed raw and canonical domain name supplied during registration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct DomainNamePayload {
  pub raw: String,
  pub canonical: String,
}

impl From<DomainNamePayload> for DomainName {
  fn from(value: DomainNamePayload) -> Self {
    Self {
      raw: value.raw,
      canonical: value.canonical,
    }
  }
}

/// Closed log-stream identity supplied by a shim registration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct RegistrationLogStreamPayload {
  pub stream_id: String,
  pub domain_key: Option<String>,
  pub channel: String,
}

impl From<RegistrationLogStreamPayload> for LogStreamIdentity {
  fn from(value: RegistrationLogStreamPayload) -> Self {
    Self {
      stream_id: value.stream_id,
      domain_key: value.domain_key,
      channel: value.channel,
    }
  }
}

/// Closed domain entry supplied by a shim registration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct RegisteredDomainPayload {
  pub name: DomainNamePayload,
  pub activation_state: RegistrationActivationState,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub upstream: Option<String>,
  pub log_stream: RegistrationLogStreamPayload,
}

impl From<RegisteredDomainPayload> for RegisteredDomain {
  fn from(value: RegisteredDomainPayload) -> Self {
    Self {
      name: value.name.into(),
      activation_state: value.activation_state.into(),
      upstream: value.upstream,
      log_stream: value.log_stream.into(),
    }
  }
}

/// Closed owner-process identity supplied by a shim registration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct OwnerProcessPayload {
  pub process_id: u32,
  pub process_start_time_utc: DateTime<Utc>,
  pub shim_session_nonce: String,
  pub executable_path: Option<String>,
}

impl From<OwnerProcessPayload> for OwnerProcessIdentity {
  fn from(value: OwnerProcessPayload) -> Self {
    Self {
      process_id: value.process_id,
      process_start_time_utc: value.process_start_time_utc,
      shim_session_nonce: value.shim_session_nonce,
      executable_path: value.executable_path,
    }
  }
}

/// Closed command metadata supplied by a shim registration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct ShimRunPayload {
  pub adapter: Option<String>,
  pub raw_arguments: Vec<String>,
  pub command_line: String,
}

impl From<ShimRunPayload> for ShimRunMetadata {
  fn from(value: ShimRunPayload) -> Self {
    Self {
      adapter: value.adapter,
      raw_arguments: value.raw_arguments,
      command_line: value.command_line,
    }
  }
}

/// Deliberately stable registration input, separate from additive runtime snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct EntrypointRegistrationPayload {
  pub registration_id: String,
  pub entrypoint_instance: EntrypointInstancePayload,
  pub source_working_directory: SourcePathPayload,
  pub source_config_path: SourcePathPayload,
  pub registered_domains: Vec<RegisteredDomainPayload>,
  pub activation_state: RegistrationActivationState,
  pub owner_process: OwnerProcessPayload,
  pub log_stream: RegistrationLogStreamPayload,
  pub shim_run: Option<ShimRunPayload>,
  pub created_at_utc: DateTime<Utc>,
  pub last_heartbeat_utc: DateTime<Utc>,
}

impl From<EntrypointRegistrationPayload> for EntrypointRegistration {
  fn from(value: EntrypointRegistrationPayload) -> Self {
    Self {
      registration_id: value.registration_id,
      entrypoint_instance: value.entrypoint_instance.into(),
      source_working_directory: value.source_working_directory.into(),
      source_config_path: value.source_config_path.into(),
      registered_domains: value
        .registered_domains
        .into_iter()
        .map(Into::into)
        .collect(),
      activation_state: value.activation_state.into(),
      owner_process: value.owner_process.into(),
      log_stream: value.log_stream.into(),
      shim_run: value.shim_run.map(Into::into),
      created_at_utc: value.created_at_utc,
      last_heartbeat_utc: value.last_heartbeat_utc,
    }
  }
}

/// Registers one shim-owned entrypoint after the request envelope is authorized.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct RegisterEntrypointPayload {
  pub registration: EntrypointRegistrationPayload,
}

impl From<RegisterEntrypointPayload> for EntrypointRegistration {
  fn from(value: RegisterEntrypointPayload) -> Self {
    value.registration.into()
  }
}

/// Releases a live entrypoint registration owned by the matching shim session.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct UnregisterEntrypointPayload {
  pub registration_id: String,
  pub shim_session_nonce: String,
}

/// Renews the lease for a live entrypoint registration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct HeartbeatEntrypointPayload {
  pub registration_id: String,
  pub shim_session_nonce: String,
}

/// Changes the desired activation state of one entrypoint.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct SetEntrypointEnabledPayload {
  pub registration_id: String,
  pub shim_session_nonce: Option<String>,
  pub enabled: bool,
}

/// Changes the desired activation state of one registered domain.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct SetDomainEnabledPayload {
  pub registration_id: String,
  pub domain_key: String,
  pub enabled: bool,
}

/// Selects the desired per-user daemon autostart mode.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct SetAutostartPayload {
  pub mode: AutostartModePayload,
}

impl From<SetAutostartPayload> for AutostartMode {
  fn from(value: SetAutostartPayload) -> Self {
    value.mode.into()
  }
}

/// Requests a coordinated daemon shutdown without carrying duplicate correlation data.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[schemars(deny_unknown_fields)]
pub struct ShutdownDaemonPayload {}

macro_rules! operation_payload {
  ($payload:ty, $operation:expr) => {
    impl operation_payload_sealed::Sealed for $payload {}

    impl OperationPayload for $payload {
      const OPERATION: &'static str = $operation;
    }
  };
}

impl operation_payload_sealed::Sealed for RegisterEntrypointPayload {}

impl OperationPayload for RegisterEntrypointPayload {
  const OPERATION: &'static str = message_types::REGISTER_ENTRYPOINT_REQUEST;

  fn incompatible_discriminator(payload: &Value) -> Option<String> {
    let registration = payload.get("registration")?.as_object()?;
    if unknown_registration_activation(registration.get("activationState")) {
      return Some("registration.activationState".to_string());
    }
    registration
      .get("registeredDomains")?
      .as_array()?
      .iter()
      .enumerate()
      .find_map(|(index, domain)| {
        unknown_registration_activation(domain.get("activationState"))
          .then(|| format!("registration.registeredDomains[{index}].activationState"))
      })
  }
}

impl operation_payload_sealed::Sealed for SetAutostartPayload {}

impl OperationPayload for SetAutostartPayload {
  const OPERATION: &'static str = message_types::SET_AUTOSTART_REQUEST;

  fn incompatible_discriminator(payload: &Value) -> Option<String> {
    unknown_autostart_mode(payload.get("mode")).then(|| "mode".to_string())
  }
}

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

fn unknown_registration_activation(value: Option<&Value>) -> bool {
  unknown_string_discriminator::<RegistrationActivationState>(value)
}

fn unknown_autostart_mode(value: Option<&Value>) -> bool {
  unknown_string_discriminator::<AutostartModePayload>(value)
}

fn unknown_string_discriminator<T>(value: Option<&Value>) -> bool
where
  T: DeserializeOwned,
{
  value
    .filter(|value| value.is_string())
    .is_some_and(|value| serde_json::from_value::<T>(value.clone()).is_err())
}
