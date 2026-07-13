//! Closed, authenticated protocol for one runtime-guard control channel.

use crate::{
  ipc_codec::{BoundedNdjsonCodec, IpcCodecError, encode_json_frame},
  runtime_guard_record::{
    RuntimeGuardChildIdentity, RuntimeGuardGeneration, RuntimeGuardGenerationContext,
    RuntimeGuardIdentity, RuntimeGuardRecordState, RuntimeGuardTerminalOutcome,
  },
};
use bytes::BytesMut;
use serde::{Deserialize, Serialize};
use std::{fmt, io};
use tokio_util::codec::{Decoder, Encoder};

/// Wire revision shared by one daemon and its runtime guard.
pub const RUNTIME_GUARD_PROTOCOL_REVISION: u16 = 1;

/// Authenticated first frame for one runtime-guard generation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardBootstrapRequest {
  pub protocol_revision: u16,
  pub context: RuntimeGuardGenerationContext,
  pub request_id: u64,
  pub nonce: String,
}

impl fmt::Debug for RuntimeGuardBootstrapRequest {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("RuntimeGuardBootstrapRequest")
      .field("protocol_revision", &self.protocol_revision)
      .field("context", &self.context)
      .field("request_id", &self.request_id)
      .field("nonce", &"[REDACTED]")
      .finish()
  }
}

/// Fixed start command for the already-pinned Caddy image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardStartRequest {
  pub context: RuntimeGuardGenerationContext,
  pub request_id: u64,
  pub child_generation: String,
}

/// Context-bound command without an arbitrary payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardCommandRequest {
  pub context: RuntimeGuardGenerationContext,
  pub request_id: u64,
}

/// Complete set of commands accepted by the runtime guard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase")]
pub enum RuntimeGuardRequest {
  Bootstrap(RuntimeGuardBootstrapRequest),
  Start(RuntimeGuardStartRequest),
  Terminate(RuntimeGuardCommandRequest),
  Status(RuntimeGuardCommandRequest),
  Finalize(RuntimeGuardCommandRequest),
}

/// Bootstrap acknowledgement emitted after nonce authentication and before lock handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardBootstrapAuthenticatedResponse {
  pub protocol_revision: u16,
  pub context: RuntimeGuardGenerationContext,
  pub request_id: u64,
}

/// Readiness acknowledgement emitted only after the guard owns the lock and Ready record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardReadyResponse {
  pub context: RuntimeGuardGenerationContext,
  pub request_id: u64,
  pub guard: RuntimeGuardIdentity,
}

/// Start acknowledgement for the exact child created by the guard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardStartedResponse {
  pub context: RuntimeGuardGenerationContext,
  pub request_id: u64,
  pub child: RuntimeGuardChildIdentity,
}

/// Termination acknowledgement after joining the exact owned tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardTerminatedResponse {
  pub context: RuntimeGuardGenerationContext,
  pub request_id: u64,
  pub child_exit_code: Option<i32>,
}

/// Typed status snapshot for the active guard generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardStatusResponse {
  pub context: RuntimeGuardGenerationContext,
  pub request_id: u64,
  pub state: RuntimeGuardRecordState,
  pub guard: RuntimeGuardIdentity,
  pub child: Option<RuntimeGuardChildIdentity>,
}

/// Final acknowledgement after the terminal record is durable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardFinalizedResponse {
  pub context: RuntimeGuardGenerationContext,
  pub request_id: u64,
  pub terminal: RuntimeGuardTerminalOutcome,
}

/// Complete set of successful responses emitted by the runtime guard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum RuntimeGuardResponse {
  BootstrapAuthenticated(RuntimeGuardBootstrapAuthenticatedResponse),
  GuardReady(RuntimeGuardReadyResponse),
  Started(RuntimeGuardStartedResponse),
  Terminated(RuntimeGuardTerminatedResponse),
  Status(RuntimeGuardStatusResponse),
  Finalized(RuntimeGuardFinalizedResponse),
}

