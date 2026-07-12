use crate::{
  CapabilityId, PROTOCOL_VERSION_1_0, ProtocolCapabilities, ProtocolError, ProtocolResult,
  ProtocolVersion, ProtocolVersionRange, SUPPORTED_PROTOCOL_VERSIONS, capabilities,
  current_capabilities, ensure_compatible_protocol_version, message_types,
};

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
}

impl OperationDefinition {
  const fn new(
    name: &'static str,
    required_capability: &'static str,
    access: OperationAccess,
    shape: OperationShape,
    deadline: OperationDeadlineClass,
    timeout_retryable: bool,
  ) -> Self {
    Self {
      name,
      minimum_version: PROTOCOL_VERSION_1_0,
      required_capability,
      access,
      shape,
      deadline,
      timeout_retryable,
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

  /// Returns every capability advertised by this build in stable order.
  pub fn advertised_capabilities(self) -> Box<[CapabilityId]> {
    capabilities::ALL
      .iter()
      .map(|capability| CapabilityId::known(capability))
      .collect()
  }

  /// Intersects requested capabilities with this build's stable advertised set.
  pub fn negotiate_capabilities(self, requested: &[CapabilityId]) -> Box<[CapabilityId]> {
    capabilities::ALL
      .iter()
      .filter(|supported| {
        requested
          .iter()
          .any(|requested| requested.as_str() == **supported)
      })
      .map(|capability| CapabilityId::known(capability))
      .collect()
  }

  /// Gates a post-handshake operation before its typed payload is decoded.
  pub fn authorize(
    self,
    name: &str,
    version: ProtocolVersion,
    negotiated_capabilities: &[CapabilityId],
  ) -> ProtocolResult<&'static OperationDefinition> {
    let offered = ProtocolVersionRange::exact(version);
    if SUPPORTED_PROTOCOL_VERSIONS.negotiate(offered) != Some(version) {
      return Err(ProtocolError::incompatible_protocol_range(
        offered,
        SUPPORTED_PROTOCOL_VERSIONS,
      ));
    }
    let operation = self
      .lookup(name)
      .ok_or_else(|| unsupported_versioned_operation(name))?;
    if version.major() != operation.minimum_version.major() || version < operation.minimum_version {
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

const OPERATIONS: &[OperationDefinition] = &[
  OperationDefinition::new(
    message_types::REGISTER_ENTRYPOINT_REQUEST,
    capabilities::ENTRYPOINT_REGISTRATION,
    OperationAccess::Mutation,
    OperationShape::Unary,
    OperationDeadlineClass::Reload,
    false,
  ),
  OperationDefinition::new(
    message_types::UNREGISTER_ENTRYPOINT_REQUEST,
    capabilities::ENTRYPOINT_REGISTRATION,
    OperationAccess::Mutation,
    OperationShape::Unary,
    OperationDeadlineClass::Reload,
    false,
  ),
  OperationDefinition::new(
    message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
    capabilities::ENTRYPOINT_REGISTRATION,
    OperationAccess::Mutation,
    OperationShape::Unary,
    OperationDeadlineClass::Ordinary,
    true,
  ),
  OperationDefinition::new(
    message_types::QUERY_STATE_REQUEST,
    capabilities::RUNTIME_STATE,
    OperationAccess::ReadOnly,
    OperationShape::Unary,
    OperationDeadlineClass::Ordinary,
    true,
  ),
  OperationDefinition::new(
    message_types::SUBSCRIBE_STATE_REQUEST,
    capabilities::STATE_SUBSCRIPTION,
    OperationAccess::ReadOnly,
    OperationShape::ServerStream,
    OperationDeadlineClass::Stream,
    true,
  ),
  OperationDefinition::new(
    message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
    capabilities::ACTIVATION_CONTROL,
    OperationAccess::Mutation,
    OperationShape::Unary,
    OperationDeadlineClass::Reload,
    false,
  ),
  OperationDefinition::new(
    message_types::SET_DOMAIN_ENABLED_REQUEST,
    capabilities::ACTIVATION_CONTROL,
    OperationAccess::Mutation,
    OperationShape::Unary,
    OperationDeadlineClass::Reload,
    false,
  ),
  OperationDefinition::new(
    message_types::QUERY_IIS_BINDINGS_REQUEST,
    capabilities::IIS_HANDOFF,
    OperationAccess::ReadOnly,
    OperationShape::Unary,
    OperationDeadlineClass::Ordinary,
    true,
  ),
  OperationDefinition::new(
    message_types::SET_IIS_HANDOFF_REQUEST,
    capabilities::IIS_HANDOFF,
    OperationAccess::Mutation,
    OperationShape::Unary,
    OperationDeadlineClass::Reload,
    false,
  ),
  OperationDefinition::new(
    message_types::QUERY_LOGS_REQUEST,
    capabilities::LOGS,
    OperationAccess::ReadOnly,
    OperationShape::Unary,
    OperationDeadlineClass::Ordinary,
    true,
  ),
  OperationDefinition::new(
    message_types::QUERY_HISTORY_REQUEST,
    capabilities::HISTORY,
    OperationAccess::ReadOnly,
    OperationShape::Unary,
    OperationDeadlineClass::Ordinary,
    true,
  ),
  OperationDefinition::new(
    message_types::QUERY_AUTOSTART_REQUEST,
    capabilities::AUTOSTART,
    OperationAccess::ReadOnly,
    OperationShape::Unary,
    OperationDeadlineClass::Ordinary,
    true,
  ),
  OperationDefinition::new(
    message_types::SET_AUTOSTART_REQUEST,
    capabilities::AUTOSTART,
    OperationAccess::Mutation,
    OperationShape::Unary,
    OperationDeadlineClass::Ordinary,
    false,
  ),
  OperationDefinition::new(
    message_types::SHUTDOWN_DAEMON_REQUEST,
    capabilities::DAEMON_LIFECYCLE,
    OperationAccess::Mutation,
    OperationShape::Unary,
    OperationDeadlineClass::Shutdown,
    false,
  ),
];
