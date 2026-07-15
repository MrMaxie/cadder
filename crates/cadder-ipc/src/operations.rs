use crate::{
  CapabilityId, OperationPayload, PROTOCOL_VERSION_1_0, ProtocolCapabilities, ProtocolError,
  ProtocolResult, ProtocolVersion, ProtocolVersionRange, RawRequestEnvelope, RequestId,
  SUPPORTED_PROTOCOL_VERSIONS, capabilities, current_capabilities,
  ensure_compatible_protocol_version, message_types,
};
use std::collections::BTreeSet;

/// Whether an operation can publish runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationAccess {
  ReadOnly,
  Mutation,
}

/// Whether an operation produces one response or keeps a server stream open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationShape {
  Unary,
  ServerStream,
}

/// The timeout policy advertised for an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationDeadlineClass {
  Ordinary,
  Reload,
  Stream,
  Shutdown,
}

/// A capability-gated extension to a closed mutation payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationExtensionDefinition {
  capability: &'static str,
  minimum_version: ProtocolVersion,
}

impl MutationExtensionDefinition {
  /// Returns the extension capability declared in a request envelope.
  pub const fn capability(self) -> &'static str {
    self.capability
  }

  /// Returns the first protocol version that can decode this extension.
  pub const fn minimum_version(self) -> ProtocolVersion {
    self.minimum_version
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OperationPolicy {
  access: OperationAccess,
  shape: OperationShape,
  deadline: OperationDeadlineClass,
  timeout_retryable: bool,
  payload_extensions: &'static [MutationExtensionDefinition],
}

impl OperationPolicy {
  const fn new(
    access: OperationAccess,
    shape: OperationShape,
    deadline: OperationDeadlineClass,
    timeout_retryable: bool,
    payload_extensions: &'static [MutationExtensionDefinition],
  ) -> Self {
    Self {
      access,
      shape,
      deadline,
      timeout_retryable,
      payload_extensions,
    }
  }
}

/// Immutable metadata used by clients, dispatch, authorization, and timeout selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationDefinition {
  name: &'static str,
  minimum_version: ProtocolVersion,
  required_capability: &'static str,
  access: OperationAccess,
  shape: OperationShape,
  deadline: OperationDeadlineClass,
  timeout_retryable: bool,
  payload_extensions: &'static [MutationExtensionDefinition],
}

impl OperationDefinition {
  const fn new(
    name: &'static str,
    minimum_version: ProtocolVersion,
    required_capability: &'static str,
    policy: OperationPolicy,
  ) -> Self {
    Self {
      name,
      minimum_version,
      required_capability,
      access: policy.access,
      shape: policy.shape,
      deadline: policy.deadline,
      timeout_retryable: policy.timeout_retryable,
      payload_extensions: policy.payload_extensions,
    }
  }

  /// Returns the exact request operation label.
  pub const fn name(self) -> &'static str {
    self.name
  }

  /// Returns the minimum negotiated version that can dispatch the operation.
  pub const fn minimum_version(self) -> ProtocolVersion {
    self.minimum_version
  }

  /// Reports whether this operation exists in the selected protocol version.
  pub const fn supports_version(self, version: ProtocolVersion) -> bool {
    version.major() == self.minimum_version.major()
      && version.minor() >= self.minimum_version.minor()
  }

  /// Returns the capability that both peers must advertise.
  pub const fn required_capability(self) -> &'static str {
    self.required_capability
  }

  /// Returns whether the handler can commit state.
  pub const fn access(self) -> OperationAccess {
    self.access
  }

  /// Returns whether the handler is unary or streaming.
  pub const fn shape(self) -> OperationShape {
    self.shape
  }

  /// Returns the operation's timeout policy.
  pub const fn deadline(self) -> OperationDeadlineClass {
    self.deadline
  }

  /// Reports whether an unchanged request can be retried after its timeout.
  pub const fn timeout_retryable(self) -> bool {
    self.timeout_retryable
  }

  /// Returns the payload extensions accepted after base operation authorization.
  pub const fn payload_extensions(self) -> &'static [MutationExtensionDefinition] {
    self.payload_extensions
  }
}

/// A request whose version and base and payload capabilities passed the central registry.
#[derive(Debug)]
pub struct AuthorizedRequestEnvelope<'a> {
  definition: &'static OperationDefinition,
  envelope: &'a RawRequestEnvelope,
}