/// Fail-closed protocol error. The channel is unusable after any error.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeGuardProtocolError {
  #[error("runtime guard frame exceeds the 1 MiB limit")]
  FrameTooLarge,
  #[error("runtime guard frame is malformed or uses an unknown operation or field")]
  InvalidFrame,
  #[error("runtime guard bootstrap authentication failed")]
  AuthenticationFailed,
  #[error("runtime guard frame belongs to another generation")]
  CrossGeneration,
  #[error("runtime guard request ID is duplicate, zero, or nonmonotonic")]
  InvalidRequestOrder,
  #[error("runtime guard operation is invalid for the current protocol state")]
  InvalidOperationState,
  #[error("runtime guard response does not match the pending request")]
  ResponseMismatch,
  #[error("runtime guard protocol is unusable after an earlier error")]
  Poisoned,
  #[error("runtime guard framing I/O failed")]
  Io(#[from] io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeGuardOperation {
  Bootstrap,
  Start,
  Terminate,
  Status,
  Finalize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingRequest {
  operation: RuntimeGuardOperation,
  request_id: u64,
}

/// Stateful NDJSON codec for one authenticated runtime-guard connection.
#[derive(Debug)]
pub struct RuntimeGuardProtocol {
  codec: BoundedNdjsonCodec,
  expected_context: RuntimeGuardGenerationContext,
  pending: Option<PendingRequest>,
  last_request_id: Option<u64>,
  authenticated: bool,
  bootstrap_acknowledged: bool,
  ready: bool,
  finalized: bool,
  poisoned: bool,
}

impl RuntimeGuardProtocol {
  /// Creates a protocol bound to one immutable daemon generation.
  pub fn new(expected_context: RuntimeGuardGenerationContext) -> Self {
    Self {
      codec: BoundedNdjsonCodec::new(),
      expected_context,
      pending: None,
      last_request_id: None,
      authenticated: false,
      bootstrap_acknowledged: false,
      ready: false,
      finalized: false,
      poisoned: false,
    }
  }

  fn fail<T>(&mut self, error: RuntimeGuardProtocolError) -> Result<T, RuntimeGuardProtocolError> {
    self.poisoned = true;
    Err(error)
  }

  fn decode_frame(
    &mut self,
    frame: &str,
  ) -> Result<RuntimeGuardRequest, RuntimeGuardProtocolError> {
    let request = match serde_json::from_str::<RuntimeGuardRequest>(frame) {
      Ok(request) => request,
      Err(_) => return self.fail(RuntimeGuardProtocolError::InvalidFrame),
    };
    if self.pending.is_some() || self.finalized {
      return self.fail(RuntimeGuardProtocolError::InvalidOperationState);
    }

    let (operation, context, request_id) = request_metadata(&request);
    if context != &self.expected_context {
      return self.fail(RuntimeGuardProtocolError::CrossGeneration);
    }
    if request_id == 0
      || self
        .last_request_id
        .is_some_and(|last_request_id| request_id <= last_request_id)
    {
      return self.fail(RuntimeGuardProtocolError::InvalidRequestOrder);
    }

    match &request {
      RuntimeGuardRequest::Bootstrap(bootstrap) => {
        if self.authenticated
          || bootstrap.protocol_revision != RUNTIME_GUARD_PROTOCOL_REVISION
          || !RuntimeGuardGeneration::nonce_matches(
            &bootstrap.nonce,
            &self.expected_context.nonce_commitment,
          )
        {
          return self.fail(RuntimeGuardProtocolError::AuthenticationFailed);
        }
        self.authenticated = true;
      }
      RuntimeGuardRequest::Start(start) => {
        if !self.ready || !valid_generation_id(&start.child_generation) {
          return self.fail(RuntimeGuardProtocolError::InvalidOperationState);
        }
      }
      RuntimeGuardRequest::Terminate(_)
      | RuntimeGuardRequest::Status(_)
      | RuntimeGuardRequest::Finalize(_) => {
        if !self.ready {
          return self.fail(RuntimeGuardProtocolError::InvalidOperationState);
        }
      }
    }

    self.last_request_id = Some(request_id);
    self.pending = Some(PendingRequest {
      operation,
      request_id,
    });
    Ok(request)
  }

  fn encode_response(
    &mut self,
    response: &RuntimeGuardResponse,
    destination: &mut BytesMut,
  ) -> Result<(), RuntimeGuardProtocolError> {
    if self.poisoned {
      return Err(RuntimeGuardProtocolError::Poisoned);
    }
    let Some(pending) = self.pending else {
      return self.fail(RuntimeGuardProtocolError::ResponseMismatch);
    };
    let (operation, context, request_id) = response_metadata(response);
    if operation != pending.operation
      || request_id != pending.request_id
      || context != &self.expected_context
    {
      return self.fail(RuntimeGuardProtocolError::ResponseMismatch);
    }
    match response {
      RuntimeGuardResponse::BootstrapAuthenticated(response) => {
        if !self.authenticated
          || self.bootstrap_acknowledged
          || response.protocol_revision != RUNTIME_GUARD_PROTOCOL_REVISION
        {
          return self.fail(RuntimeGuardProtocolError::ResponseMismatch);
        }
      }
      RuntimeGuardResponse::GuardReady(_) => {
        if !self.authenticated || !self.bootstrap_acknowledged || self.ready {
          return self.fail(RuntimeGuardProtocolError::ResponseMismatch);
        }
      }
      RuntimeGuardResponse::Started(_)
      | RuntimeGuardResponse::Terminated(_)
      | RuntimeGuardResponse::Status(_)
      | RuntimeGuardResponse::Finalized(_) => {}
    }
    let encoded = match encode_json_frame(response) {
      Ok(encoded) => encoded,
      Err(error) => return self.fail(map_codec_error(error)),
    };
    destination.extend_from_slice(&encoded);
    match response {
      RuntimeGuardResponse::BootstrapAuthenticated(_) => {
        self.bootstrap_acknowledged = true;
      }
      RuntimeGuardResponse::GuardReady(_) => {
        self.ready = true;
        self.pending = None;
      }
      RuntimeGuardResponse::Finalized(_) => {
        self.pending = None;
        self.finalized = true;
      }
      RuntimeGuardResponse::Started(_)
      | RuntimeGuardResponse::Terminated(_)
      | RuntimeGuardResponse::Status(_) => {
        self.pending = None;
      }
    }
    Ok(())
  }

  fn map_decode_result(
    &mut self,
    result: Result<Option<String>, IpcCodecError>,
  ) -> Result<Option<RuntimeGuardRequest>, RuntimeGuardProtocolError> {
    match result {
      Ok(Some(frame)) => self.decode_frame(&frame).map(Some),
      Ok(None) => Ok(None),
      Err(error) => self.fail(map_codec_error(error)),
    }
  }
}

impl Decoder for RuntimeGuardProtocol {
  type Item = RuntimeGuardRequest;
  type Error = RuntimeGuardProtocolError;

  fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
    if self.poisoned {
      return Err(RuntimeGuardProtocolError::Poisoned);
    }
    let result = self.codec.decode(source);
    self.map_decode_result(result)
  }

  fn decode_eof(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
    if self.poisoned {
      return Err(RuntimeGuardProtocolError::Poisoned);
    }
    let result = self.codec.decode_eof(source);
    self.map_decode_result(result)
  }
}

