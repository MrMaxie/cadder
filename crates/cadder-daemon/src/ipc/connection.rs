use super::*;

#[derive(Debug, thiserror::Error)]
pub(super) enum PeerAuthenticationError {
  #[error("the local IPC authentication preface was not accepted")]
  Preface(#[source] io::Error),
  #[error("the local IPC peer identity could not be authenticated")]
  Identity(#[source] io::Error),
  #[error("the local IPC peer identity does not match the runtime owner")]
  PrincipalMismatch,
}

impl PeerAuthenticationError {
  fn reason_code(&self) -> &'static str {
    match self {
      Self::Preface(_) => "authentication-preface-rejected",
      Self::Identity(_) => "peer-identity-unavailable",
      Self::PrincipalMismatch => "principal-outside-runtime-owner",
    }
  }
}

pub(super) async fn authenticate_accepted_connection(
  mut conn: Stream,
  owner_principal: IpcPrincipal,
  policy: IpcSecurityPolicy,
  peer_identity_resolver: IpcPeerIdentityResolver,
) -> std::result::Result<(Stream, ConnectionSecurityContext), PeerAuthenticationError> {
  receive_peer_authentication_preface(&mut conn)
    .await
    .map_err(PeerAuthenticationError::Preface)?;
  let peer_principal = peer_identity_resolver
    .resolve(&conn)
    .map_err(PeerAuthenticationError::Identity)?;
  if !policy
    .authenticate_peer(&owner_principal, &peer_principal)
    .is_allowed()
  {
    return Err(PeerAuthenticationError::PrincipalMismatch);
  }

  let security = ConnectionSecurityContext {
    owner_principal,
    policy,
    peer_principal,
  };
  Ok((conn, security))
}

pub(super) async fn serve_accepted_connection(conn: Stream, context: AcceptedConnectionContext) {
  let AcceptedConnectionContext {
    state,
    owner_principal,
    policy,
    peer_identity_resolver,
    handshake_identity,
    mutation_tasks,
    control,
  } = context;
  let authenticated = tokio::select! {
    _ = control.connection_cancellation.cancelled() => return,
    authenticated = timeout_at(
      control.accepted_at + control.limits.first_frame_byte,
      authenticate_accepted_connection(
        conn,
        owner_principal,
        policy,
        peer_identity_resolver,
      ),
    ) => authenticated,
  };
  match authenticated {
    Ok(Ok((conn, security))) => {
      let _ = handle_connection(
        conn,
        state,
        security,
        handshake_identity,
        &mutation_tasks,
        control,
      )
      .await;
    }
    Ok(Err(error)) => log_peer_authentication_denial(&state, &error).await,
    Err(_) => {}
  }
}

pub(super) async fn log_peer_authentication_denial(
  state: &DaemonState,
  error: &PeerAuthenticationError,
) {
  state.logs().append(
    LogStreamIdentity::runtime_control(),
    LogSeverity::Warn,
    format!(
      "Cadder rejected a local connection before any request ran; runtime state is unchanged. Use the runtime owner account or inspect security diagnostics. Reason: {}",
      error.reason_code()
    ),
    LogAttributionKind::RuntimeControl,
    Some("ipc-peer-denied".to_string()),
  ).await;
}

#[derive(Debug, Clone)]
pub(super) struct ConnectionSecurityContext {
  pub(super) owner_principal: IpcPrincipal,
  pub(super) policy: IpcSecurityPolicy,
  pub(super) peer_principal: IpcPrincipal,
}

#[derive(Debug, Clone)]
pub(super) struct ConnectionControl {
  pub(super) accepted_at: Instant,
  pub(super) limits: IpcLimits,
  pub(super) request_drain: CancellationToken,
  pub(super) request_drain_deadline: Arc<OnceLock<Instant>>,
  pub(super) mutation_cancellation: CancellationToken,
  pub(super) connection_cancellation: CancellationToken,
}

pub(super) async fn join_connection_tasks(
  tasks: &mut JoinSet<()>,
  allow_cancelled: bool,
) -> Result<()> {
  let mut first_failure = None;
  while let Some(result) = tasks.join_next().await {
    if let Err(error) = result
      && !(allow_cancelled && error.is_cancelled())
      && first_failure.is_none()
    {
      first_failure = Some(error);
    }
  }
  if let Some(error) = first_failure {
    anyhow::bail!("local IPC connection task terminated unexpectedly: {error}");
  }
  Ok(())
}

pub(super) async fn drain_handler_tasks(
  connection_tasks: &mut JoinSet<()>,
  mutation_tasks: &MutationTaskRegistry,
  allow_cancelled: bool,
) -> Result<()> {
  let (connections, mutations) = tokio::join!(
    join_connection_tasks(connection_tasks, allow_cancelled),
    mutation_tasks.wait()
  );
  connections?;
  mutations
}

#[derive(Debug, Default)]
struct MutationTaskRegistryState {
  tracker: TaskTracker,
  closed: bool,
  first_panic: Option<Box<str>>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct MutationTaskRegistry {
  state: Arc<std::sync::Mutex<MutationTaskRegistryState>>,
  panic_signal: CancellationToken,
}

impl MutationTaskRegistry {
  pub(super) fn spawn<F>(&self, task: F) -> Option<JoinHandle<F::Output>>
  where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
  {
    let state = self
      .state
      .lock()
      .expect("mutation task registry lock poisoned");
    if state.closed {
      None
    } else {
      let registry = self.state.clone();
      let panic_signal = self.panic_signal.clone();
      Some(state.tracker.spawn(async move {
        match AssertUnwindSafe(task).catch_unwind().await {
          Ok(output) => output,
          Err(payload) => {
            let message = panic_payload_message(payload.as_ref());
            let mut state = registry
              .lock()
              .expect("mutation task registry lock poisoned");
            if state.first_panic.is_none() {
              state.first_panic = Some(message.into());
            }
            drop(state);
            panic_signal.cancel();
            resume_unwind(payload);
          }
        }
      }))
    }
  }

