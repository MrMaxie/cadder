use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct IpcClientDeadlines {
  pub(super) connect: Duration,
  pub(super) ordinary: Duration,
  pub(super) reload: Duration,
  pub(super) shutdown: Duration,
}

impl Default for IpcClientDeadlines {
  fn default() -> Self {
    Self {
      connect: Duration::from_secs(5),
      ordinary: Duration::from_secs(30),
      reload: Duration::from_secs(120),
      shutdown: Duration::from_secs(30),
    }
  }
}

impl IpcClientDeadlines {
  fn response_for(self, operation: &str) -> Duration {
    match OPERATION_REGISTRY
      .lookup(operation)
      .map(|definition| definition.deadline())
    {
      Some(OperationDeadlineClass::Reload) => self.reload,
      Some(OperationDeadlineClass::Shutdown) => self.shutdown,
      Some(OperationDeadlineClass::Ordinary) | None => self.ordinary,
    }
  }
}

#[derive(Debug, Clone)]
pub struct CadderClient {
  paths: RuntimePaths,
  deadlines: IpcClientDeadlines,
}

impl CadderClient {
  pub fn new(paths: RuntimePaths) -> Self {
    Self {
      paths,
      deadlines: IpcClientDeadlines::default(),
    }
  }

  pub async fn request<TRequest>(
    &self,
    request_id: impl AsRef<str>,
    request: &TRequest,
  ) -> IpcClientResult<TRequest::Response>
  where
    TRequest: CorrelatedRequest,
  {
    let prepared = PreparedClientRequest::new(request_id.as_ref(), request)?;
    let request_id = prepared.context.request_id.clone();
    let operation = prepared.context.operation.clone();
    let mut session = CadderSession::connect_with_deadlines(&self.paths, self.deadlines)
      .await
      .map_err(|error| error.with_request_context(request_id, operation))?;
    session
      .request_prepared::<TRequest::Response>(prepared)
      .await
  }
}

#[derive(Debug)]
pub struct CadderSession {
  reader: Option<IpcFrameReader>,
  writer: Option<tokio::io::WriteHalf<Stream>>,
  negotiation: NegotiatedSession,
  deadlines: IpcClientDeadlines,
  usable: bool,
}

impl CadderSession {
  pub async fn connect(paths: &RuntimePaths) -> IpcClientResult<Self> {
    Self::connect_with_deadlines(paths, IpcClientDeadlines::default()).await
  }

  pub(super) async fn connect_with_deadlines(
    paths: &RuntimePaths,
    deadlines: IpcClientDeadlines,
  ) -> IpcClientResult<Self> {
    let deadline = tokio::time::Instant::now() + deadlines.connect;
    Self::connect_endpoint(paths, deadlines, deadline).await
  }

  async fn connect_endpoint(
    paths: &RuntimePaths,
    deadlines: IpcClientDeadlines,
    deadline: tokio::time::Instant,
  ) -> IpcClientResult<Self> {
    let name = local_socket_name(paths).map_err(endpoint_resolution_error)?;
    let result = tokio::time::timeout_at(deadline, async {
      let mut conn = Stream::connect(name).await.map_err(connection_error)?;
      send_peer_authentication_preface(&mut conn)
        .await
        .map_err(peer_authentication_preface_error)?;
      let (read_half, mut writer) = tokio::io::split(conn);
      let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
      let negotiation = perform_client_handshake(&mut reader, &mut writer, paths).await?;
      Ok(Self {
        reader: Some(reader),
        writer: Some(writer),
        negotiation,
        deadlines,
        usable: true,
      })
    })
    .await;
    match result {
      Ok(session) => session,
      Err(_) => Err(connection_timeout_error()),
    }
  }

  pub fn negotiated_version(&self) -> ProtocolVersion {
    self.negotiation.version
  }

  pub async fn request<TRequest>(
    &mut self,
    request_id: impl AsRef<str>,
    request: &TRequest,
  ) -> IpcClientResult<TRequest::Response>
  where
    TRequest: CorrelatedRequest,
  {
    let prepared = PreparedClientRequest::new(request_id.as_ref(), request)?;
    self.request_prepared::<TRequest::Response>(prepared).await
  }

  async fn request_prepared<TResponse>(
    &mut self,
    prepared: PreparedClientRequest,
  ) -> IpcClientResult<TResponse>
  where
    TResponse: DeserializeOwned,
  {
    if !self.usable {
      return Err(connection_no_longer_usable(&prepared.context));
    }

    let deadline = self.deadlines.response_for(&prepared.context.operation);
    let mut exchange = SessionExchangeGuard::new(self);
    let result = exchange.session.exchange(&prepared, deadline).await;
    if result.is_ok()
      || result
        .as_ref()
        .is_err_and(|error| error.daemon_error().is_some())
    {
      exchange.restore();
    }
    result
  }

  fn retire(&mut self) {
    self.usable = false;
    self.reader.take();
    self.writer.take();
  }

  async fn exchange<TResponse>(
    &mut self,
    prepared: &PreparedClientRequest,
    deadline: Duration,
  ) -> IpcClientResult<TResponse>
  where
    TResponse: DeserializeOwned,
  {
    let deadline = tokio::time::Instant::now() + deadline;
    let writer = self
      .writer
      .as_mut()
      .expect("usable Cadder session retains its writer");
    write_prepared_request_until(writer, prepared, deadline).await?;
    let reader = self
      .reader
      .as_mut()
      .expect("usable Cadder session retains its reader");
    match tokio::time::timeout_at(
      deadline,
      read_client_response::<_, TResponse>(reader, &prepared.context),
    )
    .await
    {
      Ok(result) => result,
      Err(_) => Err(response_timeout_error(
        &prepared.context,
        operation_retryable(&prepared.context.operation),
      )),
    }
  }
}

struct SessionExchangeGuard<'a> {
  session: &'a mut CadderSession,
  restored: bool,
}

impl<'a> SessionExchangeGuard<'a> {
  fn new(session: &'a mut CadderSession) -> Self {
    session.usable = false;
    Self {
      session,
      restored: false,
    }
  }

  fn restore(&mut self) {
    self.session.usable = true;
    self.restored = true;
  }
}

impl Drop for SessionExchangeGuard<'_> {
  fn drop(&mut self) {
    if !self.restored {
      self.session.retire();
    }
  }
}

mod protocol;

pub(super) use protocol::*;