impl Encoder<RuntimeGuardResponse> for RuntimeGuardProtocol {
  type Error = RuntimeGuardProtocolError;

  fn encode(
    &mut self,
    response: RuntimeGuardResponse,
    destination: &mut BytesMut,
  ) -> Result<(), Self::Error> {
    self.encode_response(&response, destination)
  }
}

fn request_metadata(
  request: &RuntimeGuardRequest,
) -> (RuntimeGuardOperation, &RuntimeGuardGenerationContext, u64) {
  match request {
    RuntimeGuardRequest::Bootstrap(request) => (
      RuntimeGuardOperation::Bootstrap,
      &request.context,
      request.request_id,
    ),
    RuntimeGuardRequest::Start(request) => (
      RuntimeGuardOperation::Start,
      &request.context,
      request.request_id,
    ),
    RuntimeGuardRequest::Terminate(request) => (
      RuntimeGuardOperation::Terminate,
      &request.context,
      request.request_id,
    ),
    RuntimeGuardRequest::Status(request) => (
      RuntimeGuardOperation::Status,
      &request.context,
      request.request_id,
    ),
    RuntimeGuardRequest::Finalize(request) => (
      RuntimeGuardOperation::Finalize,
      &request.context,
      request.request_id,
    ),
  }
}

fn response_metadata(
  response: &RuntimeGuardResponse,
) -> (RuntimeGuardOperation, &RuntimeGuardGenerationContext, u64) {
  match response {
    RuntimeGuardResponse::BootstrapAuthenticated(response) => (
      RuntimeGuardOperation::Bootstrap,
      &response.context,
      response.request_id,
    ),
    RuntimeGuardResponse::GuardReady(response) => (
      RuntimeGuardOperation::Bootstrap,
      &response.context,
      response.request_id,
    ),
    RuntimeGuardResponse::Started(response) => (
      RuntimeGuardOperation::Start,
      &response.context,
      response.request_id,
    ),
    RuntimeGuardResponse::Terminated(response) => (
      RuntimeGuardOperation::Terminate,
      &response.context,
      response.request_id,
    ),
    RuntimeGuardResponse::Status(response) => (
      RuntimeGuardOperation::Status,
      &response.context,
      response.request_id,
    ),
    RuntimeGuardResponse::Finalized(response) => (
      RuntimeGuardOperation::Finalize,
      &response.context,
      response.request_id,
    ),
  }
}