impl AuthorizedRequestEnvelope<'_> {
  /// Returns the authorized operation metadata.
  pub const fn definition(&self) -> &'static OperationDefinition {
    self.definition
  }

  /// Returns the validated request correlation ID.
  pub fn request_id(&self) -> &RequestId {
    self.envelope.request_id()
  }

  /// Returns the authorized payload extensions used by this request.
  pub fn payload_capabilities(&self) -> &[CapabilityId] {
    self.envelope.payload_capabilities()
  }

  /// Decodes the payload only after every header gate has passed.
  ///
  /// Mutation payloads are closed recursively. Read-only payloads retain additive decoding.
  pub fn decode<T>(&self) -> ProtocolResult<T>
  where
    T: OperationPayload,
  {
    if T::OPERATION != self.definition.name() {
      return Err(
        ProtocolError::decoder_contract_mismatch(self.definition.name(), T::OPERATION)
          .with_request_id(self.envelope.request_id().clone()),
      );
    }
    if !payload_capability_sets_match::<T>(self.envelope.payload_capabilities()) {
      return Err(
        ProtocolError::incompatible_payload_contract(Some("payloadCapabilities"))
          .with_request_id(self.envelope.request_id().clone()),
      );
    }
    self
      .envelope
      .decode_payload(self.definition.access() == OperationAccess::Mutation)
      .map_err(|error| error.with_request_id(self.envelope.request_id().clone()))
  }
}

/// The single registry for every request operation accepted by Cadder.
#[derive(Debug, Clone, Copy, Default)]
pub struct OperationRegistry;

/// The canonical operation registry for this protocol build.
pub const OPERATION_REGISTRY: OperationRegistry = OperationRegistry;

impl OperationRegistry {
  /// Iterates over every accepted request operation in stable order.
  pub fn iter(self) -> impl ExactSizeIterator<Item = &'static OperationDefinition> {
    OPERATIONS.iter()
  }

  /// Finds exact metadata for a request operation.
  pub fn lookup(self, name: &str) -> Option<&'static OperationDefinition> {
    OPERATIONS.iter().find(|operation| operation.name == name)
  }

  /// Returns capabilities available at one supported negotiated version in stable order.
  pub fn advertised_capabilities(
    self,
    version: ProtocolVersion,
  ) -> ProtocolResult<Box<[CapabilityId]>> {
    ensure_supported_version(version)?;
    Ok(
      capabilities::ALL
        .iter()
        .filter(|capability| self.capability_available_at(capability, version))
        .map(|capability| CapabilityId::known(capability))
        .collect(),
    )
  }

  /// Intersects requested capabilities with those available at the negotiated version.
  pub fn negotiate_capabilities(
    self,
    version: ProtocolVersion,
    requested: &[CapabilityId],
  ) -> ProtocolResult<Box<[CapabilityId]>> {
    Ok(
      self
        .advertised_capabilities(version)?
        .into_iter()
        .filter(|supported| requested.iter().any(|requested| requested == supported))
        .collect(),
    )
  }

  /// Gates a post-handshake operation before its typed payload is decoded.
  pub fn authorize(
    self,
    name: &str,
    version: ProtocolVersion,
    negotiated_capabilities: &[CapabilityId],
  ) -> ProtocolResult<&'static OperationDefinition> {
    ensure_supported_version(version)?;
    let operation = self
      .lookup(name)
      .ok_or_else(|| unsupported_versioned_operation(name))?;
    if !operation.supports_version(version) {
      return Err(ProtocolError::incompatible_protocol_range(
        ProtocolVersionRange::exact(version),
        ProtocolVersionRange::exact(operation.minimum_version),
      ));
    }
    if !negotiated_capabilities
      .iter()
      .any(|capability| capability.as_str() == operation.required_capability)
    {
      return Err(unsupported_capability(operation, negotiated_capabilities));
    }
    Ok(operation)
  }

  fn capability_available_at(self, capability: &str, version: ProtocolVersion) -> bool {
    self.iter().any(|operation| {
      (operation.required_capability() == capability && operation.supports_version(version))
        || operation.payload_extensions().iter().any(|extension| {
          extension.capability() == capability && extension.minimum_version() <= version
        })
    })
  }

  /// Gates a raw request header and returns the only API that exposes typed payload decoding.
  pub fn authorize_envelope<'a>(
    self,
    envelope: &'a RawRequestEnvelope,
    negotiated_capabilities: &[CapabilityId],
  ) -> ProtocolResult<AuthorizedRequestEnvelope<'a>> {
    let authorized = (|| {
      let definition = self.authorize(
        envelope.operation(),
        envelope.protocol_version(),
        negotiated_capabilities,
      )?;
      authorize_payload_capabilities(
        definition,
        envelope.protocol_version(),
        negotiated_capabilities,
        envelope.payload_capabilities(),
      )?;
      Ok(AuthorizedRequestEnvelope {
        definition,
        envelope,
      })
    })();

    authorized.map_err(|error: ProtocolError| error.with_request_id(envelope.request_id().clone()))
  }

  /// Gates the transitional flat envelope without treating it as a negotiated V1 session.
  pub fn authorize_legacy(
    self,
    name: &str,
    protocol_version: u16,
    advertised_capabilities: Option<&ProtocolCapabilities>,
  ) -> ProtocolResult<&'static OperationDefinition> {
    ensure_compatible_protocol_version(protocol_version)?;
    let operation = self
      .lookup(name)
      .ok_or_else(|| unsupported_legacy_operation(name))?;
    if !advertised_capabilities
      .is_some_and(|advertised| advertised.supports_version(operation.required_capability, 1))
    {
      return Err(ProtocolError::unsupported_capability(
        operation.required_capability,
        current_capabilities(),
      ));
    }
    Ok(operation)
  }
}

