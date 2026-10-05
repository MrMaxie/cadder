use super::*;

pub(super) async fn accept_client_handshake<W>(
  reader: &mut IpcFrameReader,
  writer: &mut W,
  identity: &ServerHandshakeIdentity,
  accepted_at: Instant,
  limits: IpcLimits,
) -> Result<Option<NegotiatedSession>>
where
  W: AsyncWrite + Unpin,
{
  let Some(line) = read_first_frame(reader, accepted_at, limits).await? else {
    return Ok(None);
  };
  let hello: ClientHello = serde_json::from_str(&line).context("decode IPC client handshake")?;

  if hello.runtime_id.as_ref() != identity.runtime_id.as_ref() {
    let frame = ServerHandshakeFrame::rejected(
      hello.request_id,
      identity.runtime_id.clone(),
      identity.daemon_instance_id.clone(),
      ProtocolError::stale_instance(),
    );
    write_handshake_frame(writer, &frame, limits.write_no_progress).await?;
    return Ok(None);
  }

  let version = CURRENT_PROTOCOL_VERSION;
  if hello.protocol_version != version {
    let frame = ServerHandshakeFrame::rejected(
      hello.request_id,
      identity.runtime_id.clone(),
      identity.daemon_instance_id.clone(),
      ProtocolError::incompatible_protocol_version_pair(
        hello.protocol_version,
        CURRENT_PROTOCOL_VERSION,
      ),
    );
    write_handshake_frame(writer, &frame, limits.write_no_progress).await?;
    return Ok(None);
  }
  let frame = ServerHandshakeFrame::accepted(ServerHello {
    request_id: hello.request_id,
    runtime_id: identity.runtime_id.clone(),
    daemon_instance_id: identity.daemon_instance_id.clone(),
    protocol_version: version,
  });
  write_handshake_frame(writer, &frame, limits.write_no_progress).await?;
  Ok(Some(NegotiatedSession { version }))
}

pub(super) async fn read_first_frame(
  reader: &mut IpcFrameReader,
  accepted_at: Instant,
  limits: IpcLimits,
) -> Result<Option<String>> {
  let mut first_byte = [0_u8; 1];
  match timeout_at(
    accepted_at + limits.first_frame_byte,
    reader.get_mut().read_exact(&mut first_byte),
  )
  .await
  {
    Ok(Ok(_)) => reader.read_buffer_mut().extend_from_slice(&first_byte),
    Ok(Err(error)) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
    Ok(Err(error)) => return Err(error).context("read first IPC frame byte"),
    Err(_) => return Ok(None),
  }

  match timeout_at(Instant::now() + limits.frame_completion, reader.next()).await {
    Ok(Some(Ok(line))) => Ok(Some(line)),
    Ok(Some(Err(error))) => Err(error).context("read bounded IPC client handshake"),
    Ok(None) | Err(_) => Ok(None),
  }
}

pub(super) async fn write_handshake_frame<W>(
  writer: &mut W,
  frame: &ServerHandshakeFrame,
  no_progress: Duration,
) -> Result<()>
where
  W: AsyncWrite + Unpin,
{
  let encoded = encode_json_frame(frame)?;
  write_frame_until(writer, &encoded, Instant::now() + no_progress, no_progress).await?;
  Ok(())
}