  pub(super) fn close(&self) {
    let mut state = self
      .state
      .lock()
      .expect("mutation task registry lock poisoned");
    state.closed = true;
    state.tracker.close();
  }

  pub(super) async fn wait(&self) -> Result<()> {
    let tracker = self
      .state
      .lock()
      .expect("mutation task registry lock poisoned")
      .tracker
      .clone();
    tracker.wait().await;
    let first_panic = self
      .state
      .lock()
      .expect("mutation task registry lock poisoned")
      .first_panic
      .clone();
    if let Some(message) = first_panic {
      anyhow::bail!("owned mutation task panicked: {message}");
    }
    Ok(())
  }

  pub(super) async fn panic_detected(&self) {
    self.panic_signal.cancelled().await;
  }
}

pub(super) fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
  payload
    .downcast_ref::<&'static str>()
    .copied()
    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
    .unwrap_or("non-string panic payload")
}

pub(super) async fn handle_connection(
  conn: Stream,
  state: DaemonState,
  security: ConnectionSecurityContext,
  handshake_identity: ServerHandshakeIdentity,
  mutation_tasks: &MutationTaskRegistry,
  control: ConnectionControl,
) -> Result<()> {
  let owned = ConnectionOwnership::default();
  let cleanup_signal = CancellationToken::new();
  let worker_signal = cleanup_signal.clone();
  let worker_ownership = owned.clone();
  let cleanup_state = state.clone();
  let Some(cleanup_task) = mutation_tasks.spawn(async move {
    worker_signal.cancelled().await;
    for (id, nonce) in worker_ownership.disconnect_and_take_entries().await {
      cleanup_state
        .unregister_for_ipc_disconnect(&id, &nonce)
        .await;
    }
  }) else {
    return Ok(());
  };
  let cleanup_guard = ConnectionCleanupGuard(cleanup_signal);
  let result = tokio::select! {
    _ = control.connection_cancellation.cancelled() => Ok(()),
    result = handle_connection_loop(
      conn,
      state.clone(),
      &owned,
      mutation_tasks,
      &security,
      &handshake_identity,
      &control,
    ) => result,
  };
  drop(cleanup_guard);
  cleanup_task
    .await
    .context("join registration disconnect cleanup")?;
  result
}

pub(super) struct ConnectionCleanupGuard(CancellationToken);

impl Drop for ConnectionCleanupGuard {
  fn drop(&mut self) {
    self.0.cancel();
  }
}

pub(super) async fn handle_connection_loop(
  conn: Stream,
  state: DaemonState,
  owned: &ConnectionOwnership,
  mutation_tasks: &MutationTaskRegistry,
  security: &ConnectionSecurityContext,
  handshake_identity: &ServerHandshakeIdentity,
  control: &ConnectionControl,
) -> Result<()> {
  let (read_half, mut write_half) = tokio::io::split(conn);
  let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
  let Some(_negotiation) = accept_client_handshake(
    &mut reader,
    &mut write_half,
    handshake_identity,
    control.accepted_at,
    control.limits,
  )
  .await?
  else {
    return Ok(());
  };
  macro_rules! send_response {
    ($message_type:expr, $response:expr) => {
      write_envelope(&mut write_half, $message_type, &$response).await?;
    };
  }
  loop {
    let next_frame = read_request_or_drain(&mut reader, control).await;
    let line = match next_frame {
      Some(Ok(line)) => line,
      Some(Err(error)) => {
        let response = ProtocolErrorResponse::rejected(None, invalid_request_frame_error());
        let _ = write_envelope(
          &mut write_half,
          message_types::PROTOCOL_ERROR_RESPONSE,
          &response,
        )
        .await;
        return Err(error).context("read bounded IPC request frame");
      }
      None => break,
    };

    let envelope: RawRequestEnvelope = match serde_json::from_str(&line) {
      Ok(envelope) => envelope,
      Err(error) => return Err(error).context("decode versioned IPC request envelope"),
    };
    if control.request_drain.is_cancelled() {
      send_shutting_down(
        &mut write_half,
        Some(envelope.request_id().clone()),
        control.limits,
      )
      .await?;
      break;
    }
    if !authorize_or_reject(&mut write_half, &state, &envelope, security).await? {
      continue;
    }
    let authorized = match OPERATION_REGISTRY.authorize_envelope(&envelope) {
      Ok(authorized) => authorized,
      Err(error) => {
        let response = ProtocolErrorResponse::rejected(Some(envelope.request_id().clone()), error);
        send_response!(message_types::PROTOCOL_ERROR_RESPONSE, response);
        continue;
      }
    };
    let action = supervise_unary_request(
      &mut reader,
      &mut write_half,
      &authorized,
      UnarySupervisionContext {
        state: &state,
        owned,
        mutation_tasks,
        mutation_cancellation: &control.mutation_cancellation,
        request_drain: &control.request_drain,
        limits: control.limits,
      },
    )
    .await?;
    if action == ConnectionAction::Close {
      break;
    }
  }

  Ok(())
}
