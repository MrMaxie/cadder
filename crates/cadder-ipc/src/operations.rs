use crate::{
  CURRENT_PROTOCOL_VERSION, OperationPayload, ProtocolError, ProtocolResult, RawRequestEnvelope,
  RequestId, message_types,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationAccess {
  ReadOnly,
  Mutation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationShape {
  Unary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationDeadlineClass {
  Ordinary,
  Reload,
  Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationDefinition {
  name: &'static str,
  access: OperationAccess,
  deadline: OperationDeadlineClass,
  timeout_retryable: bool,
}

impl OperationDefinition {
  const fn new(
    name: &'static str,
    access: OperationAccess,
    deadline: OperationDeadlineClass,
    timeout_retryable: bool,
  ) -> Self {
    Self {
      name,
      access,
      deadline,
      timeout_retryable,
    }
  }

  pub const fn name(self) -> &'static str {
    self.name
  }

  pub const fn access(self) -> OperationAccess {
    self.access
  }

  pub const fn shape(self) -> OperationShape {
    OperationShape::Unary
  }

  pub const fn deadline(self) -> OperationDeadlineClass {
    self.deadline
  }

  pub const fn timeout_retryable(self) -> bool {
    self.timeout_retryable
  }
}

#[derive(Debug)]
pub struct AuthorizedRequestEnvelope<'a> {
  definition: &'static OperationDefinition,
  envelope: &'a RawRequestEnvelope,
}

impl AuthorizedRequestEnvelope<'_> {
  pub const fn definition(&self) -> &'static OperationDefinition {
    self.definition
  }

  pub fn request_id(&self) -> &RequestId {
    self.envelope.request_id()
  }

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
    self
      .envelope
      .decode_payload(true)
      .map_err(|error| error.with_request_id(self.envelope.request_id().clone()))
  }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OperationRegistry;

pub const OPERATION_REGISTRY: OperationRegistry = OperationRegistry;

impl OperationRegistry {
  pub fn iter(self) -> impl ExactSizeIterator<Item = &'static OperationDefinition> {
    OPERATIONS.iter()
  }

  pub fn lookup(self, name: &str) -> Option<&'static OperationDefinition> {
    OPERATIONS.iter().find(|operation| operation.name == name)
  }

  pub fn authorize_envelope<'a>(
    self,
    envelope: &'a RawRequestEnvelope,
  ) -> ProtocolResult<AuthorizedRequestEnvelope<'a>> {
    if envelope.protocol_version() != CURRENT_PROTOCOL_VERSION {
      return Err(
        ProtocolError::incompatible_protocol_version_pair(
          envelope.protocol_version(),
          CURRENT_PROTOCOL_VERSION,
        )
        .with_request_id(envelope.request_id().clone()),
      );
    }
    let definition = self.lookup(envelope.operation()).ok_or_else(|| {
      ProtocolError::unsupported_operation(envelope.operation(), Box::<[String]>::default())
        .with_request_id(envelope.request_id().clone())
    })?;
    Ok(AuthorizedRequestEnvelope {
      definition,
      envelope,
    })
  }
}

const OPERATIONS: &[OperationDefinition] = &[
  OperationDefinition::new(
    message_types::REGISTER_ENTRYPOINT_REQUEST,
    OperationAccess::Mutation,
    OperationDeadlineClass::Reload,
    false,
  ),
  OperationDefinition::new(
    message_types::UNREGISTER_ENTRYPOINT_REQUEST,
    OperationAccess::Mutation,
    OperationDeadlineClass::Reload,
    false,
  ),
  OperationDefinition::new(
    message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
    OperationAccess::Mutation,
    OperationDeadlineClass::Ordinary,
    true,
  ),
  OperationDefinition::new(
    message_types::QUERY_STATE_REQUEST,
    OperationAccess::ReadOnly,
    OperationDeadlineClass::Ordinary,
    true,
  ),
  OperationDefinition::new(
    message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
    OperationAccess::Mutation,
    OperationDeadlineClass::Reload,
    false,
  ),
  OperationDefinition::new(
    message_types::SET_DOMAIN_ENABLED_REQUEST,
    OperationAccess::Mutation,
    OperationDeadlineClass::Reload,
    false,
  ),
  OperationDefinition::new(
    message_types::QUERY_LOGS_REQUEST,
    OperationAccess::ReadOnly,
    OperationDeadlineClass::Ordinary,
    true,
  ),
  OperationDefinition::new(
    message_types::SHUTDOWN_DAEMON_REQUEST,
    OperationAccess::Mutation,
    OperationDeadlineClass::Shutdown,
    false,
  ),
];

#[cfg(test)]
mod tests;