pub(super) async fn perform_client_handshake<W>(
  reader: &mut IpcFrameReader,
  writer: &mut W,
  paths: &RuntimePaths,
) -> IpcClientResult<NegotiatedSession>
where
  W: AsyncWrite + Unpin,
{
  let request_id =
    RequestId::parse(new_request_id("hello")).expect("generated handshake request ID is valid");
  let hello = ClientHello {
    request_id: request_id.clone(),
    runtime_id: paths.instance_key().into(),
    protocol_version: CURRENT_PROTOCOL_VERSION,
  };
  let encoded = encode_json_frame(&hello).map_err(|error| {
    handshake_local_error(
      IpcClientPhase::RequestEncode,
      LocalIpcErrorCode::Frame,
      "Cadder could not encode its local daemon handshake; no operation was sent.",
      "Report this Cadder handshake serialization error.",
      false,
      request_id.clone(),
      Some(Box::new(error)),
    )
  })?;
  writer.write_all(&encoded).await.map_err(|error| {
    handshake_local_error(
      IpcClientPhase::RequestWrite,
      LocalIpcErrorCode::TransportWrite,
      "Cadder lost the local connection while sending the daemon handshake; no operation was sent.",
      "Retry the connection once.",
      true,
      request_id.clone(),
      Some(Box::new(error)),
    )
  })?;
  writer.flush().await.map_err(|error| {
    handshake_local_error(
      IpcClientPhase::RequestWrite,
      LocalIpcErrorCode::TransportWrite,
      "Cadder could not finish sending the local daemon handshake; no operation was sent.",
      "Retry the connection once.",
      true,
      request_id.clone(),
      Some(Box::new(error)),
    )
  })?;

  let line = match reader.next().await {
    Some(Ok(line)) => line,
    Some(Err(IpcCodecError::Io(error))) => {
      return Err(handshake_local_error(
        IpcClientPhase::ResponseRead,
        LocalIpcErrorCode::TransportRead,
        "Cadder lost the local connection while waiting for the daemon handshake; no operation was sent.",
        "Reconnect to the selected runtime endpoint once.",
        true,
        request_id,
        Some(Box::new(error)),
      ));
    }
    Some(Err(error)) => {
      return Err(handshake_local_error(
        IpcClientPhase::ResponseRead,
        LocalIpcErrorCode::Frame,
        "Cadder rejected an invalid or oversized daemon handshake response; no operation was sent.",
        "Restart the Cadder daemon with a compatible build, then retry.",
        false,
        request_id,
        Some(Box::new(error)),
      ));
    }
    None => {
      return Err(handshake_local_error(
        IpcClientPhase::ResponseRead,
        LocalIpcErrorCode::UnexpectedEof,
        "The daemon closed the connection before confirming its local endpoint; no operation was sent.",
        "Retry the connection once.",
        true,
        request_id,
        None,
      ));
    }
  };
  let frame: ServerHandshakeFrame = serde_json::from_str(&line).map_err(|error| {
    handshake_local_error(
      IpcClientPhase::ResponseDecode,
      LocalIpcErrorCode::Frame,
      "Cadder could not decode the daemon handshake response; no operation was sent.",
      "Restart the Cadder daemon with a compatible build, then retry.",
      false,
      request_id.clone(),
      Some(Box::new(error)),
    )
  })?;

  match frame {
    ServerHandshakeFrame::Accepted(hello) => validate_server_hello(hello, paths, request_id),
    ServerHandshakeFrame::Rejected(rejection) => {
      if rejection.error().request_id.as_ref() != Some(&request_id) {
        return Err(handshake_protocol_violation(
          request_id,
          "The daemon handshake rejection used a different request ID.",
        ));
      }
      if rejection.runtime_id() != paths.instance_key() {
        return Err(stale_handshake_error(request_id));
      }
      Err(IpcClientError::daemon(rejection.error().clone()))
    }
  }
}

pub(super) fn validate_server_hello(
  hello: ServerHello,
  runtime: &impl HandshakeRuntime,
  request_id: RequestId,
) -> IpcClientResult<NegotiatedSession> {
  if hello.request_id != request_id {
    return Err(handshake_protocol_violation(
      request_id,
      "The daemon handshake response used a different request ID.",
    ));
  }
  if hello.runtime_id.as_ref() != runtime.runtime_id() {
    return Err(stale_handshake_error(request_id));
  }
  if hello.protocol_version != CURRENT_PROTOCOL_VERSION {
    return Err(handshake_protocol_violation(
      request_id,
      "The daemon accepted a handshake whose published protocol range is incompatible.",
    ));
  };
  Ok(NegotiatedSession {
    version: hello.protocol_version,
  })
}

pub(super) fn handshake_local_error(
  phase: IpcClientPhase,
  code: LocalIpcErrorCode,
  message: &'static str,
  guidance: &'static str,
  retryable: bool,
  request_id: RequestId,
  source: Option<crate::ipc_client_error::BoxError>,
) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase,
    code,
    message: message.into(),
    guidance: Some(guidance.into()),
    retryable,
    request_id: Some(request_id),
    operation: Some(CLIENT_HELLO_OPERATION.into()),
    source,
  })
}

pub(super) fn stale_handshake_error(request_id: RequestId) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::ResponseValidate,
    code: LocalIpcErrorCode::StaleInstance,
    message:
      "The connected daemon does not own the selected runtime endpoint; no operation was sent."
        .into(),
    guidance: Some("Reconnect to the selected runtime endpoint once.".into()),
    retryable: true,
    request_id: Some(request_id),
    operation: Some(CLIENT_HELLO_OPERATION.into()),
    source: None,
  })
}

