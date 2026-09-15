use super::*;

#[derive(Debug)]
pub(in crate::ipc) struct PreparedClientRequest {
  pub(in crate::ipc) context: ClientRequestContext,
  frame: Box<[u8]>,
}

#[derive(Debug)]
pub(in crate::ipc) struct ClientRequestContext {
  pub(in crate::ipc) operation: Box<str>,
  pub(in crate::ipc) request_id: RequestId,
}

impl PreparedClientRequest {
  pub(in crate::ipc) fn new<T>(request_id: &str, request: &T) -> IpcClientResult<Self>
  where
    T: CorrelatedRequest,
  {
    let operation = <T as cadder_ipc::OperationPayload>::OPERATION;
    let request_id = RequestId::parse(request_id).map_err(|error| {
      IpcClientError::local(LocalIpcErrorContext {
        kind: LocalIpcErrorKind::Transport,
        phase: IpcClientPhase::RequestEncode,
        code: LocalIpcErrorCode::InvalidRequest,
        message: "Cadder rejected an invalid local request before sending it.".into(),
        guidance: Some("Correct the request ID and retry the operation.".into()),
        retryable: false,
        request_id: None,
        operation: Some(operation.into()),
        source: Some(Box::new(error)),
      })
    })?;
    let context = ClientRequestContext {
      operation: operation.into(),
      request_id: request_id.clone(),
    };
    let envelope = cadder_ipc::RequestEnvelope::new(
      CURRENT_PROTOCOL_VERSION,
      request_id.clone(),
      request.clone(),
    );
    let frame =
      encode_json_frame(&envelope).map_err(|error| request_frame_error(&context, error))?;
    Ok(Self {
      context,
      frame: frame.into_boxed_slice(),
    })
  }
}

pub(in crate::ipc) async fn write_prepared_request_until<W>(
  writer: &mut W,
  request: &PreparedClientRequest,
  deadline: tokio::time::Instant,
) -> IpcClientResult<()>
where
  W: AsyncWrite + Unpin,
{
  match tokio::time::timeout_at(deadline, write_prepared_request(writer, request)).await {
    Ok(result) => result,
    Err(_) => Err(request_write_timeout_error(
      &request.context,
      operation_retryable(&request.context.operation),
    )),
  }
}

pub(in crate::ipc) async fn write_prepared_request<W>(
  writer: &mut W,
  request: &PreparedClientRequest,
) -> IpcClientResult<()>
where
  W: AsyncWrite + Unpin,
{
  writer
    .write_all(&request.frame)
    .await
    .map_err(|error| request_write_error(&request.context, error))?;
  writer
    .flush()
    .await
    .map_err(|error| request_write_error(&request.context, error))
}

pub(in crate::ipc) async fn read_client_response<R, T>(
  reader: &mut FramedRead<R, BoundedNdjsonCodec>,
  request: &ClientRequestContext,
) -> IpcClientResult<T>
where
  R: AsyncRead + Unpin,
  T: DeserializeOwned,
{
  let line = match reader.next().await {
    Some(Ok(line)) => line,
    Some(Err(IpcCodecError::Io(error))) => return Err(response_read_error(request, error)),
    Some(Err(error)) => return Err(response_frame_error(request, error)),
    None => return Err(response_eof_error(request)),
  };
  let envelope: cadder_ipc::ResponseEnvelope<T> =
    serde_json::from_str(&line).map_err(|error| response_decode_error(request, error))?;
  if envelope.protocol_version() != CURRENT_PROTOCOL_VERSION {
    return Err(response_validation_error(
      request,
      "The daemon response uses a different exact protocol version; the operation outcome is unknown.",
    ));
  }
  let failed = matches!(envelope.outcome(), cadder_ipc::ResponseOutcome::Failure(_));
  if !failed && envelope.operation() != request.operation.as_ref() {
    return Err(response_validation_error(
      request,
      "The daemon response names a different operation; the operation outcome is unknown.",
    ));
  }
  if envelope.request_id() != &request.request_id {
    Err(response_validation_error(
      request,
      "The daemon response belongs to a different request; the operation outcome is unknown.",
    ))
  } else {
    envelope.into_result().map_err(IpcClientError::daemon)
  }
}

pub(in crate::ipc) fn endpoint_resolution_error(error: io::Error) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::EndpointResolve,
    code: LocalIpcErrorCode::InvalidEndpoint,
    message: "Cadder could not resolve the local daemon endpoint; no request was sent.".into(),
    guidance: Some("Select a valid Cadder runtime directory, then retry.".into()),
    retryable: false,
    request_id: None,
    operation: None,
    source: Some(Box::new(error)),
  })
}