fn map_codec_error(error: IpcCodecError) -> RuntimeGuardProtocolError {
  match error {
    IpcCodecError::FrameTooLarge => RuntimeGuardProtocolError::FrameTooLarge,
    IpcCodecError::Io(error) => RuntimeGuardProtocolError::Io(error),
    IpcCodecError::EmbeddedDelimiter
    | IpcCodecError::InvalidUtf8(_)
    | IpcCodecError::Serialization(_)
    | IpcCodecError::UnterminatedFrame
    | IpcCodecError::Poisoned => RuntimeGuardProtocolError::InvalidFrame,
  }
}

fn valid_generation_id(value: &str) -> bool {
  value.len() == 32
    && value
      .bytes()
      .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    ipc_codec::MAX_IPC_FRAME_LENGTH,
    runtime_guard_record::{
      RuntimeGuardImageIdentity, RuntimeGuardPinnedCaddyIdentity, RuntimeGuardProcessIdentity,
    },
  };

  #[test]
  fn runtime_guard_protocol_rejects_wrong_bootstrap_nonce_and_poisoned_reuse() {
    let (mut protocol, context, _nonce) = protocol_fixture();
    let request = RuntimeGuardRequest::Bootstrap(RuntimeGuardBootstrapRequest {
      protocol_revision: RUNTIME_GUARD_PROTOCOL_REVISION,
      context,
      request_id: 1,
      nonce: "00".repeat(32),
    });

    let error = protocol.decode(&mut frame(&request)).unwrap_err();

    assert!(matches!(
      error,
      RuntimeGuardProtocolError::AuthenticationFailed
    ));
    assert!(matches!(
      protocol.decode(&mut BytesMut::new()),
      Err(RuntimeGuardProtocolError::Poisoned)
    ));
  }

  #[test]
  fn runtime_guard_protocol_rejects_oversized_frame() {
    let (mut protocol, _context, _nonce) = protocol_fixture();
    let mut oversized = BytesMut::from(vec![b'x'; MAX_IPC_FRAME_LENGTH + 1].as_slice());

    let error = protocol.decode(&mut oversized).unwrap_err();

    assert!(matches!(error, RuntimeGuardProtocolError::FrameTooLarge));
  }

  #[test]
  fn runtime_guard_protocol_rejects_unknown_operation_and_field() {
    let (mut unknown_operation, _context, _nonce) = protocol_fixture();
    let mut operation = BytesMut::from(&b"{\"operation\":\"execute\"}\n"[..]);
    assert!(matches!(
      unknown_operation.decode(&mut operation),
      Err(RuntimeGuardProtocolError::InvalidFrame)
    ));

    let (mut unknown_field, context, nonce) = protocol_fixture();
    let mut value = serde_json::to_value(bootstrap(context, nonce)).unwrap();
    value
      .as_object_mut()
      .unwrap()
      .insert("cwd".to_string(), serde_json::json!("C:/untrusted"));
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    assert!(matches!(
      unknown_field.decode(&mut BytesMut::from(bytes.as_slice())),
      Err(RuntimeGuardProtocolError::InvalidFrame)
    ));
  }

  #[test]
  fn runtime_guard_protocol_rejects_cross_generation_command() {
    let (mut protocol, context, nonce) = protocol_fixture();
    complete_bootstrap(&mut protocol, context.clone(), nonce);
    let mut wrong_context = context;
    wrong_context.owner_generation = "aa".repeat(16);
    let request = RuntimeGuardRequest::Status(RuntimeGuardCommandRequest {
      context: wrong_context,
      request_id: 2,
    });

    let error = protocol.decode(&mut frame(&request)).unwrap_err();

    assert!(matches!(error, RuntimeGuardProtocolError::CrossGeneration));
  }

  #[test]
  fn runtime_guard_protocol_requires_authenticated_then_ready_bootstrap_phases() {
    let (mut command_before_ready, context, nonce) = protocol_fixture();
    command_before_ready
      .decode(&mut frame(&bootstrap(context.clone(), nonce)))
      .unwrap()
      .unwrap();
    let status = RuntimeGuardRequest::Status(RuntimeGuardCommandRequest {
      context: context.clone(),
      request_id: 2,
    });
    assert!(matches!(
      command_before_ready.decode(&mut frame(&status)),
      Err(RuntimeGuardProtocolError::InvalidOperationState)
    ));

    let (mut ready_before_authenticated, context, nonce) = protocol_fixture();
    ready_before_authenticated
      .decode(&mut frame(&bootstrap(context.clone(), nonce)))
      .unwrap()
      .unwrap();
    let ready = RuntimeGuardResponse::GuardReady(RuntimeGuardReadyResponse {
      context,
      request_id: 1,
      guard: guard_identity(),
    });
    assert!(matches!(
      ready_before_authenticated.encode(ready, &mut BytesMut::new()),
      Err(RuntimeGuardProtocolError::ResponseMismatch)
    ));
  }

  #[test]
  fn runtime_guard_protocol_rejects_duplicate_and_nonmonotonic_request_ids() {
    let (mut duplicate, context, nonce) = protocol_fixture();
    complete_bootstrap(&mut duplicate, context.clone(), nonce);
    complete_status(&mut duplicate, context.clone(), 2);
    let duplicate_request = RuntimeGuardRequest::Status(RuntimeGuardCommandRequest {
      context: context.clone(),
      request_id: 2,
    });
    assert!(matches!(
      duplicate.decode(&mut frame(&duplicate_request)),
      Err(RuntimeGuardProtocolError::InvalidRequestOrder)
    ));

    let (mut nonmonotonic, context, nonce) = protocol_fixture();
    complete_bootstrap(&mut nonmonotonic, context.clone(), nonce);
    complete_status(&mut nonmonotonic, context.clone(), 3);
    let earlier_request = RuntimeGuardRequest::Terminate(RuntimeGuardCommandRequest {
      context,
      request_id: 2,
    });
    assert!(matches!(
      nonmonotonic.decode(&mut frame(&earlier_request)),
      Err(RuntimeGuardProtocolError::InvalidRequestOrder)
    ));
  }

  #[test]
  fn runtime_guard_protocol_exposes_only_closed_typed_operations() {
    let (mut protocol, context, nonce) = protocol_fixture();
    complete_bootstrap(&mut protocol, context.clone(), nonce);
    let start = RuntimeGuardRequest::Start(RuntimeGuardStartRequest {
      context: context.clone(),
      request_id: 2,
      child_generation: "1234567890abcdef1234567890abcdef".to_string(),
    });

    let decoded = protocol.decode(&mut frame(&start)).unwrap().unwrap();

    assert_eq!(decoded, start);
    let response = RuntimeGuardResponse::Started(RuntimeGuardStartedResponse {
      context,
      request_id: 2,
      child: child_identity(),
    });
    let mut output = BytesMut::new();
    protocol.encode(response, &mut output).unwrap();
    assert!(output.ends_with(b"\n"));
  }

  #[test]
  fn runtime_guard_protocol_rejects_start_execution_parameters() {
    let (mut protocol, context, nonce) = protocol_fixture();
    complete_bootstrap(&mut protocol, context.clone(), nonce);
    let start = RuntimeGuardRequest::Start(RuntimeGuardStartRequest {
      context,
      request_id: 2,
      child_generation: "1234567890abcdef1234567890abcdef".to_string(),
    });
    let mut value = serde_json::to_value(start).unwrap();
    let object = value.as_object_mut().unwrap();
    object.insert("executable".to_string(), serde_json::json!("other.exe"));
    object.insert("argv".to_string(), serde_json::json!(["run"]));
    object.insert("env".to_string(), serde_json::json!({"TOKEN": "value"}));
    object.insert("cwd".to_string(), serde_json::json!("C:/untrusted"));
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');

    let error = protocol
      .decode(&mut BytesMut::from(bytes.as_slice()))
      .unwrap_err();

    assert!(matches!(error, RuntimeGuardProtocolError::InvalidFrame));
  }

  fn protocol_fixture() -> (RuntimeGuardProtocol, RuntimeGuardGenerationContext, String) {
    let generation = RuntimeGuardGeneration::random().unwrap();
    let context = RuntimeGuardGenerationContext {
      profile: "default".to_string(),
      runtime_id: "0123456789abcdef".to_string(),
      daemon_instance_id: "00112233445566778899aabbccddeeff".to_string(),
      owner_generation: "ffeeddccbbaa99887766554433221100".to_string(),
      nonce_commitment: generation.commitment().to_string(),
    };
    (
      RuntimeGuardProtocol::new(context.clone()),
      context,
      generation.nonce(),
    )
  }

  fn bootstrap(context: RuntimeGuardGenerationContext, nonce: String) -> RuntimeGuardRequest {
    RuntimeGuardRequest::Bootstrap(RuntimeGuardBootstrapRequest {
      protocol_revision: RUNTIME_GUARD_PROTOCOL_REVISION,
      context,
      request_id: 1,
      nonce,
    })
  }

  fn complete_bootstrap(
    protocol: &mut RuntimeGuardProtocol,
    context: RuntimeGuardGenerationContext,
    nonce: String,
  ) {
    protocol
      .decode(&mut frame(&bootstrap(context.clone(), nonce)))
      .unwrap()
      .unwrap();
    let authenticated =
      RuntimeGuardResponse::BootstrapAuthenticated(RuntimeGuardBootstrapAuthenticatedResponse {
        protocol_revision: RUNTIME_GUARD_PROTOCOL_REVISION,
        context: context.clone(),
        request_id: 1,
      });
    protocol
      .encode(authenticated, &mut BytesMut::new())
      .unwrap();
    let ready = RuntimeGuardResponse::GuardReady(RuntimeGuardReadyResponse {
      context,
      request_id: 1,
      guard: guard_identity(),
    });
    protocol.encode(ready, &mut BytesMut::new()).unwrap();
  }

  fn complete_status(
    protocol: &mut RuntimeGuardProtocol,
    context: RuntimeGuardGenerationContext,
    request_id: u64,
  ) {
    let request = RuntimeGuardRequest::Status(RuntimeGuardCommandRequest {
      context: context.clone(),
      request_id,
    });
    protocol.decode(&mut frame(&request)).unwrap().unwrap();
    let response = RuntimeGuardResponse::Status(RuntimeGuardStatusResponse {
      context,
      request_id,
      state: RuntimeGuardRecordState::Ready,
      guard: guard_identity(),
      child: None,
    });
    protocol.encode(response, &mut BytesMut::new()).unwrap();
  }

  fn frame(request: &RuntimeGuardRequest) -> BytesMut {
    BytesMut::from(encode_json_frame(request).unwrap().as_slice())
  }

  fn guard_identity() -> RuntimeGuardIdentity {
    RuntimeGuardIdentity {
      process: RuntimeGuardProcessIdentity {
        process_id: std::process::id(),
        creation_identity: "windows-creation-time-123".to_string(),
      },
      image: RuntimeGuardImageIdentity {
        path: std::env::current_exe().unwrap(),
        file_identity: "volume-1-file-2".to_string(),
        sha256: "22".repeat(32),
      },
    }
  }

  fn child_identity() -> RuntimeGuardChildIdentity {
    RuntimeGuardChildIdentity {
      child_generation: "1234567890abcdef1234567890abcdef".to_string(),
      process: RuntimeGuardProcessIdentity {
        process_id: 42,
        creation_identity: "windows-creation-time-456".to_string(),
      },
      pinned_caddy: RuntimeGuardPinnedCaddyIdentity {
        image: RuntimeGuardImageIdentity {
          path: std::env::current_exe().unwrap(),
          file_identity: "volume-1-file-3".to_string(),
          sha256: "33".repeat(32),
        },
        version: "2.11.3".to_string(),
        probe_revision: "cadder-v1-probe-1".to_string(),
      },
    }
  }
}
