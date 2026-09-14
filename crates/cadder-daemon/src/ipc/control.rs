use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnectionAction {
  Continue,
  Close,
}

pub(super) async fn read_request_or_drain(
  reader: &mut IpcFrameReader,
  control: &ConnectionControl,
) -> Option<std::result::Result<String, IpcCodecError>> {
  tokio::select! {
    biased;
    frame = reader.next() => frame,
    _ = control.request_drain.cancelled() => {
      let deadline = control
        .request_drain_deadline
        .get()
        .copied()
        .expect("request drain deadline is set before cancellation");
      timeout_at(deadline, reader.next()).await.unwrap_or(None)
    }
  }
}

pub(super) enum ConcurrentRead {
  Pipelined(Option<RequestId>),
  Closed,
}

pub(super) fn classify_concurrent_read(
  frame: Option<std::result::Result<String, IpcCodecError>>,
) -> ConcurrentRead {
  match frame {
    Some(Ok(line)) => ConcurrentRead::Pipelined(pipelined_request_id(&line)),
    Some(Err(_)) => ConcurrentRead::Pipelined(None),
    None => ConcurrentRead::Closed,
  }
}

fn pipelined_request_id(line: &str) -> Option<RequestId> {
  serde_json::from_str::<RawRequestEnvelope>(line)
    .ok()
    .map(|envelope| envelope.request_id().clone())
}

pub(super) async fn send_operation_timeout<W>(
  writer: &mut W,
  request_id: Option<RequestId>,
  definition: &cadder_ipc::OperationDefinition,
  limits: IpcLimits,
) -> Result<()>
where
  W: AsyncWrite + Unpin,
{
  let error = ProtocolError::new(
    ProtocolErrorKind::Timeout,
    ProtocolErrorCode::parse("timeout").expect("built-in error code is valid"),
    format!(
      "Cadder did not finish `{}` before its local operation deadline; the outcome is unknown.",
      definition.name()
    ),
    Some("Check the current daemon state before retrying the operation.".into()),
    definition.timeout_retryable(),
  );
  send_late_protocol_error(writer, request_id, error, limits).await
}

pub(super) async fn send_shutting_down<W>(
  writer: &mut W,
  request_id: Option<RequestId>,
  limits: IpcLimits,
) -> Result<()>
where
  W: AsyncWrite + Unpin,
{
  send_shutdown_error(
    writer,
    request_id,
    "The daemon is shutting down and does not accept new requests.",
    limits,
  )
  .await
}

pub(super) async fn send_request_shutting_down<W>(
  writer: &mut W,
  request_id: Option<RequestId>,
  limits: IpcLimits,
) -> Result<()>
where
  W: AsyncWrite + Unpin,
{
  send_shutdown_error(
    writer,
    request_id,
    "This request did not complete because the daemon is shutting down.",
    limits,
  )
  .await
}

async fn send_shutdown_error<W>(
  writer: &mut W,
  request_id: Option<RequestId>,
  message: &'static str,
  limits: IpcLimits,
) -> Result<()>
where
  W: AsyncWrite + Unpin,
{
  let error = ProtocolError::new(
    ProtocolErrorKind::ShuttingDown,
    ProtocolErrorCode::parse("shutting_down").expect("built-in error code is valid"),
    message,
    Some("Wait for the daemon to stop, then start it before retrying the operation.".into()),
    false,
  );
  send_late_protocol_error(writer, request_id, error, limits).await
}

pub(super) async fn send_pipelined_error<W>(
  writer: &mut W,
  request_id: Option<RequestId>,
  limits: IpcLimits,
) -> Result<()>
where
  W: AsyncWrite + Unpin,
{
  let error = ProtocolError::new(
    ProtocolErrorKind::ProtocolViolation,
    ProtocolErrorCode::parse("pipelined_request").expect("built-in error code is valid"),
    "Cadder accepts one active request per local IPC connection.",
    Some("Wait for the current response before sending the next request.".into()),
    false,
  );
  send_late_protocol_error(writer, request_id, error, limits).await
}

async fn send_late_protocol_error<W>(
  writer: &mut W,
  request_id: Option<RequestId>,
  error: ProtocolError,
  limits: IpcLimits,
) -> Result<()>
where
  W: AsyncWrite + Unpin,
{
  let response = ProtocolErrorResponse::rejected(request_id, error);
  write_envelope_until(
    writer,
    message_types::PROTOCOL_ERROR_RESPONSE,
    &response,
    Instant::now() + limits.write_no_progress,
    limits.write_no_progress,
  )
  .await
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::io::{AsyncReadExt, duplex};

  async fn finish_frame(
    mut reader: tokio::io::DuplexStream,
    writer: tokio::io::DuplexStream,
  ) -> String {
    drop(writer);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.unwrap();
    String::from_utf8(bytes).unwrap()
  }

  #[test]
  fn concurrent_read_classification_preserves_valid_request_correlation() {
    let request = cadder_ipc::RequestEnvelope::new(
      CURRENT_PROTOCOL_VERSION,
      RequestId::parse("request-1").unwrap(),
      QueryStatePayload::default(),
    );
    let line = serde_json::to_string(&request).unwrap();
    assert!(matches!(
      classify_concurrent_read(Some(Ok(line))),
      ConcurrentRead::Pipelined(Some(request_id)) if request_id.as_ref() == "request-1"
    ));
    assert!(matches!(
      classify_concurrent_read(Some(Ok("not-json".to_string()))),
      ConcurrentRead::Pipelined(None)
    ));
    assert!(matches!(
      classify_concurrent_read(Some(Err(IpcCodecError::Io(io::Error::other("broken"))))),
      ConcurrentRead::Pipelined(None)
    ));
    assert!(matches!(
      classify_concurrent_read(None),
      ConcurrentRead::Closed
    ));
  }

  #[tokio::test]
  async fn protocol_control_responses_are_bounded_correlated_and_actionable() {
    let limits = IpcLimits::default();
    let request_id = RequestId::parse("request-1").unwrap();
    let definition = OPERATION_REGISTRY
      .lookup(message_types::QUERY_STATE_REQUEST)
      .unwrap();

    let (reader, mut writer) = duplex(8 * 1024);
    send_operation_timeout(&mut writer, Some(request_id.clone()), definition, limits)
      .await
      .unwrap();
    let timeout = finish_frame(reader, writer).await;
    assert!(timeout.ends_with('\n'));
    assert!(timeout.contains("request-1"));
    assert!(timeout.contains("timeout"));
    assert!(timeout.contains("outcome is unknown"));

    let (reader, mut writer) = duplex(8 * 1024);
    send_shutting_down(&mut writer, Some(request_id.clone()), limits)
      .await
      .unwrap();
    let shutting_down = finish_frame(reader, writer).await;
    assert!(shutting_down.contains("does not accept new requests"));

    let (reader, mut writer) = duplex(8 * 1024);
    send_request_shutting_down(&mut writer, Some(request_id.clone()), limits)
      .await
      .unwrap();
    let request_shutdown = finish_frame(reader, writer).await;
    assert!(request_shutdown.contains("did not complete"));

    let (reader, mut writer) = duplex(8 * 1024);
    send_pipelined_error(&mut writer, None, limits)
      .await
      .unwrap();
    let pipelined = finish_frame(reader, writer).await;
    assert!(pipelined.contains("uncorrelated-error"));
    assert!(pipelined.contains("pipelined_request"));
    assert!(pipelined.contains("one active request"));
  }
}