pub(in crate::ipc) fn connection_error(error: io::Error) -> IpcClientError {
  let (kind, code, message, guidance, retryable) = match error.kind() {
    io::ErrorKind::PermissionDenied => (
      LocalIpcErrorKind::Transport,
      LocalIpcErrorCode::PermissionDenied,
      "Cadder cannot access the selected daemon endpoint; no request was sent.",
      "Use the account that owns this Cadder runtime or select an accessible profile.",
      false,
    ),
    io::ErrorKind::TimedOut => (
      LocalIpcErrorKind::Timeout,
      LocalIpcErrorCode::Timeout,
      "The Cadder daemon connection did not complete before the local deadline; no request was sent.",
      "Check the daemon status, then retry once it is ready.",
      true,
    ),
    kind if daemon_not_ready_error(kind) => (
      LocalIpcErrorKind::Transport,
      LocalIpcErrorCode::DaemonUnavailable,
      "The Cadder daemon is unavailable; no request was sent.",
      "Start the Cadder daemon for this runtime, then retry.",
      true,
    ),
    _ => (
      LocalIpcErrorKind::Transport,
      LocalIpcErrorCode::TransportConnect,
      "Cadder could not connect to the selected daemon endpoint; no request was sent.",
      "Inspect the local IPC endpoint and runtime diagnostics before retrying.",
      false,
    ),
  };
  IpcClientError::local(LocalIpcErrorContext {
    kind,
    phase: IpcClientPhase::Connect,
    code,
    message: message.into(),
    guidance: Some(guidance.into()),
    retryable,
    request_id: None,
    operation: None,
    source: Some(Box::new(error)),
  })
}

pub(in crate::ipc) fn connection_timeout_error() -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Timeout,
    phase: IpcClientPhase::Connect,
    code: LocalIpcErrorCode::Timeout,
    message: "Cadder could not confirm the local daemon endpoint before the connection deadline; no request was sent."
      .into(),
    guidance: Some("Check the daemon status, then retry once it is ready.".into()),
    retryable: true,
    request_id: None,
    operation: Some(CLIENT_HELLO_OPERATION.into()),
    source: None,
  })
}

pub(in crate::ipc) fn peer_authentication_preface_error(error: io::Error) -> IpcClientError {
  let (kind, code, guidance, retryable) = match error.kind() {
    io::ErrorKind::PermissionDenied => (
      LocalIpcErrorKind::Transport,
      LocalIpcErrorCode::PermissionDenied,
      "Use the account that owns this Cadder runtime or select an accessible profile.",
      false,
    ),
    io::ErrorKind::TimedOut => (
      LocalIpcErrorKind::Timeout,
      LocalIpcErrorCode::Timeout,
      "Check the daemon status, then retry once it is ready.",
      true,
    ),
    kind if daemon_not_ready_error(kind) => (
      LocalIpcErrorKind::Transport,
      LocalIpcErrorCode::DaemonUnavailable,
      "Start the Cadder daemon for this runtime, then retry.",
      true,
    ),
    _ => (
      LocalIpcErrorKind::Transport,
      LocalIpcErrorCode::TransportConnect,
      "Inspect the local IPC endpoint and runtime diagnostics before retrying.",
      false,
    ),
  };
  IpcClientError::local(LocalIpcErrorContext {
    kind,
    phase: IpcClientPhase::Connect,
    code,
    message: "Cadder could not open a connection to the selected runtime; no request was sent."
      .into(),
    guidance: Some(guidance.into()),
    retryable,
    request_id: None,
    operation: None,
    source: Some(Box::new(error)),
  })
}

pub(in crate::ipc) fn daemon_not_ready_error(kind: io::ErrorKind) -> bool {
  matches!(
    kind,
    io::ErrorKind::NotFound
      | io::ErrorKind::ConnectionRefused
      | io::ErrorKind::ConnectionReset
      | io::ErrorKind::ConnectionAborted
      | io::ErrorKind::NotConnected
      | io::ErrorKind::AddrNotAvailable
  )
}

pub(in crate::ipc) fn request_frame_error(
  request: &ClientRequestContext,
  error: IpcCodecError,
) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::RequestEncode,
    code: LocalIpcErrorCode::Frame,
    message: "Cadder rejected a request that does not fit one bounded IPC frame; nothing was sent."
      .into(),
    guidance: Some("Reduce the request payload below 1 MiB and retry the operation.".into()),
    retryable: false,
    request_id: Some(request.request_id.clone()),
    operation: Some(request.operation.clone()),
    source: Some(Box::new(error)),
  })
}

pub(in crate::ipc) fn request_write_error(
  request: &ClientRequestContext,
  error: io::Error,
) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::RequestWrite,
    code: LocalIpcErrorCode::TransportWrite,
    message: "Cadder lost the daemon connection while sending the request; the operation outcome is unknown."
      .into(),
    guidance: Some("Check the current daemon state before retrying the operation.".into()),
    retryable: operation_retryable(&request.operation),
    request_id: Some(request.request_id.clone()),
    operation: Some(request.operation.clone()),
    source: Some(Box::new(error)),
  })
}