fn ensure_supported_version(version: ProtocolVersion) -> ProtocolResult<()> {
  let offered = ProtocolVersionRange::exact(version);
  if SUPPORTED_PROTOCOL_VERSIONS.negotiate(offered) == Some(version) {
    return Ok(());
  }
  Err(ProtocolError::incompatible_protocol_range(
    offered,
    SUPPORTED_PROTOCOL_VERSIONS,
  ))
}

fn payload_capability_sets_match<T>(declared: &[CapabilityId]) -> bool
where
  T: OperationPayload,
{
  declared.len() == T::PAYLOAD_CAPABILITIES.len()
    && T::PAYLOAD_CAPABILITIES.iter().all(|required| {
      declared
        .iter()
        .any(|declared| declared.as_str() == *required)
    })
}

fn authorize_payload_capabilities(
  operation: &OperationDefinition,
  version: ProtocolVersion,
  negotiated_capabilities: &[CapabilityId],
  payload_capabilities: &[CapabilityId],
) -> ProtocolResult<()> {
  let mut unique_capabilities = BTreeSet::new();
  for capability in payload_capabilities {
    if !unique_capabilities.insert(capability) {
      return Err(ProtocolError::incompatible_payload_contract(Some(
        "payloadCapabilities",
      )));
    }
    let extension = operation
      .payload_extensions()
      .iter()
      .find(|extension| extension.capability() == capability.as_str())
      .ok_or_else(|| unsupported_payload_capability(capability, negotiated_capabilities))?;
    if version.major() != extension.minimum_version().major()
      || version < extension.minimum_version()
    {
      return Err(ProtocolError::incompatible_protocol_range(
        ProtocolVersionRange::exact(version),
        ProtocolVersionRange::exact(extension.minimum_version()),
      ));
    }
    if !negotiated_capabilities.contains(capability) {
      return Err(unsupported_payload_capability(
        capability,
        negotiated_capabilities,
      ));
    }
  }
  Ok(())
}

fn unsupported_versioned_operation(name: &str) -> ProtocolError {
  ProtocolError::new(
    crate::ProtocolErrorKind::UnsupportedCapability,
    crate::ProtocolErrorCode::known("unsupported_operation"),
    format!("Cadder does not support the `{name}` operation."),
    Some("Use an operation advertised by the connected Cadder daemon.".into()),
    false,
  )
}

fn unsupported_legacy_operation(name: &str) -> ProtocolError {
  ProtocolError::unsupported_operation(name, current_capabilities())
}

fn unsupported_capability(
  operation: &OperationDefinition,
  capabilities: &[CapabilityId],
) -> ProtocolError {
  ProtocolError::unsupported_capability(
    operation.required_capability,
    capabilities
      .iter()
      .map(ToString::to_string)
      .collect::<Box<[_]>>(),
  )
}

fn unsupported_payload_capability(
  capability: &CapabilityId,
  negotiated_capabilities: &[CapabilityId],
) -> ProtocolError {
  ProtocolError::unsupported_capability(
    capability.as_str(),
    negotiated_capabilities
      .iter()
      .map(ToString::to_string)
      .collect::<Box<[_]>>(),
  )
}

