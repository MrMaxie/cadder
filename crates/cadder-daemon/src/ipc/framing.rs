use super::*;

pub(super) async fn write_envelope<W, T>(
  writer: &mut W,
  message_type: &str,
  payload: &T,
) -> Result<()>
where
  W: AsyncWrite + Unpin,
  T: Serialize,
{
  let encoded = encode_envelope(message_type, payload)?;
  let limits = IpcLimits::default();
  write_frame_until(
    writer,
    &encoded,
    Instant::now() + limits.frame_completion,
    limits.write_no_progress,
  )
  .await?;
  Ok(())
}

pub(super) async fn write_envelope_until<W, T>(
  writer: &mut W,
  message_type: &str,
  payload: &T,
  terminal_deadline: Instant,
  no_progress: Duration,
) -> Result<()>
where
  W: AsyncWrite + Unpin,
  T: Serialize,
{
  let encoded = encode_envelope(message_type, payload)?;
  write_frame_until(writer, &encoded, terminal_deadline, no_progress).await?;
  Ok(())
}

pub(super) fn encode_envelope<T>(
  message_type: &str,
  payload: &T,
) -> std::result::Result<Vec<u8>, IpcCodecError>
where
  T: Serialize,
{
  let mut payload = serde_json::to_value(payload).map_err(IpcCodecError::Serialization)?;
  let request_id = payload
    .get("requestId")
    .and_then(serde_json::Value::as_str)
    .ok_or_else(|| {
      IpcCodecError::Serialization(serde_json::Error::io(io::Error::new(
        io::ErrorKind::InvalidData,
        "a response requires a correlated request ID",
      )))
    })
    .and_then(|value| {
      RequestId::parse(value).map_err(|error| {
        IpcCodecError::Serialization(serde_json::Error::io(io::Error::new(
          io::ErrorKind::InvalidData,
          error,
        )))
      })
    })?;
  if let Some(object) = payload.as_object_mut() {
    object.remove("requestId");
  }
  if message_type == message_types::PROTOCOL_ERROR_RESPONSE {
    let error: ProtocolError =
      serde_json::from_value(payload.get("error").cloned().ok_or_else(|| {
        IpcCodecError::Serialization(serde_json::Error::io(io::Error::new(
          io::ErrorKind::InvalidData,
          "a failed response requires an error",
        )))
      })?)
      .map_err(IpcCodecError::Serialization)?;
    return encode_json_frame(&cadder_ipc::ResponseEnvelope::<serde_json::Value>::failure(
      CURRENT_PROTOCOL_VERSION,
      "protocol-error",
      request_id,
      error,
    ));
  }
  let operation = request_operation_for_response(message_type).ok_or_else(|| {
    IpcCodecError::Serialization(serde_json::Error::io(io::Error::new(
      io::ErrorKind::InvalidData,
      format!("unknown response type `{message_type}`"),
    )))
  })?;
  encode_json_frame(&cadder_ipc::ResponseEnvelope::success(
    CURRENT_PROTOCOL_VERSION,
    operation,
    request_id,
    payload,
  ))
}

fn request_operation_for_response(response: &str) -> Option<&'static str> {
  Some(match response {
    message_types::REGISTER_ENTRYPOINT_RESPONSE => message_types::REGISTER_ENTRYPOINT_REQUEST,
    message_types::UNREGISTER_ENTRYPOINT_RESPONSE => message_types::UNREGISTER_ENTRYPOINT_REQUEST,
    message_types::HEARTBEAT_ENTRYPOINT_RESPONSE => message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
    message_types::QUERY_STATE_RESPONSE => message_types::QUERY_STATE_REQUEST,
    message_types::SET_ENTRYPOINT_ENABLED_RESPONSE => message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
    message_types::SET_DOMAIN_ENABLED_RESPONSE => message_types::SET_DOMAIN_ENABLED_REQUEST,
    message_types::QUERY_LOGS_RESPONSE => message_types::QUERY_LOGS_REQUEST,
    message_types::SHUTDOWN_DAEMON_RESPONSE => message_types::SHUTDOWN_DAEMON_REQUEST,
    _ => return None,
  })
}

pub(super) async fn write_frame_until<W>(
  writer: &mut W,
  encoded: &[u8],
  terminal_deadline: Instant,
  no_progress: Duration,
) -> io::Result<()>
where
  W: AsyncWrite + Unpin,
{
  write_frame_with_deadlines(writer, encoded, Some(terminal_deadline), no_progress).await
}

pub(super) async fn write_frame_with_deadlines<W>(
  writer: &mut W,
  encoded: &[u8],
  terminal_deadline: Option<Instant>,
  no_progress: Duration,
) -> io::Result<()>
where
  W: AsyncWrite + Unpin,
{
  let mut remaining = encoded;
  while !remaining.is_empty() {
    if let Some(terminal_deadline) = terminal_deadline {
      ensure_write_deadline(terminal_deadline)?;
    }
    let progress_deadline = next_write_deadline(terminal_deadline, no_progress);
    let written = timeout_at(progress_deadline, writer.write(remaining))
      .await
      .map_err(|_| write_deadline_exceeded())??;
    if written == 0 {
      return Err(io::Error::new(
        io::ErrorKind::WriteZero,
        "failed to write the complete IPC frame",
      ));
    }
    remaining = &remaining[written..];
  }

  if let Some(terminal_deadline) = terminal_deadline {
    ensure_write_deadline(terminal_deadline)?;
  }
  let progress_deadline = next_write_deadline(terminal_deadline, no_progress);
  timeout_at(progress_deadline, writer.flush())
    .await
    .map_err(|_| write_deadline_exceeded())??;
  Ok(())
}

pub(super) fn next_write_deadline(
  terminal_deadline: Option<Instant>,
  no_progress: Duration,
) -> Instant {
  let progress_deadline = Instant::now() + no_progress;
  terminal_deadline.map_or(progress_deadline, |deadline| {
    progress_deadline.min(deadline)
  })
}

pub(super) fn ensure_write_deadline(terminal_deadline: Instant) -> io::Result<()> {
  if Instant::now() >= terminal_deadline {
    return Err(write_deadline_exceeded());
  }
  Ok(())
}

pub(super) fn write_deadline_exceeded() -> io::Error {
  io::Error::new(
    io::ErrorKind::TimedOut,
    "IPC frame write did not finish before its progress or operation deadline",
  )
}

pub(super) fn invalid_request_frame_error() -> ProtocolError {
  ProtocolError::new(
    ProtocolErrorKind::Frame,
    ProtocolErrorCode::parse(ProtocolErrorKind::Frame.default_code())
      .expect("built-in protocol error code is valid"),
    "Cadder rejected an invalid or oversized local IPC request frame.",
    Some(
      "Send one UTF-8 JSON object terminated by LF and keep the frame at or below 1 MiB.".into(),
    ),
    false,
  )
}

pub(super) async fn decode_or_reject_until<T, W>(
  writer: &mut W,
  envelope: &AuthorizedRequestEnvelope<'_>,
  deadline: Instant,
  limits: IpcLimits,
) -> Result<Option<T>>
where
  T: DeserializeOwned + cadder_ipc::OperationPayload,
  W: AsyncWrite + Unpin,
{
  match envelope.decode() {
    Ok(request) => Ok(Some(request)),
    Err(error) => {
      let response = ProtocolErrorResponse::rejected(Some(envelope.request_id().clone()), error);
      write_envelope_until(
        writer,
        message_types::PROTOCOL_ERROR_RESPONSE,
        &response,
        deadline,
        limits.write_no_progress,
      )
      .await?;
      Ok(None)
    }
  }
}