pub(in crate::ipc) fn response_read_error(
  request: &ClientRequestContext,
  error: io::Error,
) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::ResponseRead,
    code: LocalIpcErrorCode::TransportRead,
    message: "Cadder lost the daemon connection before receiving a complete response; the operation outcome is unknown."
      .into(),
    guidance: Some("Check the current daemon state before retrying the operation.".into()),
    retryable: operation_retryable(&request.operation),
    request_id: Some(request.request_id.clone()),
    operation: Some(request.operation.clone()),
    source: Some(Box::new(error)),
  })
}

pub(in crate::ipc) fn response_frame_error(
  request: &ClientRequestContext,
  error: IpcCodecError,
) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::ResponseRead,
    code: LocalIpcErrorCode::Frame,
    message: "Cadder rejected an invalid or oversized daemon response; the operation outcome is unknown."
      .into(),
    guidance: Some(
      "Inspect daemon diagnostics and verify that the client and daemon use compatible framing limits."
        .into(),
    ),
    retryable: false,
    request_id: Some(request.request_id.clone()),
    operation: Some(request.operation.clone()),
    source: Some(Box::new(error)),
  })
}

pub(in crate::ipc) fn response_eof_error(request: &ClientRequestContext) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::ResponseRead,
    code: LocalIpcErrorCode::UnexpectedEof,
    message: "The daemon closed the connection before returning a response; the operation outcome is unknown."
      .into(),
    guidance: Some("Check the current daemon state before retrying the operation.".into()),
    retryable: operation_retryable(&request.operation),
    request_id: Some(request.request_id.clone()),
    operation: Some(request.operation.clone()),
    source: Some(Box::new(io::Error::new(
      io::ErrorKind::UnexpectedEof,
      "daemon closed the IPC connection before returning a response",
    ))),
  })
}

pub(in crate::ipc) fn response_decode_error(
  request: &ClientRequestContext,
  error: serde_json::Error,
) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::ResponseDecode,
    code: LocalIpcErrorCode::Frame,
    message: "Cadder could not decode the daemon response; the operation outcome is unknown."
      .into(),
    guidance: Some(
      "Verify that the Cadder client and daemon versions are compatible, then inspect daemon diagnostics."
        .into(),
    ),
    retryable: false,
    request_id: Some(request.request_id.clone()),
    operation: Some(request.operation.clone()),
    source: Some(Box::new(error)),
  })
}

pub(in crate::ipc) fn response_validation_error(
  request: &ClientRequestContext,
  message: impl Into<Box<str>>,
) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::ResponseValidate,
    code: LocalIpcErrorCode::ProtocolViolation,
    message: message.into(),
    guidance: Some(
      "Verify that the Cadder client and daemon versions are compatible, then inspect daemon diagnostics."
        .into(),
    ),
    retryable: false,
    request_id: Some(request.request_id.clone()),
    operation: Some(request.operation.clone()),
    source: None,
  })
}

pub(in crate::ipc) fn request_write_timeout_error(
  request: &ClientRequestContext,
  retryable: bool,
) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Timeout,
    phase: IpcClientPhase::RequestWrite,
    code: LocalIpcErrorCode::Timeout,
    message: "Cadder could not send the complete request before the local deadline; the operation outcome is unknown."
      .into(),
    guidance: Some("Check the current daemon state before retrying the operation.".into()),
    retryable,
    request_id: Some(request.request_id.clone()),
    operation: Some(request.operation.clone()),
    source: None,
  })
}

pub(in crate::ipc) fn response_timeout_error(
  request: &ClientRequestContext,
  retryable: bool,
) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Timeout,
    phase: IpcClientPhase::ResponseRead,
    code: LocalIpcErrorCode::Timeout,
    message: "The daemon did not return a complete response before the local deadline; the operation outcome is unknown."
      .into(),
    guidance: Some("Check the current daemon state before retrying the operation.".into()),
    retryable,
    request_id: Some(request.request_id.clone()),
    operation: Some(request.operation.clone()),
    source: None,
  })
}

pub(in crate::ipc) fn connection_no_longer_usable(
  request: &ClientRequestContext,
) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::Connect,
    code: LocalIpcErrorCode::ConnectionClosed,
    message: "This Cadder client session is closed; no new request was sent.".into(),
    guidance: Some("Open a new Cadder client session, then retry if the operation is safe.".into()),
    retryable: true,
    request_id: Some(request.request_id.clone()),
    operation: Some(request.operation.clone()),
    source: None,
  })
}

pub(in crate::ipc) fn operation_retryable(operation: &str) -> bool {
  OPERATION_REGISTRY
    .lookup(operation)
    .is_some_and(|definition| definition.timeout_retryable())
}