const NO_PAYLOAD_EXTENSIONS: &[MutationExtensionDefinition] = &[];

const OPERATIONS: &[OperationDefinition] = &[
  OperationDefinition::new(
    message_types::REGISTER_ENTRYPOINT_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::ENTRYPOINT_REGISTRATION,
    OperationPolicy::new(
      OperationAccess::Mutation,
      OperationShape::Unary,
      OperationDeadlineClass::Reload,
      false,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
  OperationDefinition::new(
    message_types::UNREGISTER_ENTRYPOINT_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::ENTRYPOINT_REGISTRATION,
    OperationPolicy::new(
      OperationAccess::Mutation,
      OperationShape::Unary,
      OperationDeadlineClass::Reload,
      false,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
  OperationDefinition::new(
    message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::ENTRYPOINT_REGISTRATION,
    OperationPolicy::new(
      OperationAccess::Mutation,
      OperationShape::Unary,
      OperationDeadlineClass::Ordinary,
      true,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
  OperationDefinition::new(
    message_types::QUERY_STATE_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::RUNTIME_STATE,
    OperationPolicy::new(
      OperationAccess::ReadOnly,
      OperationShape::Unary,
      OperationDeadlineClass::Ordinary,
      true,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
  OperationDefinition::new(
    message_types::SUBSCRIBE_STATE_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::STATE_SUBSCRIPTION,
    OperationPolicy::new(
      OperationAccess::ReadOnly,
      OperationShape::ServerStream,
      OperationDeadlineClass::Stream,
      true,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
  OperationDefinition::new(
    message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::ACTIVATION_CONTROL,
    OperationPolicy::new(
      OperationAccess::Mutation,
      OperationShape::Unary,
      OperationDeadlineClass::Reload,
      false,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
  OperationDefinition::new(
    message_types::SET_DOMAIN_ENABLED_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::ACTIVATION_CONTROL,
    OperationPolicy::new(
      OperationAccess::Mutation,
      OperationShape::Unary,
      OperationDeadlineClass::Reload,
      false,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
  OperationDefinition::new(
    message_types::QUERY_LOGS_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::LOGS,
    OperationPolicy::new(
      OperationAccess::ReadOnly,
      OperationShape::Unary,
      OperationDeadlineClass::Ordinary,
      true,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
  OperationDefinition::new(
    message_types::QUERY_HISTORY_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::HISTORY,
    OperationPolicy::new(
      OperationAccess::ReadOnly,
      OperationShape::Unary,
      OperationDeadlineClass::Ordinary,
      true,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
  OperationDefinition::new(
    message_types::QUERY_AUTOSTART_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::AUTOSTART,
    OperationPolicy::new(
      OperationAccess::ReadOnly,
      OperationShape::Unary,
      OperationDeadlineClass::Ordinary,
      true,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
  OperationDefinition::new(
    message_types::SET_AUTOSTART_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::AUTOSTART,
    OperationPolicy::new(
      OperationAccess::Mutation,
      OperationShape::Unary,
      OperationDeadlineClass::Ordinary,
      false,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
  OperationDefinition::new(
    message_types::SHUTDOWN_DAEMON_REQUEST,
    PROTOCOL_VERSION_1_0,
    capabilities::DAEMON_LIFECYCLE,
    OperationPolicy::new(
      OperationAccess::Mutation,
      OperationShape::Unary,
      OperationDeadlineClass::Shutdown,
      false,
      NO_PAYLOAD_EXTENSIONS,
    ),
  ),
];

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{ProtocolErrorKind, ShutdownDaemonPayload, operation_payload_sealed};
  use serde::Deserialize;

  #[derive(Debug, Deserialize, PartialEq, Eq)]
  #[serde(rename_all = "camelCase", deny_unknown_fields)]
  struct ScheduledAutostartPayload {
    mode: String,
    schedule: String,
  }

  impl operation_payload_sealed::Sealed for ScheduledAutostartPayload {}

  impl OperationPayload for ScheduledAutostartPayload {
    const OPERATION: &'static str = message_types::SET_AUTOSTART_REQUEST;
    const PAYLOAD_CAPABILITIES: &'static [&'static str] = &["scheduled-autostart"];
  }

  #[test]
  fn wire_compatibility_payload_extensions_require_their_minor_and_capability() {
    const VERSION_1_1: ProtocolVersion = match ProtocolVersion::new(1, 1) {
      Ok(version) => version,
      Err(_) => panic!("1.1 is a valid protocol version"),
    };
    const FUTURE_OPERATION: OperationDefinition = OperationDefinition::new(
      "future-operation",
      VERSION_1_1,
      "future-capability",
      OperationPolicy::new(
        OperationAccess::ReadOnly,
        OperationShape::Unary,
        OperationDeadlineClass::Ordinary,
        true,
        NO_PAYLOAD_EXTENSIONS,
      ),
    );
    const EXTENSION: MutationExtensionDefinition = MutationExtensionDefinition {
      capability: "scheduled-autostart",
      minimum_version: VERSION_1_1,
    };
    const EXTENSIONS: &[MutationExtensionDefinition] = &[EXTENSION];
    const OPERATION: OperationDefinition = OperationDefinition::new(
      message_types::SET_AUTOSTART_REQUEST,
      PROTOCOL_VERSION_1_0,
      capabilities::AUTOSTART,
      OperationPolicy::new(
        OperationAccess::Mutation,
        OperationShape::Unary,
        OperationDeadlineClass::Ordinary,
        false,
        EXTENSIONS,
      ),
    );

    assert!(!FUTURE_OPERATION.supports_version(PROTOCOL_VERSION_1_0));
    assert!(FUTURE_OPERATION.supports_version(VERSION_1_1));
    assert!(!FUTURE_OPERATION.supports_version(ProtocolVersion::new(2, 0).unwrap()));

    let extension = CapabilityId::parse("scheduled-autostart").unwrap();
    let too_old = authorize_payload_capabilities(
      &OPERATION,
      PROTOCOL_VERSION_1_0,
      std::slice::from_ref(&extension),
      std::slice::from_ref(&extension),
    )
    .unwrap_err();
    assert_eq!(too_old.kind, ProtocolErrorKind::IncompatibleProtocolVersion);

    let unavailable = authorize_payload_capabilities(
      &OPERATION,
      VERSION_1_1,
      &[],
      std::slice::from_ref(&extension),
    )
    .unwrap_err();
    assert_eq!(unavailable.kind, ProtocolErrorKind::UnsupportedCapability);

    authorize_payload_capabilities(
      &OPERATION,
      VERSION_1_1,
      std::slice::from_ref(&extension),
      std::slice::from_ref(&extension),
    )
    .unwrap();

    let duplicate = authorize_payload_capabilities(
      &OPERATION,
      VERSION_1_1,
      std::slice::from_ref(&extension),
      &[extension.clone(), extension.clone()],
    )
    .unwrap_err();
    assert_eq!(
      duplicate.kind,
      ProtocolErrorKind::IncompatibleProtocolVersion
    );

    let raw: RawRequestEnvelope = serde_json::from_str(
      r#"{"protocolVersion":{"major":1,"minor":1},"operation":"set-autostart-request","requestId":"scheduled-1","payloadCapabilities":["scheduled-autostart"],"payload":{"mode":"scheduled","schedule":"startup-delay"}}"#,
    )
    .unwrap();
    authorize_payload_capabilities(
      &OPERATION,
      raw.protocol_version(),
      std::slice::from_ref(&extension),
      raw.payload_capabilities(),
    )
    .unwrap();
    let authorized = AuthorizedRequestEnvelope {
      definition: &OPERATION,
      envelope: &raw,
    };
    assert_eq!(
      authorized.decode::<ScheduledAutostartPayload>().unwrap(),
      ScheduledAutostartPayload {
        mode: "scheduled".to_string(),
        schedule: "startup-delay".to_string(),
      }
    );
    let wrong_decoder = authorized.decode::<ShutdownDaemonPayload>().unwrap_err();
    assert_eq!(wrong_decoder.kind, ProtocolErrorKind::Internal);

    let omitted: RawRequestEnvelope = serde_json::from_str(
      r#"{"protocolVersion":{"major":1,"minor":1},"operation":"set-autostart-request","requestId":"scheduled-omitted-1","payloadCapabilities":[],"payload":{"mode":"scheduled","schedule":"startup-delay"}}"#,
    )
    .unwrap();
    let omitted = AuthorizedRequestEnvelope {
      definition: &OPERATION,
      envelope: &omitted,
    }
    .decode::<ScheduledAutostartPayload>()
    .unwrap_err();
    assert_eq!(omitted.kind, ProtocolErrorKind::IncompatibleProtocolVersion);
  }
}