pub(super) fn handshake_protocol_violation(
  request_id: RequestId,
  message: &'static str,
) -> IpcClientError {
  handshake_local_error(
    IpcClientPhase::ResponseValidate,
    LocalIpcErrorCode::ProtocolViolation,
    message,
    "Restart the Cadder daemon with a compatible build, then retry.",
    false,
    request_id,
    None,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  async fn connected_pair(paths: &RuntimePaths) -> (RuntimeEndpointLease, Stream, Stream) {
    let lease = RuntimeEndpointLease::claim(paths).await.unwrap().unwrap();
    let name = local_socket_name(paths).unwrap();
    let client = tokio::spawn(async move { Stream::connect(name).await.unwrap() });
    let server = lease.listener.accept().await.unwrap();
    (lease, client.await.unwrap(), server)
  }

  fn runtime_paths(temp: &tempfile::TempDir) -> RuntimePaths {
    RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap()
  }

  fn identity(paths: &RuntimePaths) -> ServerHandshakeIdentity {
    ServerHandshakeIdentity {
      runtime_id: paths.instance_key().into(),
      daemon_instance_id: "daemon-1".into(),
    }
  }

  #[derive(Clone, Copy)]
  enum ClientReply {
    Accepted,
    WrongRequest,
    WrongRuntime,
    WrongVersion,
    Rejected,
    RejectedWrongRequest,
    RejectedWrongRuntime,
    Malformed,
    Close,
  }

  async fn perform_with_reply(reply: ClientReply) -> IpcClientResult<NegotiatedSession> {
    let temp = tempfile::tempdir().unwrap();
    let paths = runtime_paths(&temp);
    let runtime_id = paths.instance_key().to_string();
    let (_lease, client, server) = connected_pair(&paths).await;
    let responder = tokio::spawn(async move {
      let (read_half, mut writer) = tokio::io::split(server);
      let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
      let line = reader.next().await.unwrap().unwrap();
      let hello: ClientHello = serde_json::from_str(&line).unwrap();
      if matches!(reply, ClientReply::Close) {
        return;
      }
      if matches!(reply, ClientReply::Malformed) {
        writer.write_all(b"not-json\n").await.unwrap();
        return;
      }
      let request_id = if matches!(
        reply,
        ClientReply::WrongRequest | ClientReply::RejectedWrongRequest
      ) {
        RequestId::parse("wrong-request").unwrap()
      } else {
        hello.request_id
      };
      let response_runtime = if matches!(
        reply,
        ClientReply::WrongRuntime | ClientReply::RejectedWrongRuntime
      ) {
        "wrong-runtime".to_string()
      } else {
        runtime_id
      };
      let frame = if matches!(
        reply,
        ClientReply::Rejected
          | ClientReply::RejectedWrongRequest
          | ClientReply::RejectedWrongRuntime
      ) {
        ServerHandshakeFrame::rejected(
          request_id,
          response_runtime,
          "daemon-1",
          ProtocolError::stale_instance(),
        )
      } else {
        ServerHandshakeFrame::accepted(ServerHello {
          request_id,
          runtime_id: response_runtime.into(),
          daemon_instance_id: "daemon-1".into(),
          protocol_version: if matches!(reply, ClientReply::WrongVersion) {
            ProtocolVersion::new(2, 0).unwrap()
          } else {
            CURRENT_PROTOCOL_VERSION
          },
        })
      };
      writer
        .write_all(&encode_json_frame(&frame).unwrap())
        .await
        .unwrap();
    });
    let (read_half, mut writer) = tokio::io::split(client);
    let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
    let result = perform_client_handshake(&mut reader, &mut writer, &paths).await;
    responder.await.unwrap();
    result
  }

  async fn accept_hello(
    hello: ClientHello,
    paths: &RuntimePaths,
  ) -> (Option<NegotiatedSession>, String) {
    let (_lease, mut client, server) = connected_pair(paths).await;
    client
      .write_all(&encode_json_frame(&hello).unwrap())
      .await
      .unwrap();
    let (read_half, mut writer) = tokio::io::split(server);
    let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
    let result = accept_client_handshake(
      &mut reader,
      &mut writer,
      &identity(paths),
      Instant::now(),
      IpcLimits::default(),
    )
    .await
    .unwrap();
    drop(reader);
    drop(writer);
    let mut response = vec![0; 8 * 1024];
    let read = client.read(&mut response).await.unwrap_or_default();
    response.truncate(read);
    (result, String::from_utf8(response).unwrap())
  }

  #[tokio::test]
  async fn server_handshake_accepts_exact_identity_and_rejects_stale_or_wrong_versions() {
    let temp = tempfile::tempdir().unwrap();
    let paths = runtime_paths(&temp);
    let request_id = RequestId::parse("hello-1").unwrap();
    let (negotiated, response) = accept_hello(
      ClientHello {
        request_id: request_id.clone(),
        runtime_id: paths.instance_key().into(),
        protocol_version: CURRENT_PROTOCOL_VERSION,
      },
      &paths,
    )
    .await;
    assert_eq!(negotiated.unwrap().version, CURRENT_PROTOCOL_VERSION);
    assert!(response.contains("accepted"));
    assert!(response.contains("daemon-1"));

    let temp = tempfile::tempdir().unwrap();
    let paths = runtime_paths(&temp);
    let (negotiated, response) = accept_hello(
      ClientHello {
        request_id: request_id.clone(),
        runtime_id: "different-runtime".into(),
        protocol_version: CURRENT_PROTOCOL_VERSION,
      },
      &paths,
    )
    .await;
    assert!(negotiated.is_none());
    assert!(response.contains("stale_instance"));

    let temp = tempfile::tempdir().unwrap();
    let paths = runtime_paths(&temp);
    let (negotiated, response) = accept_hello(
      ClientHello {
        request_id,
        runtime_id: paths.instance_key().into(),
        protocol_version: ProtocolVersion::new(2, 0).unwrap(),
      },
      &paths,
    )
    .await;
    assert!(negotiated.is_none());
    assert!(response.contains("incompatible_protocol"));
  }

  #[tokio::test]
  async fn first_frame_reader_handles_clean_eof_and_invalid_handshake_json() {
    let temp = tempfile::tempdir().unwrap();
    let paths = runtime_paths(&temp);
    let (_lease, client, server) = connected_pair(&paths).await;
    drop(client);
    let (read_half, _writer) = tokio::io::split(server);
    let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
    assert!(
      read_first_frame(&mut reader, Instant::now(), IpcLimits::default())
        .await
        .unwrap()
        .is_none()
    );

    let temp = tempfile::tempdir().unwrap();
    let paths = runtime_paths(&temp);
    let (_lease, mut client, server) = connected_pair(&paths).await;
    client.write_all(b"not-json\n").await.unwrap();
    let (read_half, mut writer) = tokio::io::split(server);
    let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
    let error = accept_client_handshake(
      &mut reader,
      &mut writer,
      &identity(&paths),
      Instant::now(),
      IpcLimits::default(),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("decode IPC client handshake"));
  }

  #[test]
  fn client_validation_rejects_each_mismatched_server_field() {
    let temp = tempfile::tempdir().unwrap();
    let paths = runtime_paths(&temp);
    let request_id = RequestId::parse("hello-1").unwrap();
    let hello = |request_id: RequestId, runtime_id: Box<str>, version| ServerHello {
      request_id,
      runtime_id,
      daemon_instance_id: "daemon-1".into(),
      protocol_version: version,
    };

    assert_eq!(
      validate_server_hello(
        hello(
          request_id.clone(),
          paths.instance_key().into(),
          CURRENT_PROTOCOL_VERSION,
        ),
        &paths,
        request_id.clone(),
      )
      .unwrap()
      .version,
      CURRENT_PROTOCOL_VERSION
    );
    assert!(
      validate_server_hello(
        hello(
          RequestId::parse("wrong").unwrap(),
          paths.instance_key().into(),
          CURRENT_PROTOCOL_VERSION,
        ),
        &paths,
        request_id.clone(),
      )
      .unwrap_err()
      .message()
      .contains("different request ID")
    );
    assert!(
      validate_server_hello(
        hello(
          request_id.clone(),
          "different".into(),
          CURRENT_PROTOCOL_VERSION
        ),
        &paths,
        request_id.clone(),
      )
      .unwrap_err()
      .is_stale_instance()
    );
    assert!(
      validate_server_hello(
        hello(
          request_id.clone(),
          paths.instance_key().into(),
          ProtocolVersion::new(2, 0).unwrap(),
        ),
        &paths,
        request_id,
      )
      .unwrap_err()
      .message()
      .contains("incompatible")
    );
  }

  #[tokio::test]
  async fn client_handshake_validates_accepted_rejected_and_malformed_frames() {
    assert_eq!(
      perform_with_reply(ClientReply::Accepted)
        .await
        .unwrap()
        .version,
      CURRENT_PROTOCOL_VERSION
    );
    for reply in [
      ClientReply::WrongRequest,
      ClientReply::WrongRuntime,
      ClientReply::WrongVersion,
      ClientReply::Rejected,
      ClientReply::RejectedWrongRequest,
      ClientReply::RejectedWrongRuntime,
      ClientReply::Malformed,
      ClientReply::Close,
    ] {
      let error = perform_with_reply(reply).await.unwrap_err();
      assert!(error.request_id().is_some());
      assert!(
        error
          .operation()
          .is_none_or(|operation| operation == CLIENT_HELLO_OPERATION)
      );
    }
  }
}