pub(in crate::ipc) fn daemon_launch_error(
  code: LocalIpcErrorCode,
  message: &'static str,
  guidance: &'static str,
  source: Option<crate::ipc_client_error::BoxError>,
) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::DaemonLaunch,
    code,
    message: message.into(),
    guidance: Some(guidance.into()),
    retryable: false,
    request_id: None,
    operation: Some("start-daemon".into()),
    source,
  })
}

pub(in crate::ipc) fn daemon_readiness_timeout(message: &'static str) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Timeout,
    phase: IpcClientPhase::DaemonReadiness,
    code: LocalIpcErrorCode::Timeout,
    message: message.into(),
    guidance: Some(
      "Run cadderd in foreground diagnostic mode, correct any startup error, then retry.".into(),
    ),
    retryable: true,
    request_id: None,
    operation: Some("start-daemon".into()),
    source: None,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn context(operation: &str) -> ClientRequestContext {
    ClientRequestContext {
      operation: operation.into(),
      request_id: RequestId::parse("request-1").unwrap(),
    }
  }

  #[tokio::test]
  async fn prepared_request_encodes_and_writes_one_bounded_frame() {
    let request =
      PreparedClientRequest::new("request-1", &cadder_ipc::QueryStatePayload::default()).unwrap();
    assert_eq!(
      request.context.operation.as_ref(),
      cadder_ipc::message_types::QUERY_STATE_REQUEST
    );
    assert!(request.frame.ends_with(b"\n"));

    let mut writer = tokio::io::sink();
    write_prepared_request(&mut writer, &request).await.unwrap();
    write_prepared_request_until(
      &mut writer,
      &request,
      tokio::time::Instant::now() + Duration::from_secs(1),
    )
    .await
    .unwrap();

    let error =
      PreparedClientRequest::new("", &cadder_ipc::QueryStatePayload::default()).unwrap_err();
    assert_eq!(error.code(), "invalid_request");
  }

  #[test]
  fn local_protocol_errors_keep_phase_code_context_and_retryability() {
    let ordinary = context(cadder_ipc::message_types::QUERY_STATE_REQUEST);
    let mutation = context(cadder_ipc::message_types::SET_DOMAIN_ENABLED_REQUEST);
    let json_error = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    let errors = vec![
      endpoint_resolution_error(io::Error::new(io::ErrorKind::InvalidInput, "endpoint")),
      connection_error(io::Error::new(io::ErrorKind::PermissionDenied, "connect")),
      connection_error(io::Error::new(io::ErrorKind::TimedOut, "connect")),
      connection_error(io::Error::new(io::ErrorKind::NotFound, "connect")),
      connection_error(io::Error::other("connect")),
      connection_timeout_error(),
      peer_authentication_preface_error(io::Error::new(io::ErrorKind::PermissionDenied, "preface")),
      peer_authentication_preface_error(io::Error::new(io::ErrorKind::TimedOut, "preface")),
      peer_authentication_preface_error(io::Error::new(io::ErrorKind::NotFound, "preface")),
      peer_authentication_preface_error(io::Error::other("preface")),
      request_frame_error(&ordinary, IpcCodecError::FrameTooLarge),
      request_write_error(
        &ordinary,
        io::Error::new(io::ErrorKind::BrokenPipe, "write"),
      ),
      response_read_error(&ordinary, io::Error::new(io::ErrorKind::BrokenPipe, "read")),
      response_frame_error(&ordinary, IpcCodecError::UnterminatedFrame),
      response_eof_error(&ordinary),
      response_decode_error(&ordinary, json_error),
      response_validation_error(&ordinary, "invalid response"),
      request_write_timeout_error(&ordinary, true),
      response_timeout_error(&ordinary, true),
      connection_no_longer_usable(&ordinary),
      daemon_launch_error(
        LocalIpcErrorCode::DaemonStartFailed,
        "start failed",
        "retry manually",
        Some(Box::new(io::Error::other("launch"))),
      ),
      daemon_readiness_timeout("readiness failed"),
    ];

    for error in errors {
      assert!(!error.code().is_empty());
      assert!(!error.message().is_empty());
      assert!(error.local_error().is_some());
    }
    assert!(operation_retryable(&ordinary.operation));
    assert!(!operation_retryable(&mutation.operation));
    assert!(!operation_retryable("unknown-operation"));
    for kind in [
      io::ErrorKind::NotFound,
      io::ErrorKind::ConnectionRefused,
      io::ErrorKind::ConnectionReset,
      io::ErrorKind::ConnectionAborted,
      io::ErrorKind::NotConnected,
      io::ErrorKind::AddrNotAvailable,
    ] {
      assert!(daemon_not_ready_error(kind));
    }
    assert!(!daemon_not_ready_error(io::ErrorKind::PermissionDenied));
  }
}
