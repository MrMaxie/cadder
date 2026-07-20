#[cfg(test)]
use crate::IpcEndpointMetadata;
use crate::{
  CaddyBackendMode, DaemonState, IpcClientError, IpcClientPhase, IpcClientResult, IpcOperation,
  IpcPrincipal, IpcSecurityPolicy, LocalIpcErrorCode, LocalIpcErrorKind, RuntimePaths,
  RuntimeProfile,
  ipc_client_error::LocalIpcErrorContext,
  ipc_codec::{BoundedNdjsonCodec, IpcCodecError, encode_json_frame},
  ipc_security::{
    IpcPeerIdentityResolver, receive_peer_authentication_preface, secure_bound_socket,
    secure_listener_options, send_peer_authentication_preface,
  },
  operation_fence::{CommitRejection, OperationFence, RevokeOutcome},
  operation_registry::{AuthorizedLegacyEnvelope, authorize_legacy},
};
use anyhow::{Context, Result};
use cadder_ipc::{
  BasicResponse, CLIENT_HELLO_OPERATION, CapabilityId, ClientHello, HeartbeatEntrypointRequest,
  IpcEnvelope, LegacyCorrelatedRequest, LogAttributionKind, LogSeverity, LogStreamIdentity,
  OPERATION_REGISTRY, OperationAccess, OperationDeadlineClass, OperationShape, PROTOCOL_VERSION,
  ProtocolCapabilities, ProtocolError, ProtocolErrorCode, ProtocolErrorKind, ProtocolErrorResponse,
  ProtocolVersion, ProtocolVersionRange, QueryAutostartRequest, QueryLogsRequest,
  QueryStateRequest, RegisterEntrypointRequest, RequestId, SUPPORTED_PROTOCOL_VERSIONS,
  ServerHandshakeFrame, ServerHello, SetAutostartRequest, SetDomainEnabledRequest,
  SetEntrypointEnabledRequest, ShutdownDaemonRequest, StateChangedEvent, StateStreamGap,
  StateStreamHeartbeat, StateStreamRecord, SubscribeStateRequest, UnregisterEntrypointRequest,
  ensure_compatible_protocol_version, message_types, new_request_id,
};
use futures_util::{FutureExt, StreamExt};
#[cfg(windows)]
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use interprocess::local_socket::{
  ListenerOptions, Name,
  tokio::{Listener, Stream, prelude::*},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
  collections::{BTreeMap, VecDeque},
  env,
  future::Future,
  io,
  panic::{AssertUnwindSafe, resume_unwind},
  path::PathBuf,
  process::Stdio,
  sync::{Arc, OnceLock},
  time::Duration,
};
use tokio::{
  io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
  process::Command,
  sync::{Semaphore, watch},
  task::{JoinHandle, JoinSet},
  time::{Instant, sleep, sleep_until, timeout_at},
};
use tokio_util::{codec::FramedRead, sync::CancellationToken, task::TaskTracker};

type IpcFrameReader = FramedRead<tokio::io::ReadHalf<Stream>, BoundedNdjsonCodec>;

#[derive(Debug, Clone, Copy)]
struct RequestDispatchContext<'a> {
  operation_fence: Option<&'a OperationFence>,
  deadline: Instant,
  limits: IpcLimits,
}

#[derive(Clone, Copy)]
struct UnarySupervisionContext<'a> {
  state: &'a DaemonState,
  owned: &'a ConnectionOwnership,
  mutation_tasks: &'a MutationTaskRegistry,
  mutation_cancellation: &'a CancellationToken,
  request_drain: &'a CancellationToken,
  limits: IpcLimits,
}

#[derive(Clone, Copy)]
struct OwnedMutationWriteContext<'a> {
  fence: &'a OperationFence,
  request_id: Option<&'a RequestId>,
  definition: &'a cadder_ipc::OperationDefinition,
  response_type: &'a str,
  deadline: Instant,
  limits: IpcLimits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NegotiatedSession {
  version: ProtocolVersion,
  capabilities: Box<[CapabilityId]>,
}

#[derive(Debug, Clone)]
struct ServerHandshakeIdentity {
  runtime_id: Box<str>,
  daemon_instance_id: Box<str>,
  supported_versions: ProtocolVersionRange,
  capabilities: Box<[CapabilityId]>,
}

trait HandshakeRuntime {
  fn runtime_id(&self) -> &str;
}

impl HandshakeRuntime for RuntimePaths {
  fn runtime_id(&self) -> &str {
    self.instance_key()
  }
}

#[cfg(test)]
impl HandshakeRuntime for IpcEndpointMetadata {
  fn runtime_id(&self) -> &str {
    &self.runtime_id
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownOrigin {
  IpcRequest,
  ExternalSignal,
  ServerFailure,
}

#[derive(Debug, Clone)]
struct AcceptedConnectionContext {
  state: DaemonState,
  owner_principal: IpcPrincipal,
  policy: IpcSecurityPolicy,
  peer_identity_resolver: IpcPeerIdentityResolver,
  handshake_identity: ServerHandshakeIdentity,
  mutation_tasks: MutationTaskRegistry,
  control: ConnectionControl,
}

impl ServerHandshakeIdentity {
  fn for_runtime(paths: &RuntimePaths) -> Result<Self> {
    Ok(Self {
      runtime_id: paths.instance_key().into(),
      daemon_instance_id: random_daemon_instance_id()?,
      supported_versions: SUPPORTED_PROTOCOL_VERSIONS,
      capabilities: OPERATION_REGISTRY
        .advertised_capabilities(SUPPORTED_PROTOCOL_VERSIONS.maximum())?,
    })
  }
}

#[cfg(test)]
impl From<&IpcEndpointMetadata> for ServerHandshakeIdentity {
  fn from(metadata: &IpcEndpointMetadata) -> Self {
    Self {
      runtime_id: metadata.runtime_id.clone().into_boxed_str(),
      daemon_instance_id: metadata.daemon_instance_id.clone().into_boxed_str(),
      supported_versions: metadata.supported_versions,
      capabilities: metadata.capabilities.clone(),
    }
  }
}

fn random_daemon_instance_id() -> Result<Box<str>> {
  let mut bytes = [0_u8; 16];
  getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
  Ok(hex::encode(bytes).into_boxed_str())
}

#[derive(Debug, Clone, Copy)]
struct IpcLimits {
  max_connections: usize,
  first_frame_byte: Duration,
  frame_completion: Duration,
  write_no_progress: Duration,
  ordinary_operation: Duration,
  reload_operation: Duration,
  stream_setup: Duration,
  stream_heartbeat: Duration,
  stream_max_records: usize,
  stream_max_bytes: usize,
  shutdown_operation: Duration,
  shutdown_accept: Duration,
  shutdown_handler_grace: Duration,
  shutdown_connection_abort_join: Duration,
  shutdown_runtime: Duration,
  shutdown_storage: Duration,
  #[allow(dead_code)]
  shutdown_cleanup: Duration,
  #[cfg(test)]
  dispatch_delay: Duration,
  #[cfg(test)]
  shutdown_response_delay: Duration,
}

impl Default for IpcLimits {
  fn default() -> Self {
    Self {
      max_connections: 64,
      first_frame_byte: Duration::from_secs(5),
      frame_completion: Duration::from_secs(30),
      write_no_progress: Duration::from_secs(5),
      ordinary_operation: Duration::from_secs(30),
      reload_operation: Duration::from_secs(120),
      stream_setup: Duration::from_secs(30),
      stream_heartbeat: Duration::from_secs(15),
      stream_max_records: 256,
      stream_max_bytes: 512 * 1024,
      shutdown_operation: Duration::from_secs(30),
      shutdown_accept: Duration::from_secs(2),
      shutdown_handler_grace: Duration::from_secs(8),
      shutdown_connection_abort_join: Duration::from_secs(1),
      shutdown_runtime: Duration::from_secs(10),
      shutdown_storage: Duration::from_secs(5),
      shutdown_cleanup: Duration::from_secs(5),
      #[cfg(test)]
      dispatch_delay: Duration::ZERO,
      #[cfg(test)]
      shutdown_response_delay: Duration::ZERO,
    }
  }
}

impl IpcLimits {
  fn operation_deadline(self, class: OperationDeadlineClass) -> Instant {
    let duration = match class {
      OperationDeadlineClass::Ordinary => self.ordinary_operation,
      OperationDeadlineClass::Reload => self.reload_operation,
      OperationDeadlineClass::Stream => self.stream_setup,
      OperationDeadlineClass::Shutdown => self.shutdown_operation,
    };
    Instant::now() + duration
  }
}

/// Owns the only local endpoint for one daemon lifetime.
///
/// The listener is claimed before storage or Caddy are initialized and is
/// dropped only after their shutdown completes. This makes the live endpoint,
/// rather than a persistent file, the runtime singleton.
#[derive(Debug)]
pub(crate) struct RuntimeEndpointLease {
  listener: Listener,
  owner_principal: IpcPrincipal,
  handshake_identity: ServerHandshakeIdentity,
  #[cfg(unix)]
  socket_claim_guard: Option<crate::ipc_unix_security::SocketClaimGuard>,
}

impl RuntimeEndpointLease {
  pub(crate) async fn claim(paths: &RuntimePaths) -> Result<Option<Self>> {
    paths
      .ensure_dirs()
      .context("secure the local IPC runtime directory")?;
    let owner_principal = IpcPrincipal::current_process(crate::current_privilege_status())
      .context("authenticate the Cadder runtime-owner identity")?;
    #[cfg(unix)]
    let socket_claim_guard = tokio::task::spawn_blocking({
      let paths = paths.clone();
      move || crate::ipc_unix_security::SocketClaimGuard::acquire(&paths)
    })
    .await
    .context("join local IPC socket-claim coordination")?
    .context("coordinate local IPC socket recovery")?;

    match Self::create(paths, &owner_principal) {
      Ok(listener) => {
        #[cfg(unix)]
        return Self::from_listener(listener, owner_principal, paths, socket_claim_guard).map(Some);
        #[cfg(not(unix))]
        Self::from_listener(listener, owner_principal, paths).map(Some)
      }
      Err(error)
        if matches!(
          error.kind(),
          io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
        ) =>
      {
        if wait_for_endpoint_owner(paths).await {
          return Ok(None);
        }
        #[cfg(unix)]
        {
          crate::ipc_unix_security::remove_stale_socket(paths)?;
          let listener =
            Self::create(paths, &owner_principal).context("reclaim stale local IPC socket")?;
          return Self::from_listener(listener, owner_principal, paths, socket_claim_guard)
            .map(Some);
        }
        #[cfg(not(unix))]
        {
          Ok(None)
        }
      }
      Err(error) => Err(error).context("claim local IPC endpoint"),
    }
  }

  fn create(paths: &RuntimePaths, owner: &IpcPrincipal) -> io::Result<Listener> {
    let name = local_socket_name(paths)?;
    let options = ListenerOptions::new()
      .name(name)
      .reclaim_name(false)
      .try_overwrite(false);
    let listener = secure_listener_options(options, owner)?.create_tokio()?;
    secure_bound_socket(paths)?;
    Ok(listener)
  }

  fn from_listener(
    listener: Listener,
    owner_principal: IpcPrincipal,
    paths: &RuntimePaths,
    #[cfg(unix)] socket_claim_guard: crate::ipc_unix_security::SocketClaimGuard,
  ) -> Result<Self> {
    Ok(Self {
      listener,
      owner_principal,
      handshake_identity: ServerHandshakeIdentity::for_runtime(paths)?,
      #[cfg(unix)]
      socket_claim_guard: Some(socket_claim_guard),
    })
  }
}

async fn wait_for_endpoint_owner(paths: &RuntimePaths) -> bool {
  let deadlines = IpcClientDeadlines {
    connect: Duration::from_millis(100),
    ..IpcClientDeadlines::default()
  };
  for _ in 0..25 {
    if daemon_is_ready_with_deadlines(paths, deadlines)
      .await
      .unwrap_or(false)
    {
      return true;
    }
    sleep(Duration::from_millis(100)).await;
  }
  false
}

#[derive(Debug, Clone, Copy)]
struct ShutdownTimeline {
  started_at: Instant,
  limits: IpcLimits,
}

impl ShutdownTimeline {
  fn new(started_at: Instant, limits: IpcLimits) -> Self {
    Self { started_at, limits }
  }

  fn accept_deadline(self) -> Instant {
    self.started_at + self.limits.shutdown_accept
  }

  fn handler_deadline(self) -> Instant {
    self.accept_deadline() + self.limits.shutdown_handler_grace
  }

  fn runtime_deadline(self, phase_started_at: Instant) -> Instant {
    self.phase_deadline(
      phase_started_at,
      self.handler_deadline() + self.limits.shutdown_runtime,
      self.limits.shutdown_runtime,
    )
  }

  fn storage_deadline(self, phase_started_at: Instant) -> Instant {
    self.phase_deadline(
      phase_started_at,
      self.handler_deadline() + self.limits.shutdown_runtime + self.limits.shutdown_storage,
      self.limits.shutdown_storage,
    )
  }

  #[allow(dead_code)]
  fn cleanup_deadline(self, phase_started_at: Instant) -> Instant {
    self.phase_deadline(
      phase_started_at,
      self.handler_deadline()
        + self.limits.shutdown_runtime
        + self.limits.shutdown_storage
        + self.limits.shutdown_cleanup,
      self.limits.shutdown_cleanup,
    )
  }

  fn phase_deadline(
    self,
    phase_started_at: Instant,
    absolute_deadline: Instant,
    phase_budget: Duration,
  ) -> Instant {
    std::cmp::min(phase_started_at + phase_budget, absolute_deadline)
  }
}

#[derive(Debug)]
pub struct DaemonServer {
  paths: RuntimePaths,
  state: DaemonState,
  lease: Option<RuntimeEndpointLease>,
  security_policy: IpcSecurityPolicy,
  peer_identity_resolver: IpcPeerIdentityResolver,
  limits: IpcLimits,
}

impl DaemonServer {
  pub fn new(paths: RuntimePaths, state: DaemonState) -> Self {
    Self {
      paths,
      state,
      lease: None,
      security_policy: IpcSecurityPolicy,
      peer_identity_resolver: IpcPeerIdentityResolver::System,
      limits: IpcLimits::default(),
    }
  }

  pub(crate) fn with_lease(mut self, lease: RuntimeEndpointLease) -> Self {
    self.lease = Some(lease);
    self
  }

  #[cfg(test)]
  pub fn with_peer_principal(mut self, peer_principal: IpcPrincipal) -> Self {
    self.peer_identity_resolver = IpcPeerIdentityResolver::Fixed(peer_principal);
    self
  }

  #[cfg(test)]
  pub fn with_peer_identity_failure(mut self, kind: io::ErrorKind) -> Self {
    self.peer_identity_resolver = IpcPeerIdentityResolver::Failure(kind);
    self
  }

  #[cfg(test)]
  pub fn with_counting_peer_principal(
    mut self,
    peer_principal: IpcPrincipal,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
  ) -> Self {
    self.peer_identity_resolver = IpcPeerIdentityResolver::Counting {
      principal: peer_principal,
      calls,
    };
    self
  }

  #[cfg(test)]
  fn with_limits(mut self, limits: IpcLimits) -> Self {
    self.limits = limits;
    self
  }

  pub async fn run_until(self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    let lease = match self.lease {
      Some(lease) => lease,
      None => match RuntimeEndpointLease::claim(&self.paths).await? {
        Some(lease) => lease,
        None => anyhow::bail!("another Cadder daemon already owns the local endpoint"),
      },
    };
    let RuntimeEndpointLease {
      listener,
      owner_principal,
      handshake_identity,
      #[cfg(unix)]
      socket_claim_guard,
    } = lease;
    let shutdown_signal = self.state.shutdown_signal();
    let connection_permits = Arc::new(Semaphore::new(self.limits.max_connections));
    let stream_cancellation = CancellationToken::new();
    let request_drain = CancellationToken::new();
    let request_drain_deadline = Arc::new(OnceLock::new());
    let connection_cancellation = CancellationToken::new();
    let mut connection_tasks = JoinSet::new();
    let mutation_tasks = MutationTaskRegistry::default();
    let mutation_cancellation = CancellationToken::new();
    let mut server_failure = None;

    #[cfg(unix)]
    drop(socket_claim_guard);

    let _shutdown_origin = loop {
      tokio::select! {
          _ = shutdown_signal.wait() => break ShutdownOrigin::IpcRequest,
          changed = shutdown.changed() => {
              match changed {
                Ok(()) if *shutdown.borrow() => break ShutdownOrigin::ExternalSignal,
                Ok(()) => {}
                Err(_) => break ShutdownOrigin::ExternalSignal,
              }
          }
          _ = mutation_tasks.panic_detected() => {
            server_failure = Some(io::Error::other("owned mutation task panicked"));
            break ShutdownOrigin::ServerFailure;
          }
          joined = connection_tasks.join_next(), if !connection_tasks.is_empty() => {
              if let Some(Err(error)) = joined {
                server_failure = Some(io::Error::other(format!(
                  "local IPC connection task terminated unexpectedly: {error}"
                )));
                break ShutdownOrigin::ServerFailure;
              }
          }
          accepted = listener.accept() => {
              match accepted {
                  Ok(conn) => {
                      let accepted_at = Instant::now();
                      let Ok(connection_permit) = connection_permits.clone().try_acquire_owned()
                      else {
                        drop(conn);
                        continue;
                      };
                      let state = self.state.clone();
                      let owner_principal = owner_principal.clone();
                      let policy = self.security_policy.clone();
                      let peer_identity_resolver = self.peer_identity_resolver.clone();
                      let handshake_identity = handshake_identity.clone();
                      let mutation_tasks = mutation_tasks.clone();
                      let stream_cancellation = stream_cancellation.clone();
                      let request_drain = request_drain.clone();
                      let request_drain_deadline = request_drain_deadline.clone();
                      let mutation_cancellation = mutation_cancellation.clone();
                      let connection_cancellation = connection_cancellation.child_token();
                      let limits = self.limits;
                      connection_tasks.spawn(async move {
                          let _connection_permit = connection_permit;
                          serve_accepted_connection(conn, AcceptedConnectionContext {
                            state,
                            owner_principal,
                            policy,
                            peer_identity_resolver,
                            handshake_identity,
                            mutation_tasks,
                            control: ConnectionControl {
                              accepted_at,
                              limits,
                              stream_cancellation,
                              request_drain,
                              request_drain_deadline,
                              mutation_cancellation,
                              connection_cancellation,
                            },
                          })
                          .await;
                      });
                  }
                  Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                  Err(error) => {
                    server_failure = Some(error);
                    break ShutdownOrigin::ServerFailure;
                  }
              }
          }
      }
    };

    let shutdown_timeline = ShutdownTimeline::new(
      shutdown_signal.started_at().unwrap_or_else(Instant::now),
      self.limits,
    );
    self.state.begin_operation_drain();
    let _listener_lease = listener;
    let accept_deadline = shutdown_timeline.accept_deadline();
    request_drain_deadline
      .set(accept_deadline)
      .expect("request drain deadline is set once");
    request_drain.cancel();
    stream_cancellation.cancel();
    let handler_deadline = shutdown_timeline.handler_deadline();
    let abort_join_budget = std::cmp::min(
      self.limits.shutdown_connection_abort_join,
      self.limits.shutdown_handler_grace / 2,
    );
    let graceful_deadline = handler_deadline - abort_join_budget;
    mutation_tasks.close();
    let mut handler_failure = None;
    match timeout_at(
      graceful_deadline,
      drain_handler_tasks(&mut connection_tasks, &mutation_tasks, false),
    )
    .await
    {
      Ok(Ok(())) => {}
      Ok(Err(error)) => handler_failure = Some(error),
      Err(_) => {
        mutation_cancellation.cancel();
        connection_cancellation.cancel();
        connection_tasks.abort_all();
        match timeout_at(
          handler_deadline,
          drain_handler_tasks(&mut connection_tasks, &mutation_tasks, true),
        )
        .await
        {
          Ok(Ok(())) => {}
          Ok(Err(error)) => handler_failure = Some(error),
          Err(_) => {
            handler_failure = Some(anyhow::anyhow!(
              "IPC handlers did not finish within the shutdown grace; started mutations remained contained until rollback completed"
            ));
            self.state.logs().append(
              LogStreamIdentity::runtime_control(),
              LogSeverity::Error,
              "An owned mutation exceeded the shutdown grace; Cadder keeps local endpoint ownership until the mutation finishes rollback.",
              LogAttributionKind::RuntimeControl,
              Some("shutdown-mutation-containment".to_string()),
            );
            // A started mutation may be performing an asynchronous rollback. Keep the
            // daemon, runtime, and endpoint lease owned until that rollback
            // finishes instead of detaching or aborting it at an unsafe commit point.
            if let Err(error) =
              drain_handler_tasks(&mut connection_tasks, &mutation_tasks, true).await
            {
              handler_failure = Some(error);
            }
          }
        }
      }
    }
    let runtime_deadline = shutdown_timeline.runtime_deadline(Instant::now());
    let runtime_shutdown = self.state.prepare_shutdown_until(runtime_deadline).await;
    if !runtime_shutdown.runtime_quiescent {
      self.state.contain_runtime_fail_stop().await;
    }
    let storage_deadline = shutdown_timeline.storage_deadline(Instant::now());
    let storage_shutdown = match self.state.shutdown_storage_until(storage_deadline).await {
      Ok(true) => Ok(()),
      Ok(false) => self.state.contain_storage_shutdown().await,
      Err(error) => Err(error),
    };
    if !runtime_shutdown.response.accepted {
      anyhow::bail!(runtime_shutdown.response.message);
    }
    storage_shutdown.context("flush and join runtime storage")?;
    if let Some(error) = handler_failure {
      return Err(error).context("drain local IPC connection tasks");
    }
    if let Some(error) = server_failure {
      return Err(error).context("accept local IPC connection");
    }
    Ok(())
  }
}

#[cfg(unix)]
fn local_socket_name(paths: &RuntimePaths) -> io::Result<Name<'static>> {
  crate::ipc_unix_security::unix_listener_name(paths)
}

#[cfg(windows)]
fn local_socket_name(paths: &RuntimePaths) -> io::Result<Name<'static>> {
  paths
    .socket_name()
    .to_ns_name::<GenericNamespaced>()
    .map(Name::into_owned)
}

#[derive(Debug, thiserror::Error)]
enum PeerAuthenticationError {
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

async fn authenticate_accepted_connection(
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

async fn serve_accepted_connection(conn: Stream, context: AcceptedConnectionContext) {
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
    Ok(Err(error)) => log_peer_authentication_denial(&state, &error),
    Err(_) => {}
  }
}

fn log_peer_authentication_denial(state: &DaemonState, error: &PeerAuthenticationError) {
  state.logs().append(
    LogStreamIdentity::runtime_control(),
    LogSeverity::Warn,
    format!(
      "Cadder rejected a local connection before any request ran; runtime state is unchanged. Use the runtime owner account or inspect security diagnostics. Reason: {}",
      error.reason_code()
    ),
    LogAttributionKind::RuntimeControl,
    Some("ipc-peer-denied".to_string()),
  );
}

#[derive(Debug, Clone)]
struct ConnectionSecurityContext {
  owner_principal: IpcPrincipal,
  policy: IpcSecurityPolicy,
  peer_principal: IpcPrincipal,
}

#[derive(Debug, Clone)]
struct ConnectionControl {
  accepted_at: Instant,
  limits: IpcLimits,
  stream_cancellation: CancellationToken,
  request_drain: CancellationToken,
  request_drain_deadline: Arc<OnceLock<Instant>>,
  mutation_cancellation: CancellationToken,
  connection_cancellation: CancellationToken,
}

async fn join_connection_tasks(tasks: &mut JoinSet<()>, allow_cancelled: bool) -> Result<()> {
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

async fn drain_handler_tasks(
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
struct MutationTaskRegistry {
  state: Arc<std::sync::Mutex<MutationTaskRegistryState>>,
  panic_signal: CancellationToken,
}

impl MutationTaskRegistry {
  fn spawn<F>(&self, task: F) -> Option<JoinHandle<F::Output>>
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

  fn close(&self) {
    let mut state = self
      .state
      .lock()
      .expect("mutation task registry lock poisoned");
    state.closed = true;
    state.tracker.close();
  }

  async fn wait(&self) -> Result<()> {
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

  async fn panic_detected(&self) {
    self.panic_signal.cancelled().await;
  }
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
  payload
    .downcast_ref::<&'static str>()
    .copied()
    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
    .unwrap_or("non-string panic payload")
}

async fn handle_connection(
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

struct ConnectionCleanupGuard(CancellationToken);

impl Drop for ConnectionCleanupGuard {
  fn drop(&mut self) {
    self.0.cancel();
  }
}

async fn handle_connection_loop(
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

    let envelope: IpcEnvelope = match serde_json::from_str(&line) {
      Ok(envelope) => envelope,
      Err(error) => {
        let response =
          ProtocolErrorResponse::rejected(None, ProtocolError::payload_decode_failed(error));
        send_response!(message_types::PROTOCOL_ERROR_RESPONSE, response);
        continue;
      }
    };
    if control.request_drain.is_cancelled() {
      send_shutting_down(
        &mut write_half,
        request_id_from_payload(&envelope),
        control.limits,
      )
      .await?;
      break;
    }
    if !authorize_or_reject(&mut write_half, &state, &envelope, security).await? {
      continue;
    }
    let authorized = match authorize_legacy(&envelope) {
      Ok(authorized) => authorized,
      Err(error) => {
        let response = ProtocolErrorResponse::rejected(request_id_from_payload(&envelope), error);
        send_response!(message_types::PROTOCOL_ERROR_RESPONSE, response);
        continue;
      }
    };
    let action = match authorized.definition().shape() {
      OperationShape::Unary => {
        supervise_unary_request(
          &mut reader,
          &mut write_half,
          &authorized,
          &envelope,
          UnarySupervisionContext {
            state: &state,
            owned,
            mutation_tasks,
            mutation_cancellation: &control.mutation_cancellation,
            request_drain: &control.request_drain,
            limits: control.limits,
          },
        )
        .await?
      }
      OperationShape::ServerStream => {
        supervise_state_subscription(
          &mut reader,
          &mut write_half,
          &state,
          &authorized,
          control.stream_cancellation.child_token(),
          control.limits,
        )
        .await?
      }
    };
    if action == ConnectionAction::Close {
      break;
    }
  }

  Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionAction {
  Continue,
  Close,
}

async fn read_request_or_drain(
  reader: &mut IpcFrameReader,
  control: &ConnectionControl,
) -> Option<Result<String, IpcCodecError>> {
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

enum ConcurrentRead {
  Pipelined(Option<RequestId>),
  Closed,
}

const STREAM_CONTROL_RESERVE_BYTES: usize = 1024;

#[derive(Debug)]
struct EncodedStateStreamRecord {
  frame: Vec<u8>,
  kind: StateStreamRecordKind,
  delivery_deadline: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StateStreamRecordKind {
  Event(u64),
  Heartbeat,
  Gap { first: u64, last: u64 },
}

impl EncodedStateStreamRecord {
  fn event(event: &StateChangedEvent) -> std::result::Result<Self, IpcCodecError> {
    Ok(Self {
      frame: encode_envelope(message_types::STATE_CHANGED_EVENT, event)?,
      kind: StateStreamRecordKind::Event(event.sequence_number),
      delivery_deadline: None,
    })
  }

  fn heartbeat(heartbeat: &StateStreamHeartbeat) -> std::result::Result<Self, IpcCodecError> {
    Ok(Self {
      frame: encode_envelope(message_types::STATE_STREAM_HEARTBEAT, heartbeat)?,
      kind: StateStreamRecordKind::Heartbeat,
      delivery_deadline: None,
    })
  }

  fn gap(
    gap: &StateStreamGap,
    delivery_deadline: Instant,
  ) -> std::result::Result<Self, IpcCodecError> {
    Ok(Self {
      frame: encode_envelope(message_types::STATE_STREAM_GAP, gap)?,
      kind: StateStreamRecordKind::Gap {
        first: gap.first_missing_sequence_number,
        last: gap.last_missing_sequence_number,
      },
      delivery_deadline: Some(delivery_deadline),
    })
  }

  fn missing_range(&self) -> Option<(u64, u64)> {
    match self.kind {
      StateStreamRecordKind::Event(sequence) => Some((sequence, sequence)),
      StateStreamRecordKind::Gap { first, last } => Some((first, last)),
      StateStreamRecordKind::Heartbeat => None,
    }
  }
}

#[derive(Debug)]
struct StateStreamBuffer {
  records: VecDeque<EncodedStateStreamRecord>,
  total_records: usize,
  total_bytes: usize,
  max_records: usize,
  max_bytes: usize,
}

impl StateStreamBuffer {
  fn new(max_records: usize, max_bytes: usize) -> Self {
    Self {
      records: VecDeque::new(),
      total_records: 0,
      total_bytes: 0,
      max_records,
      max_bytes,
    }
  }

  fn enqueue_event(&mut self, event: &StateChangedEvent, gap_deadline: Instant) -> Result<()> {
    if self.has_queued_gap() {
      return self.record_gap(
        &event.request_id,
        event.sequence_number,
        event.sequence_number,
        gap_deadline,
      );
    }

    let record = EncodedStateStreamRecord::event(event)?;
    let data_record_limit = self.max_records.saturating_sub(1);
    let data_byte_limit = self.max_bytes.saturating_sub(STREAM_CONTROL_RESERVE_BYTES);
    if self.total_records < data_record_limit
      && record.frame.len() <= data_byte_limit.saturating_sub(self.total_bytes)
    {
      self.push_back(record);
      return Ok(());
    }

    self.record_gap(
      &event.request_id,
      event.sequence_number,
      event.sequence_number,
      gap_deadline,
    )
  }

  fn enqueue_heartbeat(&mut self, heartbeat: &StateStreamHeartbeat) -> Result<()> {
    if self.records.iter().any(|record| {
      matches!(
        record.kind,
        StateStreamRecordKind::Heartbeat | StateStreamRecordKind::Gap { .. }
      )
    }) {
      return Ok(());
    }
    let record = EncodedStateStreamRecord::heartbeat(heartbeat)?;
    if self.total_records >= self.max_records
      || record.frame.len() > self.max_bytes.saturating_sub(self.total_bytes)
    {
      anyhow::bail!("state stream control record does not fit its bounded queue");
    }
    self.push_back(record);
    Ok(())
  }

  fn record_gap(
    &mut self,
    request_id: &str,
    first: u64,
    last: u64,
    delivery_deadline: Instant,
  ) -> Result<()> {
    let mut first_missing = first;
    let mut last_missing = last;
    let mut earliest_deadline = delivery_deadline;
    while let Some(record) = self.records.pop_front() {
      self.total_records -= 1;
      self.total_bytes -= record.frame.len();
      if let Some(record_deadline) = record.delivery_deadline {
        earliest_deadline = earliest_deadline.min(record_deadline);
      }
      if let Some((record_first, record_last)) = record.missing_range() {
        first_missing = first_missing.min(record_first);
        last_missing = last_missing.max(record_last);
      }
    }

    let gap = StateStreamGap {
      request_id: request_id.to_string(),
      first_missing_sequence_number: first_missing,
      last_missing_sequence_number: last_missing,
    };
    let record = EncodedStateStreamRecord::gap(&gap, earliest_deadline)?;
    if self.total_records >= self.max_records
      || record.frame.len() > self.max_bytes.saturating_sub(self.total_bytes)
    {
      anyhow::bail!("state stream gap does not fit its bounded queue");
    }
    self.push_back(record);
    Ok(())
  }

  fn has_queued_gap(&self) -> bool {
    self
      .records
      .iter()
      .any(|record| matches!(record.kind, StateStreamRecordKind::Gap { .. }))
  }

  fn gap_deadline(&self) -> Option<Instant> {
    self
      .records
      .iter()
      .filter_map(|record| record.delivery_deadline)
      .min()
  }

  fn take_next(&mut self) -> Option<EncodedStateStreamRecord> {
    self.records.pop_front()
  }

  fn complete(&mut self, record: &EncodedStateStreamRecord) {
    self.total_records -= 1;
    self.total_bytes -= record.frame.len();
  }

  fn push_back(&mut self, record: EncodedStateStreamRecord) {
    self.total_records += 1;
    self.total_bytes += record.frame.len();
    self.records.push_back(record);
    debug_assert!(self.total_records <= self.max_records);
    debug_assert!(self.total_bytes <= self.max_bytes);
  }
}

async fn supervise_unary_request<W>(
  reader: &mut IpcFrameReader,
  writer: &mut W,
  authorized: &AuthorizedLegacyEnvelope<'_>,
  envelope: &IpcEnvelope,
  supervision: UnarySupervisionContext<'_>,
) -> Result<ConnectionAction>
where
  W: AsyncWrite + Unpin,
{
  let state = supervision.state;
  let owned = supervision.owned;
  let limits = supervision.limits;
  let definition = authorized.definition();
  let deadline = limits.operation_deadline(definition.deadline());
  let request_id = authorized.request_id();
  let operation_fence = if operation_uses_fence(definition) {
    match state.issue_operation_fence() {
      Ok(fence) => Some(fence),
      Err(CommitRejection::Draining) => {
        send_shutting_down(writer, request_id, limits).await?;
        return Ok(ConnectionAction::Close);
      }
      Err(error) => return Err(error.into()),
    }
  } else {
    None
  };
  if owned_mutation_uses_worker(definition.name()) {
    return supervise_owned_mutation_request(
      reader,
      writer,
      authorized,
      operation_fence.expect("registration mutation has an operation fence"),
      deadline,
      supervision,
    )
    .await;
  }
  let mut handler = Box::pin(dispatch_authorized_request(
    writer,
    state,
    owned,
    authorized,
    envelope,
    RequestDispatchContext {
      operation_fence: operation_fence.as_ref(),
      deadline,
      limits,
    },
  ));
  let mut next_frame = Box::pin(reader.next());

  enum First<T> {
    Handler(T),
    Reader(ConcurrentRead),
    Cancelled,
    ShuttingDown,
    Timeout,
  }

  let first = tokio::select! {
    biased;
    _ = wait_for_request_drain(definition, supervision.request_drain) => First::ShuttingDown,
    _ = wait_for_operation_cancellation(operation_fence.as_ref()) => First::Cancelled,
    frame = &mut next_frame => First::Reader(classify_concurrent_read(frame)),
    _ = sleep_until(deadline) => First::Timeout,
    result = &mut handler => First::Handler(result),
  };

  match first {
    First::Handler(result) => {
      if let Some(fence) = &operation_fence {
        fence.complete();
      }
      let action = result?;
      drop(handler);
      drop(next_frame);
      if reader.read_buffer().is_empty() {
        Ok(action)
      } else {
        send_pipelined_error(writer, None, limits).await?;
        Ok(ConnectionAction::Close)
      }
    }
    First::Reader(concurrent) => {
      drop(next_frame);
      enum FollowUp<T> {
        Handler(T),
        ShuttingDown,
        Cancelled,
        Timeout,
      }
      let handler_result = tokio::select! {
        biased;
        _ = wait_for_request_drain(definition, supervision.request_drain) => FollowUp::ShuttingDown,
        _ = wait_for_operation_cancellation(operation_fence.as_ref()) => FollowUp::Cancelled,
        _ = sleep_until(deadline) => FollowUp::Timeout,
        result = &mut handler => FollowUp::Handler(result),
      };

      match handler_result {
        FollowUp::Handler(result) => {
          if let Some(fence) = &operation_fence {
            fence.complete();
          }
          drop(handler);
          result?;
        }
        FollowUp::ShuttingDown | FollowUp::Cancelled => {
          if let Some(fence) = &operation_fence {
            fence.revoke();
          }
          drop(handler);
          send_request_shutting_down(writer, request_id, limits).await?;
          return Ok(ConnectionAction::Close);
        }
        FollowUp::Timeout => {
          if let Some(fence) = &operation_fence {
            fence.revoke();
          }
          drop(handler);
          send_operation_timeout(writer, request_id, definition, limits).await?;
        }
      }
      if let ConcurrentRead::Pipelined(pipelined_request_id) = concurrent {
        send_pipelined_error(writer, pipelined_request_id, limits).await?;
      }
      Ok(ConnectionAction::Close)
    }
    First::Cancelled => {
      if let Some(fence) = &operation_fence {
        fence.revoke();
      }
      drop(handler);
      drop(next_frame);
      Ok(ConnectionAction::Close)
    }
    First::ShuttingDown => {
      if let Some(fence) = &operation_fence {
        fence.revoke();
      }
      drop(handler);
      drop(next_frame);
      send_request_shutting_down(writer, request_id, limits).await?;
      Ok(ConnectionAction::Close)
    }
    First::Timeout => {
      if let Some(fence) = &operation_fence {
        fence.revoke();
      }
      drop(handler);
      drop(next_frame);
      send_operation_timeout(writer, request_id, definition, limits).await?;
      Ok(ConnectionAction::Close)
    }
  }
}

fn owned_mutation_uses_worker(message_type: &str) -> bool {
  matches!(
    message_type,
    message_types::REGISTER_ENTRYPOINT_REQUEST
      | message_types::UNREGISTER_ENTRYPOINT_REQUEST
      | message_types::HEARTBEAT_ENTRYPOINT_REQUEST
      | message_types::SET_ENTRYPOINT_ENABLED_REQUEST
      | message_types::SET_DOMAIN_ENABLED_REQUEST
      | message_types::SET_AUTOSTART_REQUEST
  )
}

async fn supervise_owned_mutation_request<W>(
  reader: &mut IpcFrameReader,
  writer: &mut W,
  authorized: &AuthorizedLegacyEnvelope<'_>,
  fence: OperationFence,
  deadline: Instant,
  supervision: UnarySupervisionContext<'_>,
) -> Result<ConnectionAction>
where
  W: AsyncWrite + Unpin,
{
  let UnarySupervisionContext {
    state,
    owned,
    mutation_tasks,
    mutation_cancellation,
    request_drain: _,
    limits,
  } = supervision;
  macro_rules! decode_owned_request {
    ($request:ty, $map:expr) => {
      match authorized.decode::<$request>() {
        Ok(request) => $map(request),
        Err(error) => {
          let response = ProtocolErrorResponse::rejected(authorized.request_id(), error);
          write_envelope_until(
            writer,
            message_types::PROTOCOL_ERROR_RESPONSE,
            &response,
            deadline,
            limits.write_no_progress,
          )
          .await?;
          return Ok(ConnectionAction::Continue);
        }
      }
    };
  }
  let request = match authorized.definition().name() {
    message_types::REGISTER_ENTRYPOINT_REQUEST => {
      decode_owned_request!(RegisterEntrypointRequest, |request| {
        OwnedMutationRequest::Register(Box::new(request))
      })
    }
    message_types::UNREGISTER_ENTRYPOINT_REQUEST => decode_owned_request!(
      UnregisterEntrypointRequest,
      OwnedMutationRequest::Unregister
    ),
    message_types::HEARTBEAT_ENTRYPOINT_REQUEST => {
      decode_owned_request!(HeartbeatEntrypointRequest, OwnedMutationRequest::Heartbeat)
    }
    message_types::SET_ENTRYPOINT_ENABLED_REQUEST => decode_owned_request!(
      SetEntrypointEnabledRequest,
      OwnedMutationRequest::SetEntrypointEnabled
    ),
    message_types::SET_DOMAIN_ENABLED_REQUEST => decode_owned_request!(
      SetDomainEnabledRequest,
      OwnedMutationRequest::SetDomainEnabled
    ),
    message_types::SET_AUTOSTART_REQUEST => {
      decode_owned_request!(SetAutostartRequest, OwnedMutationRequest::SetAutostart)
    }
    _ => unreachable!("owned mutation supervisor called for an untracked operation"),
  };
  let request_id = authorized.request_id();
  let response_type = request.response_type();
  let Some(mutation_ownership) = owned.begin_mutation() else {
    let _ = fence.try_revoke();
    send_shutting_down(writer, request_id, limits).await?;
    return Ok(ConnectionAction::Close);
  };
  let worker_state = state.clone();
  let worker_fence = fence.clone();
  let completion_fence = fence.clone();
  let worker_ownership = owned.clone();
  let worker_cancellation = mutation_cancellation.clone();
  let worker = async move {
    let _mutation_ownership = mutation_ownership;
    #[cfg(test)]
    tokio::select! {
      biased;
      _ = worker_cancellation.cancelled() => return Err(CommitRejection::Revoked),
      _ = sleep(limits.dispatch_delay) => {}
    }
    let mut execution = Box::pin(async move {
      request
        .execute(worker_state, worker_ownership, worker_fence)
        .await
    });
    tokio::select! {
      result = &mut execution => result,
      _ = worker_cancellation.cancelled() => {
        let _ = completion_fence.try_revoke();
        execution.await
      }
    }
  };
  let Some(mut worker) = mutation_tasks.spawn(worker) else {
    let _ = fence.try_revoke();
    send_shutting_down(writer, request_id, limits).await?;
    return Ok(ConnectionAction::Close);
  };
  let mut next_frame = Box::pin(reader.next());
  let cancellation = fence.cancellation();

  enum First<T> {
    Worker(T),
    Reader(ConcurrentRead),
    ShuttingDown,
    Cancelled,
    Timeout,
  }

  let first = tokio::select! {
    biased;
    _ = supervision.request_drain.cancelled() => First::ShuttingDown,
    _ = cancellation.cancelled() => First::Cancelled,
    frame = &mut next_frame => First::Reader(classify_concurrent_read(frame)),
    result = &mut worker => First::Worker(result),
    _ = sleep_until(deadline) => First::Timeout,
  };

  match first {
    First::Worker(result) => {
      let response = result.context("owned mutation worker failed")??;
      let wrote_response = write_owned_mutation_result(
        writer,
        &response,
        OwnedMutationWriteContext {
          fence: &fence,
          request_id: request_id.as_ref(),
          definition: authorized.definition(),
          response_type,
          deadline,
          limits,
        },
      )
      .await?;
      drop(next_frame);
      if !wrote_response {
        return Ok(ConnectionAction::Close);
      }
      if reader.read_buffer().is_empty() {
        Ok(ConnectionAction::Continue)
      } else {
        send_pipelined_error(writer, None, limits).await?;
        Ok(ConnectionAction::Close)
      }
    }
    First::Reader(concurrent) => {
      drop(next_frame);
      enum FollowUp<T> {
        Worker(T),
        ShuttingDown,
        Cancelled,
        Timeout,
      }
      let worker_result = tokio::select! {
        biased;
        _ = supervision.request_drain.cancelled() => FollowUp::ShuttingDown,
        _ = cancellation.cancelled() => FollowUp::Cancelled,
        result = &mut worker => FollowUp::Worker(result),
        _ = sleep_until(deadline) => FollowUp::Timeout,
      };

      match worker_result {
        FollowUp::Worker(result) => {
          let response = result.context("owned mutation worker failed")??;
          let _ = write_owned_mutation_result(
            writer,
            &response,
            OwnedMutationWriteContext {
              fence: &fence,
              request_id: request_id.as_ref(),
              definition: authorized.definition(),
              response_type,
              deadline,
              limits,
            },
          )
          .await?;
        }
        FollowUp::ShuttingDown => {
          match fence.try_revoke() {
            RevokeOutcome::Finalized => {
              let response = (&mut worker)
                .await
                .context("owned mutation worker failed")??;
              write_envelope_until(
                writer,
                response_type,
                &response,
                Instant::now() + limits.write_no_progress,
                limits.write_no_progress,
              )
              .await?;
            }
            RevokeOutcome::Revoked | RevokeOutcome::AlreadyRevoked => {
              drop(worker);
              send_request_shutting_down(writer, request_id, limits).await?;
            }
          }
          return Ok(ConnectionAction::Close);
        }
        FollowUp::Cancelled => {
          let response = (&mut worker)
            .await
            .context("owned mutation worker failed")??;
          write_envelope_until(
            writer,
            response_type,
            &response,
            Instant::now() + limits.write_no_progress,
            limits.write_no_progress,
          )
          .await?;
        }
        FollowUp::Timeout => match fence.try_revoke() {
          RevokeOutcome::Revoked => {
            drop(worker);
            send_operation_timeout(writer, request_id, authorized.definition(), limits).await?;
          }
          RevokeOutcome::Finalized => {
            let response = (&mut worker)
              .await
              .context("owned mutation worker failed")??;
            write_envelope_until(
              writer,
              response_type,
              &response,
              Instant::now() + limits.write_no_progress,
              limits.write_no_progress,
            )
            .await?;
          }
          RevokeOutcome::AlreadyRevoked => drop(worker),
        },
      }
      if let ConcurrentRead::Pipelined(pipelined_request_id) = concurrent {
        send_pipelined_error(writer, pipelined_request_id, limits).await?;
      }
      Ok(ConnectionAction::Close)
    }
    First::ShuttingDown => {
      match fence.try_revoke() {
        RevokeOutcome::Finalized => {
          let response = worker.await.context("owned mutation worker failed")??;
          write_envelope_until(
            writer,
            response_type,
            &response,
            Instant::now() + limits.write_no_progress,
            limits.write_no_progress,
          )
          .await?;
        }
        RevokeOutcome::Revoked | RevokeOutcome::AlreadyRevoked => {
          drop(worker);
          send_request_shutting_down(writer, request_id, limits).await?;
        }
      }
      drop(next_frame);
      Ok(ConnectionAction::Close)
    }
    First::Cancelled => {
      let response = worker.await.context("owned mutation worker failed")??;
      write_envelope_until(
        writer,
        response_type,
        &response,
        Instant::now() + limits.write_no_progress,
        limits.write_no_progress,
      )
      .await?;
      drop(next_frame);
      Ok(ConnectionAction::Close)
    }
    First::Timeout => {
      drop(next_frame);
      match fence.try_revoke() {
        RevokeOutcome::Revoked => {
          drop(worker);
          send_operation_timeout(writer, request_id, authorized.definition(), limits).await?;
        }
        RevokeOutcome::Finalized => {
          let response = worker.await.context("owned mutation worker failed")??;
          write_envelope_until(
            writer,
            response_type,
            &response,
            Instant::now() + limits.write_no_progress,
            limits.write_no_progress,
          )
          .await?;
        }
        RevokeOutcome::AlreadyRevoked => drop(worker),
      }
      Ok(ConnectionAction::Close)
    }
  }
}

async fn write_owned_mutation_result<W>(
  writer: &mut W,
  response: &OwnedMutationResponse,
  context: OwnedMutationWriteContext<'_>,
) -> Result<bool>
where
  W: AsyncWrite + Unpin,
{
  let write_deadline = if Instant::now() < context.deadline {
    context.fence.complete();
    context.deadline
  } else {
    match context.fence.try_revoke() {
      RevokeOutcome::Finalized => Instant::now() + context.limits.write_no_progress,
      RevokeOutcome::Revoked => {
        send_operation_timeout(
          writer,
          context.request_id.cloned(),
          context.definition,
          context.limits,
        )
        .await?;
        return Ok(false);
      }
      RevokeOutcome::AlreadyRevoked => return Ok(false),
    }
  };
  write_envelope_until(
    writer,
    context.response_type,
    response,
    write_deadline,
    context.limits.write_no_progress,
  )
  .await?;
  Ok(true)
}

enum OwnedMutationRequest {
  Register(Box<RegisterEntrypointRequest>),
  Unregister(UnregisterEntrypointRequest),
  Heartbeat(HeartbeatEntrypointRequest),
  SetEntrypointEnabled(SetEntrypointEnabledRequest),
  SetDomainEnabled(SetDomainEnabledRequest),
  SetAutostart(SetAutostartRequest),
}

impl OwnedMutationRequest {
  fn response_type(&self) -> &'static str {
    match self {
      Self::Register(_) => message_types::REGISTER_ENTRYPOINT_RESPONSE,
      Self::Unregister(_) => message_types::UNREGISTER_ENTRYPOINT_RESPONSE,
      Self::Heartbeat(_) => message_types::HEARTBEAT_ENTRYPOINT_RESPONSE,
      Self::SetEntrypointEnabled(_) => message_types::SET_ENTRYPOINT_ENABLED_RESPONSE,
      Self::SetDomainEnabled(_) => message_types::SET_DOMAIN_ENABLED_RESPONSE,
      Self::SetAutostart(_) => message_types::SET_AUTOSTART_RESPONSE,
    }
  }

  async fn execute(
    self,
    state: DaemonState,
    ownership: ConnectionOwnership,
    fence: OperationFence,
  ) -> Result<OwnedMutationResponse, CommitRejection> {
    match self {
      Self::Register(request) => {
        let nonce = request
          .registration
          .entrypoint_instance
          .shim_session_nonce
          .clone();
        let response = state
          .register_fenced(request.request_id, request.registration, &fence)
          .await?;
        if let Some(registration_id) = response
          .registration_id
          .as_ref()
          .filter(|_| response.accepted)
        {
          ownership.insert(registration_id.clone(), nonce);
        }
        Ok(OwnedMutationResponse::Register(response))
      }
      Self::Unregister(request) => {
        let registration_id = request.registration_id.clone();
        let response = state
          .unregister_fenced(
            request.request_id,
            &registration_id,
            &request.shim_session_nonce,
            &fence,
          )
          .await?;
        if response.accepted {
          ownership.remove(&registration_id);
        }
        Ok(OwnedMutationResponse::Basic(response))
      }
      Self::Heartbeat(request) => Ok(OwnedMutationResponse::Basic(
        state.heartbeat_fenced(request, &fence).await?,
      )),
      Self::SetEntrypointEnabled(request) => Ok(OwnedMutationResponse::Basic(
        state.set_entrypoint_enabled_fenced(request, &fence).await?,
      )),
      Self::SetDomainEnabled(request) => Ok(OwnedMutationResponse::Basic(
        state.set_domain_enabled_fenced(request, &fence).await?,
      )),
      Self::SetAutostart(request) => Ok(OwnedMutationResponse::Autostart(
        state.set_autostart_fenced(request, &fence).await?,
      )),
    }
  }
}

#[derive(Serialize)]
#[serde(untagged)]
enum OwnedMutationResponse {
  Register(cadder_ipc::RegisterEntrypointResponse),
  Basic(cadder_ipc::BasicResponse),
  Autostart(cadder_ipc::SetAutostartResponse),
}

fn operation_uses_fence(definition: &cadder_ipc::OperationDefinition) -> bool {
  definition.access() == OperationAccess::Mutation
    && definition.name() != message_types::SHUTDOWN_DAEMON_REQUEST
}

async fn wait_for_operation_cancellation(fence: Option<&OperationFence>) {
  match fence {
    Some(fence) => {
      let cancellation = fence.cancellation();
      cancellation.cancelled().await;
    }
    None => std::future::pending().await,
  }
}

async fn wait_for_request_drain(
  definition: &cadder_ipc::OperationDefinition,
  request_drain: &CancellationToken,
) {
  if definition.name() == message_types::SHUTDOWN_DAEMON_REQUEST {
    std::future::pending().await
  } else {
    request_drain.cancelled().await
  }
}

async fn supervise_state_subscription<W>(
  reader: &mut IpcFrameReader,
  writer: &mut W,
  state: &DaemonState,
  authorized: &AuthorizedLegacyEnvelope<'_>,
  cancellation: CancellationToken,
  limits: IpcLimits,
) -> Result<ConnectionAction>
where
  W: AsyncWrite + Unpin,
{
  let definition = authorized.definition();
  let deadline = limits.operation_deadline(definition.deadline());
  let request_id = authorized.request_id();
  let mut setup = Box::pin(prepare_state_subscription(
    writer, state, authorized, deadline, limits,
  ));
  let mut next_frame = Box::pin(reader.next());

  enum SetupFirst<T> {
    Setup(T),
    Reader(ConcurrentRead),
    Cancelled,
    Timeout,
  }

  let first = tokio::select! {
    biased;
    frame = &mut next_frame => SetupFirst::Reader(classify_concurrent_read(frame)),
    _ = cancellation.cancelled() => SetupFirst::Cancelled,
    _ = sleep_until(deadline) => SetupFirst::Timeout,
    result = &mut setup => SetupFirst::Setup(result),
  };
  drop(next_frame);

  let Some((stream_request_id, initial_sequence, mut subscription)) = (match first {
    SetupFirst::Setup(result) => {
      let prepared = result?;
      drop(setup);
      if !reader.read_buffer().is_empty() {
        send_pipelined_error(writer, None, limits).await?;
        return Ok(ConnectionAction::Close);
      }
      prepared
    }
    SetupFirst::Reader(concurrent) => {
      enum SetupFollowUp<T> {
        Setup(T),
        Cancelled,
        Timeout,
      }
      let setup_result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => SetupFollowUp::Cancelled,
        _ = sleep_until(deadline) => SetupFollowUp::Timeout,
        result = &mut setup => SetupFollowUp::Setup(result),
      };
      drop(setup);
      match setup_result {
        SetupFollowUp::Setup(result) => {
          result?;
        }
        SetupFollowUp::Cancelled => {
          send_request_shutting_down(writer, request_id, limits).await?;
          return Ok(ConnectionAction::Close);
        }
        SetupFollowUp::Timeout => {
          send_operation_timeout(writer, request_id, definition, limits).await?;
        }
      }
      if let ConcurrentRead::Pipelined(pipelined_request_id) = concurrent {
        send_pipelined_error(writer, pipelined_request_id, limits).await?;
      }
      return Ok(ConnectionAction::Close);
    }
    SetupFirst::Cancelled => {
      drop(setup);
      send_request_shutting_down(writer, request_id, limits).await?;
      return Ok(ConnectionAction::Close);
    }
    SetupFirst::Timeout => {
      drop(setup);
      send_operation_timeout(writer, request_id, definition, limits).await?;
      return Ok(ConnectionAction::Close);
    }
  }) else {
    return Ok(ConnectionAction::Continue);
  };

  run_active_state_subscription(
    reader,
    writer,
    &stream_request_id,
    initial_sequence,
    &mut subscription,
    cancellation,
    limits,
  )
  .await
}

async fn run_active_state_subscription<W>(
  reader: &mut IpcFrameReader,
  writer: &mut W,
  request_id: &str,
  initial_sequence: u64,
  subscription: &mut tokio::sync::broadcast::Receiver<StateChangedEvent>,
  cancellation: CancellationToken,
  limits: IpcLimits,
) -> Result<ConnectionAction>
where
  W: AsyncWrite + Unpin,
{
  let mut buffer = StateStreamBuffer::new(limits.stream_max_records, limits.stream_max_bytes);
  let mut in_flight = None;
  let mut last_observed_sequence = initial_sequence;
  let mut heartbeat_deadline = Instant::now() + limits.stream_heartbeat;

  loop {
    if in_flight.is_none() {
      in_flight = buffer.take_next();
    }

    let Some(record) = in_flight.as_ref() else {
      tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
          send_stream_shutting_down(writer, request_id, limits).await?;
          return Ok(ConnectionAction::Close);
        },
        frame = reader.next() => {
          return close_state_stream_for_client_input(writer, frame, limits).await;
        }
        event = subscription.recv() => {
          if !queue_state_stream_event(
            event,
            request_id,
            &mut last_observed_sequence,
            &mut buffer,
            limits,
          )? {
            return Ok(ConnectionAction::Close);
          }
        }
        _ = sleep_until(heartbeat_deadline) => {
          buffer.enqueue_heartbeat(&StateStreamHeartbeat {
            request_id: request_id.to_string(),
            last_sequence_number: last_observed_sequence,
          })?;
          heartbeat_deadline = Instant::now() + limits.stream_heartbeat;
        }
      }
      continue;
    };

    let mut write = Box::pin(write_state_stream_record(
      writer,
      record,
      limits.write_no_progress,
    ));
    loop {
      let queued_gap_deadline = buffer.gap_deadline();
      tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
          drop(write);
          send_stream_shutting_down(writer, request_id, limits).await?;
          return Ok(ConnectionAction::Close);
        },
        _ = reader.next() => {
          drop(write);
          return Ok(ConnectionAction::Close);
        }
        result = &mut write => {
          result?;
          drop(write);
          let completed = in_flight.take().expect("active stream write retains its record");
          if matches!(
            completed.kind,
            StateStreamRecordKind::Event(_) | StateStreamRecordKind::Heartbeat
          ) {
            heartbeat_deadline = Instant::now() + limits.stream_heartbeat;
          }
          buffer.complete(&completed);
          break;
        }
        _ = wait_for_optional_deadline(queued_gap_deadline) => {
          return Ok(ConnectionAction::Close);
        }
        event = subscription.recv() => {
          if !queue_state_stream_event(
            event,
            request_id,
            &mut last_observed_sequence,
            &mut buffer,
            limits,
          )? {
            return Ok(ConnectionAction::Close);
          }
        }
        _ = sleep_until(heartbeat_deadline) => {
          buffer.enqueue_heartbeat(&StateStreamHeartbeat {
            request_id: request_id.to_string(),
            last_sequence_number: last_observed_sequence,
          })?;
          heartbeat_deadline = Instant::now() + limits.stream_heartbeat;
        }
      }
    }
  }
}

fn queue_state_stream_event(
  event: std::result::Result<StateChangedEvent, tokio::sync::broadcast::error::RecvError>,
  request_id: &str,
  last_observed_sequence: &mut u64,
  buffer: &mut StateStreamBuffer,
  limits: IpcLimits,
) -> Result<bool> {
  let gap_deadline = Instant::now() + limits.write_no_progress;
  match event {
    Ok(mut event) => {
      let expected = last_observed_sequence.saturating_add(1);
      if event.sequence_number > expected {
        buffer.record_gap(
          request_id,
          expected,
          event.sequence_number - 1,
          gap_deadline,
        )?;
      } else if event.sequence_number < expected {
        anyhow::bail!("state stream received a non-monotonic sequence number");
      }
      *last_observed_sequence = event.sequence_number;
      event.request_id = request_id.to_string();
      buffer.enqueue_event(&event, gap_deadline)?;
      Ok(true)
    }
    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
      let first = last_observed_sequence.saturating_add(1);
      let last = last_observed_sequence.saturating_add(skipped);
      *last_observed_sequence = last;
      buffer.record_gap(request_id, first, last, gap_deadline)?;
      Ok(true)
    }
    Err(tokio::sync::broadcast::error::RecvError::Closed) => Ok(false),
  }
}

async fn close_state_stream_for_client_input<W>(
  writer: &mut W,
  frame: Option<std::result::Result<String, IpcCodecError>>,
  limits: IpcLimits,
) -> Result<ConnectionAction>
where
  W: AsyncWrite + Unpin,
{
  if let ConcurrentRead::Pipelined(pipelined_request_id) = classify_concurrent_read(frame) {
    send_pipelined_error(writer, pipelined_request_id, limits).await?;
  }
  Ok(ConnectionAction::Close)
}

async fn wait_for_optional_deadline(deadline: Option<Instant>) {
  match deadline {
    Some(deadline) => sleep_until(deadline).await,
    None => std::future::pending().await,
  }
}

async fn prepare_state_subscription<W>(
  writer: &mut W,
  state: &DaemonState,
  authorized: &AuthorizedLegacyEnvelope<'_>,
  deadline: Instant,
  limits: IpcLimits,
) -> Result<
  Option<(
    String,
    u64,
    tokio::sync::broadcast::Receiver<StateChangedEvent>,
  )>,
>
where
  W: AsyncWrite + Unpin,
{
  #[cfg(test)]
  sleep(limits.dispatch_delay).await;

  let Some(request) =
    decode_or_reject_until::<SubscribeStateRequest, _>(writer, authorized, deadline, limits)
      .await?
  else {
    return Ok(None);
  };
  let request_id = request.request_id;
  let (initial, subscription) = state.subscribe_snapshot(request_id.clone()).await;
  write_envelope_until(
    writer,
    message_types::STATE_CHANGED_EVENT,
    &initial,
    deadline,
    limits.write_no_progress,
  )
  .await?;
  Ok(Some((request_id, initial.sequence_number, subscription)))
}

fn classify_concurrent_read(
  frame: Option<std::result::Result<String, IpcCodecError>>,
) -> ConcurrentRead {
  match frame {
    Some(Ok(line)) => ConcurrentRead::Pipelined(pipelined_request_id(&line)),
    Some(Err(_)) => ConcurrentRead::Pipelined(None),
    None => ConcurrentRead::Closed,
  }
}

fn pipelined_request_id(line: &str) -> Option<RequestId> {
  serde_json::from_str::<IpcEnvelope>(line)
    .ok()
    .and_then(|envelope| request_id_from_payload(&envelope))
}

async fn send_operation_timeout<W>(
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

async fn send_shutting_down<W>(
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

async fn send_request_shutting_down<W>(
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

async fn send_stream_shutting_down<W>(
  writer: &mut W,
  request_id: &str,
  limits: IpcLimits,
) -> Result<()>
where
  W: AsyncWrite + Unpin,
{
  send_shutdown_error(
    writer,
    Some(RequestId::parse(request_id).expect("active stream request ID is valid")),
    "This stream is closing because the daemon is shutting down.",
    limits,
  )
  .await
}

async fn send_pipelined_error<W>(
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

async fn dispatch_authorized_request<W>(
  writer: &mut W,
  state: &DaemonState,
  owned: &ConnectionOwnership,
  authorized: &AuthorizedLegacyEnvelope<'_>,
  envelope: &IpcEnvelope,
  dispatch: RequestDispatchContext<'_>,
) -> Result<ConnectionAction>
where
  W: AsyncWrite + Unpin,
{
  let RequestDispatchContext {
    operation_fence,
    deadline,
    limits,
  } = dispatch;
  macro_rules! send_response {
    ($message_type:expr, $response:expr) => {
      write_envelope_until(
        writer,
        $message_type,
        &$response,
        deadline,
        limits.write_no_progress,
      )
      .await?;
    };
  }
  macro_rules! decode_request {
    ($request:ty) => {
      match decode_or_reject_until::<$request, _>(writer, authorized, deadline, limits).await? {
        Some(request) => request,
        None => return Ok(ConnectionAction::Continue),
      }
    };
  }

  #[cfg(test)]
  sleep(limits.dispatch_delay).await;

  match authorized.definition().name() {
    message_types::REGISTER_ENTRYPOINT_REQUEST => {
      let request = decode_request!(RegisterEntrypointRequest);
      let fence = mutation_fence(operation_fence)?;
      let nonce = request
        .registration
        .entrypoint_instance
        .shim_session_nonce
        .clone();
      let response = state
        .register_fenced(request.request_id, request.registration, fence)
        .await?;
      if let Some(id) = response
        .registration_id
        .as_ref()
        .filter(|_| response.accepted)
      {
        owned.insert(id.clone(), nonce);
      }
      send_response!(message_types::REGISTER_ENTRYPOINT_RESPONSE, response);
    }
    message_types::UNREGISTER_ENTRYPOINT_REQUEST => {
      let request = decode_request!(UnregisterEntrypointRequest);
      let fence = mutation_fence(operation_fence)?;
      let response = state
        .unregister_fenced(
          request.request_id,
          &request.registration_id,
          &request.shim_session_nonce,
          fence,
        )
        .await?;
      if response.accepted {
        owned.remove(&request.registration_id);
      }
      send_response!(message_types::UNREGISTER_ENTRYPOINT_RESPONSE, response);
    }
    message_types::HEARTBEAT_ENTRYPOINT_REQUEST => {
      let request = decode_request!(HeartbeatEntrypointRequest);
      let response = state
        .heartbeat_fenced(request, mutation_fence(operation_fence)?)
        .await?;
      send_response!(message_types::HEARTBEAT_ENTRYPOINT_RESPONSE, response);
    }
    message_types::QUERY_STATE_REQUEST => {
      let request = decode_request!(QueryStateRequest);
      let response = state.query_state(request.request_id).await;
      send_response!(message_types::QUERY_STATE_RESPONSE, response);
    }
    message_types::SET_ENTRYPOINT_ENABLED_REQUEST => {
      let request = decode_request!(SetEntrypointEnabledRequest);
      let response = state
        .set_entrypoint_enabled_fenced(request, mutation_fence(operation_fence)?)
        .await?;
      send_response!(message_types::SET_ENTRYPOINT_ENABLED_RESPONSE, response);
    }
    message_types::SET_DOMAIN_ENABLED_REQUEST => {
      let request = decode_request!(SetDomainEnabledRequest);
      let response = state
        .set_domain_enabled_fenced(request, mutation_fence(operation_fence)?)
        .await?;
      send_response!(message_types::SET_DOMAIN_ENABLED_RESPONSE, response);
    }
    message_types::QUERY_LOGS_REQUEST => {
      let request = decode_request!(QueryLogsRequest);
      let response = state.query_logs(request).await;
      send_response!(message_types::QUERY_LOGS_RESPONSE, response);
    }
    message_types::QUERY_HISTORY_REQUEST => {
      let request = decode_request!(cadder_ipc::QueryHistoryRequest);
      let response = state.query_history(request).await;
      send_response!(message_types::QUERY_HISTORY_RESPONSE, response);
    }
    message_types::QUERY_AUTOSTART_REQUEST => {
      let request = decode_request!(QueryAutostartRequest);
      let response = state.query_autostart(request.request_id).await;
      send_response!(message_types::QUERY_AUTOSTART_RESPONSE, response);
    }
    message_types::SET_AUTOSTART_REQUEST => {
      let request = decode_request!(SetAutostartRequest);
      let response = state
        .set_autostart_fenced(request, mutation_fence(operation_fence)?)
        .await?;
      send_response!(message_types::SET_AUTOSTART_RESPONSE, response);
    }
    message_types::SUBSCRIBE_STATE_REQUEST => {
      return Err(anyhow::anyhow!(
        "state subscription reached the unary IPC dispatcher"
      ));
    }
    message_types::SHUTDOWN_DAEMON_REQUEST => {
      let request = decode_request!(ShutdownDaemonRequest);
      let response = BasicResponse {
        request_id: request.request_id,
        accepted: true,
        message: "Daemon shutdown started.".to_string(),
      };
      let shutdown_started_at = Instant::now();
      state.prepare_shutdown_at(shutdown_started_at);
      state.request_shutdown();
      #[cfg(test)]
      sleep(limits.shutdown_response_delay).await;
      let response_deadline = std::cmp::min(deadline, shutdown_started_at + limits.shutdown_accept);
      let write_result = write_envelope_until(
        writer,
        message_types::SHUTDOWN_DAEMON_RESPONSE,
        &response,
        response_deadline,
        limits.write_no_progress,
      )
      .await;
      write_result?;
      return Ok(ConnectionAction::Close);
    }
    other => {
      let response = ProtocolErrorResponse::rejected(
        request_id_from_payload(envelope),
        ProtocolError::unsupported_capability(
          format!("message-type:{other}"),
          cadder_ipc::current_capabilities(),
        ),
      );
      send_response!(message_types::PROTOCOL_ERROR_RESPONSE, response);
    }
  }

  Ok(ConnectionAction::Continue)
}

fn mutation_fence(fence: Option<&OperationFence>) -> Result<&OperationFence> {
  fence.context("mutation dispatcher requires an operation fence")
}

async fn accept_client_handshake<W>(
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

  let Some(version) = hello
    .supported_versions
    .negotiate(identity.supported_versions)
  else {
    let frame = ServerHandshakeFrame::rejected(
      hello.request_id,
      identity.runtime_id.clone(),
      identity.daemon_instance_id.clone(),
      ProtocolError::incompatible_protocol_range(
        hello.supported_versions,
        identity.supported_versions,
      ),
    );
    write_handshake_frame(writer, &frame, limits.write_no_progress).await?;
    return Ok(None);
  };

  let capabilities = OPERATION_REGISTRY
    .negotiate_capabilities(version, &hello.capabilities)?
    .into_iter()
    .filter(|capability| identity.capabilities.contains(capability))
    .collect::<Box<[_]>>();
  let frame = ServerHandshakeFrame::accepted(ServerHello {
    request_id: hello.request_id,
    runtime_id: identity.runtime_id.clone(),
    daemon_instance_id: identity.daemon_instance_id.clone(),
    selected_version: version,
    capabilities: capabilities.clone(),
  });
  write_handshake_frame(writer, &frame, limits.write_no_progress).await?;
  Ok(Some(NegotiatedSession {
    version,
    capabilities,
  }))
}

async fn read_first_frame(
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

async fn write_handshake_frame<W>(
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

async fn perform_client_handshake<W>(
  reader: &mut IpcFrameReader,
  writer: &mut W,
  paths: &RuntimePaths,
) -> IpcClientResult<NegotiatedSession>
where
  W: AsyncWrite + Unpin,
{
  let request_id =
    RequestId::parse(new_request_id("hello")).expect("generated handshake request ID is valid");
  let requested_capabilities = OPERATION_REGISTRY
    .advertised_capabilities(SUPPORTED_PROTOCOL_VERSIONS.maximum())
    .map_err(IpcClientError::daemon)?;
  let hello = ClientHello {
    request_id: request_id.clone(),
    runtime_id: paths.instance_key().into(),
    daemon_instance_id: None,
    supported_versions: SUPPORTED_PROTOCOL_VERSIONS,
    capabilities: requested_capabilities.clone(),
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
        "Reread IPC discovery and retry the connection once.",
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
    ServerHandshakeFrame::Accepted(hello) => {
      validate_server_hello(hello, paths, request_id, &requested_capabilities)
    }
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

fn validate_server_hello(
  hello: ServerHello,
  runtime: &impl HandshakeRuntime,
  request_id: RequestId,
  requested_capabilities: &[CapabilityId],
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
  if SUPPORTED_PROTOCOL_VERSIONS.negotiate(ProtocolVersionRange::exact(hello.selected_version))
    != Some(hello.selected_version)
  {
    return Err(handshake_protocol_violation(
      request_id,
      "The daemon accepted a handshake whose published protocol range is incompatible.",
    ));
  };
  let expected_capabilities = OPERATION_REGISTRY
    .negotiate_capabilities(hello.selected_version, requested_capabilities)
    .map_err(IpcClientError::daemon)?
    .into_iter()
    .collect::<Box<[_]>>();
  if hello.capabilities != expected_capabilities {
    return Err(handshake_protocol_violation(
      request_id,
      "The daemon handshake capabilities do not match the negotiated intersection.",
    ));
  }
  Ok(NegotiatedSession {
    version: hello.selected_version,
    capabilities: hello.capabilities,
  })
}

fn handshake_local_error(
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

fn stale_handshake_error(request_id: RequestId) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Discovery,
    phase: IpcClientPhase::ResponseValidate,
    code: LocalIpcErrorCode::StaleInstance,
    message: "The connected daemon does not match the instance selected from IPC discovery; no operation was sent."
      .into(),
    guidance: Some("Reread Cadder IPC discovery and retry the connection once.".into()),
    retryable: true,
    request_id: Some(request_id),
    operation: Some(CLIENT_HELLO_OPERATION.into()),
    source: None,
  })
}

fn handshake_protocol_violation(request_id: RequestId, message: &'static str) -> IpcClientError {
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

async fn authorize_or_reject<W>(
  writer: &mut W,
  state: &DaemonState,
  envelope: &IpcEnvelope,
  security: &ConnectionSecurityContext,
) -> Result<bool>
where
  W: AsyncWrite + Unpin,
{
  let operation = operation_for_message_type(&envelope.message_type);
  let decision = security.policy.evaluate(
    &security.owner_principal,
    &security.peer_principal,
    &operation,
  );
  if decision.is_allowed() {
    return Ok(true);
  }

  state.logs().append(
    LogStreamIdentity::runtime_control(),
    LogSeverity::Warn,
    format!(
      "Denied Cadder IPC {:?} operation `{}` by local security policy: {}",
      operation.kind(),
      operation.name(),
      decision.reason_code()
    ),
    LogAttributionKind::RuntimeControl,
    Some("ipc-access-denied".to_string()),
  );
  let error = ProtocolError::access_denied(
    operation.name(),
    decision.message(),
    decision.guidance().map(ToOwned::to_owned),
  );
  let response = ProtocolErrorResponse::rejected(request_id_from_payload(envelope), error);
  write_envelope(writer, message_types::PROTOCOL_ERROR_RESPONSE, &response).await?;
  Ok(false)
}

fn operation_for_message_type(message_type: &str) -> IpcOperation {
  match OPERATION_REGISTRY.lookup(message_type) {
    Some(operation) if operation.access() == OperationAccess::Mutation => {
      IpcOperation::state_changing(message_type)
    }
    _ => IpcOperation::read_only(message_type),
  }
}

#[derive(Debug, Default)]
struct ConnectionOwnershipState {
  by_registration_id: BTreeMap<String, String>,
  active_mutations: usize,
  disconnected: bool,
}

#[derive(Debug, Clone, Default)]
struct ConnectionOwnership {
  state: Arc<std::sync::Mutex<ConnectionOwnershipState>>,
  mutations_finished: Arc<tokio::sync::Notify>,
}

struct ConnectionMutationGuard(ConnectionOwnership);

impl Drop for ConnectionMutationGuard {
  fn drop(&mut self) {
    let should_notify = {
      let mut state = self
        .0
        .state
        .lock()
        .expect("connection ownership lock poisoned");
      state.active_mutations -= 1;
      state.disconnected && state.active_mutations == 0
    };
    if should_notify {
      self.0.mutations_finished.notify_one();
    }
  }
}

impl ConnectionOwnership {
  fn begin_mutation(&self) -> Option<ConnectionMutationGuard> {
    let mut state = self
      .state
      .lock()
      .expect("connection ownership lock poisoned");
    if state.disconnected {
      return None;
    }
    state.active_mutations += 1;
    Some(ConnectionMutationGuard(self.clone()))
  }

  fn insert(&self, registration_id: String, shim_session_nonce: String) {
    self
      .state
      .lock()
      .expect("connection ownership lock poisoned")
      .by_registration_id
      .insert(registration_id, shim_session_nonce);
  }

  fn remove(&self, registration_id: &str) {
    self
      .state
      .lock()
      .expect("connection ownership lock poisoned")
      .by_registration_id
      .remove(registration_id);
  }

  #[cfg(test)]
  fn take_entries(&self) -> impl Iterator<Item = (String, String)> {
    std::mem::take(
      &mut self
        .state
        .lock()
        .expect("connection ownership lock poisoned")
        .by_registration_id,
    )
    .into_iter()
  }

  #[cfg(test)]
  fn is_disconnected(&self) -> bool {
    self
      .state
      .lock()
      .expect("connection ownership lock poisoned")
      .disconnected
  }

  async fn disconnect_and_take_entries(&self) -> BTreeMap<String, String> {
    {
      let mut state = self
        .state
        .lock()
        .expect("connection ownership lock poisoned");
      state.disconnected = true;
    }
    loop {
      let mutations_finished = self.mutations_finished.notified();
      {
        let mut state = self
          .state
          .lock()
          .expect("connection ownership lock poisoned");
        if state.active_mutations == 0 {
          return std::mem::take(&mut state.by_registration_id);
        }
      }
      mutations_finished.await;
    }
  }
}

async fn write_envelope<W, T>(writer: &mut W, message_type: &str, payload: &T) -> Result<()>
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

async fn write_envelope_until<W, T>(
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

fn encode_envelope<T>(
  message_type: &str,
  payload: &T,
) -> std::result::Result<Vec<u8>, IpcCodecError>
where
  T: Serialize,
{
  encode_json_frame(&OutboundIpcEnvelope::new(message_type, payload))
}

async fn write_frame_until<W>(
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

async fn write_frame_with_deadlines<W>(
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

fn next_write_deadline(terminal_deadline: Option<Instant>, no_progress: Duration) -> Instant {
  let progress_deadline = Instant::now() + no_progress;
  terminal_deadline.map_or(progress_deadline, |deadline| {
    progress_deadline.min(deadline)
  })
}

async fn write_state_stream_record<W>(
  writer: &mut W,
  record: &EncodedStateStreamRecord,
  no_progress: Duration,
) -> io::Result<()>
where
  W: AsyncWrite + Unpin,
{
  write_frame_with_deadlines(writer, &record.frame, record.delivery_deadline, no_progress).await
}

fn ensure_write_deadline(terminal_deadline: Instant) -> io::Result<()> {
  if Instant::now() >= terminal_deadline {
    return Err(write_deadline_exceeded());
  }
  Ok(())
}

fn write_deadline_exceeded() -> io::Error {
  io::Error::new(
    io::ErrorKind::TimedOut,
    "IPC frame write did not finish before its progress or operation deadline",
  )
}

fn invalid_request_frame_error() -> ProtocolError {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OutboundIpcEnvelope<'a, T> {
  protocol_version: u16,
  capabilities: ProtocolCapabilities,
  #[serde(rename = "type")]
  message_type: &'a str,
  payload: &'a T,
}

impl<'a, T> OutboundIpcEnvelope<'a, T> {
  fn new(message_type: &'a str, payload: &'a T) -> Self {
    Self {
      protocol_version: PROTOCOL_VERSION,
      capabilities: ProtocolCapabilities::current(),
      message_type,
      payload,
    }
  }
}

async fn decode_or_reject_until<T, W>(
  writer: &mut W,
  envelope: &AuthorizedLegacyEnvelope<'_>,
  deadline: Instant,
  limits: IpcLimits,
) -> Result<Option<T>>
where
  T: DeserializeOwned,
  W: AsyncWrite + Unpin,
{
  match envelope.decode() {
    Ok(request) => Ok(Some(request)),
    Err(error) => {
      let response = ProtocolErrorResponse::rejected(envelope.request_id(), error);
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

fn request_id_from_payload(envelope: &IpcEnvelope) -> Option<RequestId> {
  envelope
    .payload
    .get("requestId")
    .and_then(|value| value.as_str())
    .and_then(|request_id| RequestId::parse(request_id).ok())
}

#[derive(Debug, Clone, Copy)]
struct IpcClientDeadlines {
  connect: Duration,
  ordinary: Duration,
  reload: Duration,
  stream: Duration,
  shutdown: Duration,
}

impl Default for IpcClientDeadlines {
  fn default() -> Self {
    Self {
      connect: Duration::from_secs(5),
      ordinary: Duration::from_secs(30),
      reload: Duration::from_secs(120),
      stream: Duration::from_secs(30),
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
      Some(OperationDeadlineClass::Stream) => self.stream,
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

  #[cfg(test)]
  fn with_deadlines(mut self, deadlines: IpcClientDeadlines) -> Self {
    self.deadlines = deadlines;
    self
  }

  pub async fn request<TRequest>(
    &self,
    message_type: &str,
    response_type: &str,
    request: &TRequest,
  ) -> IpcClientResult<TRequest::Response>
  where
    TRequest: LegacyCorrelatedRequest,
  {
    let prepared = PreparedClientRequest::new(message_type, response_type, request)?;
    let request_id = prepared.context.request_id.clone();
    let operation = prepared.context.operation.clone();
    let mut session = CadderSession::connect_with_deadlines(&self.paths, self.deadlines)
      .await
      .map_err(|error| error.with_request_context(request_id, operation))?;
    session
      .request_prepared::<TRequest::Response>(prepared)
      .await
  }

  pub async fn subscribe_state(&self, request_id: String) -> IpcClientResult<StateSubscription> {
    let prepared = PreparedClientRequest::new(
      message_types::SUBSCRIBE_STATE_REQUEST,
      message_types::STATE_CHANGED_EVENT,
      &SubscribeStateRequest { request_id },
    )?;
    let correlation = prepared.context.request_id.clone();
    let operation = prepared.context.operation.clone();
    let session = CadderSession::connect_with_deadlines(&self.paths, self.deadlines)
      .await
      .map_err(|error| error.with_request_context(correlation, operation))?;
    session.subscribe_prepared(prepared).await
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

  async fn connect_with_deadlines(
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

  pub fn negotiated_capabilities(&self) -> &[CapabilityId] {
    &self.negotiation.capabilities
  }

  pub async fn request<TRequest>(
    &mut self,
    message_type: &str,
    response_type: &str,
    request: &TRequest,
  ) -> IpcClientResult<TRequest::Response>
  where
    TRequest: LegacyCorrelatedRequest,
  {
    let prepared = PreparedClientRequest::new(message_type, response_type, request)?;
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
    let envelope = match tokio::time::timeout_at(
      deadline,
      read_client_envelope(reader, &prepared.context),
    )
    .await
    {
      Ok(result) => result?,
      Err(_) => {
        return Err(response_timeout_error(
          &prepared.context,
          operation_retryable(&prepared.context.operation),
        ));
      }
    };
    decode_client_response(envelope, &prepared.context)
  }

  pub async fn subscribe_state(self, request_id: String) -> IpcClientResult<StateSubscription> {
    let prepared = PreparedClientRequest::new(
      message_types::SUBSCRIBE_STATE_REQUEST,
      message_types::STATE_CHANGED_EVENT,
      &SubscribeStateRequest { request_id },
    )?;
    self.subscribe_prepared(prepared).await
  }

  async fn subscribe_prepared(
    mut self,
    prepared: PreparedClientRequest,
  ) -> IpcClientResult<StateSubscription> {
    let deadline = self.deadlines.response_for(&prepared.context.operation);
    let deadline = tokio::time::Instant::now() + deadline;
    let writer = self
      .writer
      .as_mut()
      .expect("new Cadder session retains its writer");
    write_prepared_request_until(writer, &prepared, deadline).await?;
    Ok(StateSubscription {
      reader: self.reader.take(),
      writer: self.writer.take(),
      context: prepared.context,
      usable: true,
    })
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

#[derive(Debug)]
pub struct StateSubscription {
  reader: Option<IpcFrameReader>,
  writer: Option<tokio::io::WriteHalf<Stream>>,
  context: ClientRequestContext,
  usable: bool,
}

impl StateSubscription {
  pub async fn next_event(&mut self) -> IpcClientResult<StateChangedEvent> {
    loop {
      match self.next_record().await? {
        StateStreamRecord::Event(event) => return Ok(*event),
        StateStreamRecord::Heartbeat(_) => {}
        StateStreamRecord::Gap(gap) => {
          let error = response_validation_error(
            &self.context,
            format!(
              "The daemon omitted state events {} through {}; refresh the authoritative state snapshot before continuing.",
              gap.first_missing_sequence_number, gap.last_missing_sequence_number
            ),
          );
          self.retire();
          return Err(error);
        }
      }
    }
  }

  pub async fn next_record(&mut self) -> IpcClientResult<StateStreamRecord> {
    if !self.usable {
      return Err(connection_no_longer_usable(&self.context));
    }
    let mut read = SubscriptionReadGuard::new(self);
    let subscription = &mut *read.subscription;
    let reader = subscription
      .reader
      .as_mut()
      .expect("usable state subscription retains its reader");
    let result = match read_client_envelope(reader, &subscription.context).await {
      Ok(envelope) => decode_state_stream_record(envelope, &subscription.context),
      Err(error) => Err(error),
    };
    if result.is_ok() {
      read.restore();
    }
    result
  }

  fn retire(&mut self) {
    self.usable = false;
    self.reader.take();
    self.writer.take();
  }
}

fn decode_state_stream_record(
  envelope: IpcEnvelope,
  request: &ClientRequestContext,
) -> IpcClientResult<StateStreamRecord> {
  validate_client_response_version(&envelope, request)?;
  if envelope.message_type == message_types::PROTOCOL_ERROR_RESPONSE {
    return decode_client_response::<StateChangedEvent>(envelope, request)
      .map(Box::new)
      .map(StateStreamRecord::Event);
  }
  validate_response_correlation(&envelope, request)?;
  match envelope.message_type.as_str() {
    message_types::STATE_CHANGED_EVENT => decode_client_payload(envelope, request)
      .map(Box::new)
      .map(StateStreamRecord::Event),
    message_types::STATE_STREAM_HEARTBEAT => {
      decode_client_payload(envelope, request).map(StateStreamRecord::Heartbeat)
    }
    message_types::STATE_STREAM_GAP => {
      decode_client_payload(envelope, request).map(StateStreamRecord::Gap)
    }
    _ => Err(response_validation_error(
      request,
      format!(
        "The daemon returned `{}` instead of a state stream record; the subscription outcome is unknown.",
        envelope.message_type
      ),
    )),
  }
}

struct SubscriptionReadGuard<'a> {
  subscription: &'a mut StateSubscription,
  restored: bool,
}

impl<'a> SubscriptionReadGuard<'a> {
  fn new(subscription: &'a mut StateSubscription) -> Self {
    subscription.usable = false;
    Self {
      subscription,
      restored: false,
    }
  }

  fn restore(&mut self) {
    self.subscription.usable = true;
    self.restored = true;
  }
}

impl Drop for SubscriptionReadGuard<'_> {
  fn drop(&mut self) {
    if !self.restored {
      self.subscription.retire();
    }
  }
}

#[derive(Debug)]
struct PreparedClientRequest {
  context: ClientRequestContext,
  frame: Box<[u8]>,
}

#[derive(Debug)]
struct ClientRequestContext {
  operation: Box<str>,
  expected_response: Box<str>,
  request_id: RequestId,
}

impl PreparedClientRequest {
  fn new<T>(operation: &str, response_type: &str, request: &T) -> IpcClientResult<Self>
  where
    T: LegacyCorrelatedRequest,
  {
    if operation != T::OPERATION {
      return Err(IpcClientError::local(LocalIpcErrorContext {
        kind: LocalIpcErrorKind::Transport,
        phase: IpcClientPhase::RequestEncode,
        code: LocalIpcErrorCode::InvalidRequest,
        message: "Cadder rejected a request paired with the wrong operation before sending it."
          .into(),
        guidance: Some("Use the operation fixed by the request's protocol type.".into()),
        retryable: false,
        request_id: request.correlation_id().ok(),
        operation: Some(operation.into()),
        source: None,
      }));
    }
    let request_id = request.correlation_id().map_err(|error| {
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
      expected_response: T::RESPONSE.into(),
      request_id,
    };
    if response_type != context.expected_response.as_ref() {
      return Err(response_contract_error(&context, response_type));
    }
    let envelope = OutboundIpcEnvelope::new(operation, request);
    let frame =
      encode_json_frame(&envelope).map_err(|error| request_frame_error(&context, error))?;
    Ok(Self {
      context,
      frame: frame.into_boxed_slice(),
    })
  }
}

async fn write_prepared_request_until<W>(
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

async fn write_prepared_request<W>(
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

async fn read_client_envelope<R>(
  reader: &mut FramedRead<R, BoundedNdjsonCodec>,
  request: &ClientRequestContext,
) -> IpcClientResult<IpcEnvelope>
where
  R: AsyncRead + Unpin,
{
  let line = match reader.next().await {
    Some(Ok(line)) => line,
    Some(Err(IpcCodecError::Io(error))) => return Err(response_read_error(request, error)),
    Some(Err(error)) => return Err(response_frame_error(request, error)),
    None => return Err(response_eof_error(request)),
  };
  serde_json::from_str(&line).map_err(|error| response_decode_error(request, error))
}

fn decode_client_response<T>(
  envelope: IpcEnvelope,
  request: &ClientRequestContext,
) -> IpcClientResult<T>
where
  T: DeserializeOwned,
{
  validate_client_response_version(&envelope, request)?;
  if envelope.message_type == message_types::PROTOCOL_ERROR_RESPONSE {
    validate_response_correlation(&envelope, request)?;
    validate_nested_protocol_error_correlation(&envelope, request)?;
    let response: ProtocolErrorResponse = decode_client_payload(envelope, request)?;
    return Err(IpcClientError::daemon(response.error));
  }
  if envelope.message_type != request.expected_response.as_ref() {
    return Err(response_validation_error(
      request,
      format!(
        "The daemon returned `{}` instead of `{}`; the operation outcome is unknown.",
        envelope.message_type, request.expected_response
      ),
    ));
  }
  validate_response_correlation(&envelope, request)?;
  decode_client_payload(envelope, request)
}

fn validate_nested_protocol_error_correlation(
  envelope: &IpcEnvelope,
  request: &ClientRequestContext,
) -> IpcClientResult<()> {
  let Some(raw_request_id) = envelope
    .payload
    .get("error")
    .and_then(|error| error.get("requestId"))
  else {
    return Ok(());
  };
  if raw_request_id.is_null() {
    return Ok(());
  }
  let Some(raw_request_id) = raw_request_id.as_str() else {
    return Err(response_validation_error(
      request,
      "The daemon error contained a non-string request ID; the operation outcome is unknown.",
    ));
  };
  let nested_request_id = RequestId::parse(raw_request_id).map_err(|error| {
    response_validation_error(
      request,
      "The daemon error contained an invalid request ID; the operation outcome is unknown.",
    )
    .with_source(Box::new(error))
  })?;
  if nested_request_id == request.request_id {
    Ok(())
  } else {
    Err(response_validation_error(
      request,
      "The daemon response and nested error belong to different requests; the operation outcome is unknown.",
    ))
  }
}

fn decode_client_payload<T>(
  envelope: IpcEnvelope,
  request: &ClientRequestContext,
) -> IpcClientResult<T>
where
  T: DeserializeOwned,
{
  serde_json::from_value(envelope.payload).map_err(|error| response_decode_error(request, error))
}

fn validate_client_response_version(
  envelope: &IpcEnvelope,
  request: &ClientRequestContext,
) -> IpcClientResult<()> {
  ensure_compatible_protocol_version(envelope.protocol_version).map_err(|error| {
    IpcClientError::local(LocalIpcErrorContext {
      kind: LocalIpcErrorKind::Transport,
      phase: IpcClientPhase::ResponseValidate,
      code: LocalIpcErrorCode::IncompatibleProtocol,
      message: "The daemon response uses an incompatible protocol version; the operation outcome is unknown."
        .into(),
      guidance: error.guidance.clone(),
      retryable: false,
      request_id: Some(request.request_id.clone()),
      operation: Some(request.operation.clone()),
      source: Some(Box::new(error)),
    })
  })
}

fn validate_response_correlation(
  envelope: &IpcEnvelope,
  request: &ClientRequestContext,
) -> IpcClientResult<()> {
  let Some(raw_request_id) = envelope
    .payload
    .get("requestId")
    .and_then(|value| value.as_str())
  else {
    return Err(response_validation_error(
      request,
      "The daemon response did not contain a request ID; the operation outcome is unknown.",
    ));
  };
  let response_request_id = RequestId::parse(raw_request_id).map_err(|error| {
    response_validation_error(
      request,
      "The daemon response contained an invalid request ID; the operation outcome is unknown.",
    )
    .with_source(Box::new(error))
  })?;
  if response_request_id == request.request_id {
    Ok(())
  } else {
    Err(response_validation_error(
      request,
      "The daemon response belongs to a different request; the operation outcome is unknown.",
    ))
  }
}

fn endpoint_resolution_error(error: io::Error) -> IpcClientError {
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

fn connection_error(error: io::Error) -> IpcClientError {
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

fn connection_timeout_error() -> IpcClientError {
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

fn peer_authentication_preface_error(error: io::Error) -> IpcClientError {
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

fn daemon_not_ready_error(kind: io::ErrorKind) -> bool {
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

fn request_frame_error(request: &ClientRequestContext, error: IpcCodecError) -> IpcClientError {
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

fn response_contract_error(request: &ClientRequestContext, response_type: &str) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Transport,
    phase: IpcClientPhase::RequestEncode,
    code: LocalIpcErrorCode::InvalidRequest,
    message: format!(
      "Cadder rejected `{response_type}` as the response contract for `{}` before sending the request.",
      request.operation
    )
    .into_boxed_str(),
    guidance: Some(
      format!(
        "Use the `{}` response fixed by the request's protocol type.",
        request.expected_response
      )
      .into_boxed_str(),
    ),
    retryable: false,
    request_id: Some(request.request_id.clone()),
    operation: Some(request.operation.clone()),
    source: None,
  })
}

fn request_write_error(request: &ClientRequestContext, error: io::Error) -> IpcClientError {
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

fn response_read_error(request: &ClientRequestContext, error: io::Error) -> IpcClientError {
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

fn response_frame_error(request: &ClientRequestContext, error: IpcCodecError) -> IpcClientError {
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

fn response_eof_error(request: &ClientRequestContext) -> IpcClientError {
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

fn response_decode_error(
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

fn response_validation_error(
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

fn request_write_timeout_error(request: &ClientRequestContext, retryable: bool) -> IpcClientError {
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

fn response_timeout_error(request: &ClientRequestContext, retryable: bool) -> IpcClientError {
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

fn connection_no_longer_usable(request: &ClientRequestContext) -> IpcClientError {
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

fn operation_retryable(operation: &str) -> bool {
  OPERATION_REGISTRY
    .lookup(operation)
    .is_some_and(|definition| definition.timeout_retryable())
}

fn daemon_launch_error(
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

fn daemon_readiness_timeout(message: &'static str) -> IpcClientError {
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

#[derive(Debug, Clone, Default)]
pub struct DaemonLaunchOptions {
  pub explicit_daemon: Option<PathBuf>,
  /// Test-only/internal runtime selection seam. Production launchers always use `None`.
  pub runtime_profile: Option<RuntimeProfile>,
  pub real_caddy_override: Option<PathBuf>,
  pub caddy_backend: Option<CaddyBackendMode>,
  pub launch_mode: DaemonLaunchMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DaemonLaunchMode {
  #[default]
  Background,
  ForegroundDiagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonStdioMode {
  Null,
  Inherit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DaemonProcessConfig {
  stdio: DaemonStdioMode,
  #[cfg(windows)]
  creation_flags: u32,
  #[cfg(unix)]
  starts_new_session: bool,
}

impl DaemonProcessConfig {
  fn for_launch_mode(mode: DaemonLaunchMode) -> Self {
    match mode {
      DaemonLaunchMode::Background => Self {
        stdio: DaemonStdioMode::Null,
        #[cfg(windows)]
        creation_flags: BACKGROUND_DAEMON_CREATION_FLAGS,
        #[cfg(unix)]
        starts_new_session: true,
      },
      DaemonLaunchMode::ForegroundDiagnostic => Self {
        stdio: DaemonStdioMode::Inherit,
        #[cfg(windows)]
        creation_flags: 0,
        #[cfg(unix)]
        starts_new_session: false,
      },
    }
  }

  fn configure_command(self, command: &mut Command) {
    #[cfg(windows)]
    if self.creation_flags != 0 {
      command.creation_flags(self.creation_flags);
    }

    #[cfg(unix)]
    if self.starts_new_session {
      // SAFETY: the closure only calls async-signal-safe `setsid` and returns
      // an `io::Result`, so it does not touch shared Rust state after fork.
      unsafe {
        command.pre_exec(start_new_unix_session);
      }
    }
  }

  fn configure_stdio(self, command: &mut Command) {
    match self.stdio {
      DaemonStdioMode::Null => {
        command
          .stdin(Stdio::null())
          .stdout(Stdio::null())
          .stderr(Stdio::null());
      }
      DaemonStdioMode::Inherit => {}
    }
  }
}

#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const BACKGROUND_DAEMON_CREATION_FLAGS: u32 =
  CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS | CREATE_NO_WINDOW;
const DAEMON_READY_ATTEMPTS: usize = 300;
const DAEMON_READY_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(unix)]
fn start_new_unix_session() -> io::Result<()> {
  unsafe extern "C" {
    fn setsid() -> i32;
  }

  // SAFETY: `setsid` has no Rust aliasing requirements and is called in the
  // child process before exec through `pre_exec`.
  if unsafe { setsid() } == -1 {
    Err(io::Error::last_os_error())
  } else {
    Ok(())
  }
}

pub async fn ensure_daemon_running(
  paths: &RuntimePaths,
  explicit_daemon: Option<PathBuf>,
) -> IpcClientResult<()> {
  ensure_daemon_running_with_options(
    paths,
    DaemonLaunchOptions {
      explicit_daemon,
      ..DaemonLaunchOptions::default()
    },
  )
  .await
}

pub async fn ensure_daemon_running_with_options(
  paths: &RuntimePaths,
  options: DaemonLaunchOptions,
) -> IpcClientResult<()> {
  if daemon_is_ready(paths).await? {
    return Ok(());
  }

  let daemon = options
    .explicit_daemon
    .or_else(|| sibling_binary("cadderd"))
    .or_else(|| find_on_path("cadderd"))
    .ok_or_else(|| {
      daemon_launch_error(
        LocalIpcErrorCode::DaemonNotFound,
        "Cadder could not find the daemon executable; no daemon was started.",
        "Install Cadder or provide a trusted cadderd path, then retry.",
        None,
      )
    })?;

  let process_config = DaemonProcessConfig::for_launch_mode(options.launch_mode);
  let caddy_backend = options
    .caddy_backend
    .map_or_else(CaddyBackendMode::from_env, Ok)
    .map_err(|error| {
      daemon_launch_error(
        LocalIpcErrorCode::InvalidInput,
        "Cadder rejected the daemon launch configuration; no daemon was started.",
        "Correct the daemon launch options, then retry.",
        Some(error.into_boxed_dyn_error()),
      )
    })?;
  if caddy_backend == CaddyBackendMode::Mock && options.real_caddy_override.is_some() {
    return Err(daemon_launch_error(
      LocalIpcErrorCode::InvalidInput,
      "Cadder rejected incompatible daemon launch options; no daemon was started.",
      "Remove either the real-Caddy override or the mock backend option, then retry.",
      None,
    ));
  }
  let daemon_dir = daemon.parent().map(PathBuf::from);
  let mut command = Command::new(&daemon);
  process_config.configure_command(&mut command);
  if let Some(daemon_dir) = &daemon_dir {
    command.current_dir(daemon_dir);
    prepend_path_dir(&mut command, daemon_dir);
  }
  command.arg("--detach-ready");
  process_config.configure_stdio(&mut command);
  if let Some(real_caddy_override) = options.real_caddy_override {
    command.arg("--real-caddy").arg(real_caddy_override);
  }
  if caddy_backend != CaddyBackendMode::Real {
    command.arg("--caddy-backend").arg(caddy_backend.as_str());
  }
  let mut child = command.spawn().map_err(|error| {
    let code = launch_code_for_io(error.kind());
    daemon_launch_error(
      code,
      "Cadder could not start the daemon; no request was sent.",
      "Check the daemon executable and runtime permissions, then retry.",
      Some(Box::new(error)),
    )
  })?;

  wait_for_daemon_ready(paths, &mut child).await
}

async fn wait_for_daemon_ready(
  paths: &RuntimePaths,
  child: &mut tokio::process::Child,
) -> IpcClientResult<()> {
  for _ in 0..DAEMON_READY_ATTEMPTS {
    if daemon_is_ready(paths).await? {
      return Ok(());
    }
    if child
      .try_wait()
      .map_err(|error| {
        let code = launch_code_for_io(error.kind());
        daemon_launch_error(
          code,
          "Cadder could not inspect the daemon launch; no request was sent.",
          "Check the daemon process and runtime permissions, then retry.",
          Some(Box::new(error)),
        )
      })?
      .is_some()
    {
      // A concurrent launcher can lose the endpoint race and exit before it
      // initializes state. Keep probing: the process that owns the endpoint
      // may become ready immediately afterwards.
    }
    sleep(DAEMON_READY_POLL_INTERVAL).await;
  }

  Err(daemon_readiness_timeout(
    "The Cadder daemon did not become ready before the local deadline; no request was sent.",
  ))
}

fn launch_code_for_io(kind: io::ErrorKind) -> LocalIpcErrorCode {
  if kind == io::ErrorKind::PermissionDenied {
    LocalIpcErrorCode::PermissionDenied
  } else {
    LocalIpcErrorCode::DaemonStartFailed
  }
}

pub(crate) async fn is_daemon_ready(paths: &RuntimePaths) -> IpcClientResult<bool> {
  daemon_is_ready(paths).await
}

#[cfg(test)]
#[derive(Debug)]
struct DaemonLaunchLock {
  _file: std::fs::File,
}

#[cfg(test)]
impl DaemonLaunchLock {
  fn try_acquire(paths: &RuntimePaths) -> Result<Option<Self>> {
    use fs4::{FileExt, TryLockError};

    let path = paths.runtime_dir().join("cadder-launch.lock");
    paths.ensure_dirs()?;
    let file = std::fs::OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .open(path)?;
    match FileExt::try_lock(&file) {
      Ok(()) => Ok(Some(Self { _file: file })),
      Err(TryLockError::WouldBlock) => Ok(None),
      Err(TryLockError::Error(error)) => Err(error.into()),
    }
  }
}

#[cfg(test)]
async fn acquire_launch_lock_or_wait_for_ready_with_policy(
  paths: &RuntimePaths,
  attempts: usize,
  poll_interval: Duration,
) -> IpcClientResult<Option<DaemonLaunchLock>> {
  for _ in 0..attempts {
    if daemon_is_ready(paths).await? {
      return Ok(None);
    }
    if let Some(lock) = DaemonLaunchLock::try_acquire(paths).map_err(|error| {
      daemon_launch_error(
        LocalIpcErrorCode::DaemonStartFailed,
        "Cadder could not coordinate a test daemon launch.",
        "Check the test runtime directory.",
        Some(error.into_boxed_dyn_error()),
      )
    })? {
      return Ok(Some(lock));
    }
    sleep(poll_interval).await;
  }
  Ok(None)
}

async fn daemon_is_ready(paths: &RuntimePaths) -> IpcClientResult<bool> {
  daemon_is_ready_with_deadlines(paths, IpcClientDeadlines::default()).await
}

async fn daemon_is_ready_with_deadlines(
  paths: &RuntimePaths,
  deadlines: IpcClientDeadlines,
) -> IpcClientResult<bool> {
  match CadderSession::connect_with_deadlines(paths, deadlines).await {
    Ok(_) => Ok(true),
    Err(error) if error.is_stale_instance() || error.is_daemon_unavailable() => Ok(false),
    Err(error) => Err(error),
  }
}

fn sibling_binary(name: &str) -> Option<PathBuf> {
  let current = env::current_exe().ok()?;
  let dir = current.parent()?;
  let candidate = dir.join(exe_name(name));
  candidate.is_file().then_some(candidate)
}

fn find_on_path(name: &str) -> Option<PathBuf> {
  let path = env::var_os("PATH")?;
  for dir in env::split_paths(&path) {
    let candidate = dir.join(exe_name(name));
    if candidate.is_file() {
      return Some(candidate);
    }
  }
  None
}

fn exe_name(name: &str) -> String {
  #[cfg(windows)]
  {
    format!("{name}.exe")
  }
  #[cfg(not(windows))]
  {
    name.to_string()
  }
}

fn prepend_path_dir(command: &mut Command, dir: &std::path::Path) {
  let paths = env::var_os("PATH")
    .map(|path| {
      std::iter::once(dir.to_path_buf())
        .chain(env::split_paths(&path))
        .collect()
    })
    .unwrap_or_else(|| vec![dir.to_path_buf()]);
  if let Ok(joined) = env::join_paths(paths) {
    command.env("PATH", joined);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    CaddyConfigCoordinator, IpcEndpoint, IpcEndpointMetadata, IpcEndpointPublication,
    PrivilegeStatus, discover_ipc_endpoint, logs::LogQuery,
    operation_fence::OperationFenceAuthority, state::RegistrationPublishTestHook,
  };
  use cadder_ipc::{
    ActivationState, AutostartMode, BasicResponse, EntrypointInstanceIdentity,
    EntrypointRegistration, IpcEnvelope, OPERATION_REGISTRY, OwnerProcessIdentity,
    ProtocolErrorCode, ProtocolErrorKind, ProtocolErrorResponse, QueryStateRequest,
    QueryStateResponse, ShimRunMetadata, SourcePath, message_types, new_request_id,
  };
  use chrono::Utc;
  use std::{env, ffi::OsString, fs, future::Future, future::pending};
  use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::{Notify, oneshot, watch},
    task::JoinHandle,
    time::{Duration, sleep, timeout},
  };

  #[test]
  fn operation_fence_classifies_every_registered_mutation_and_shutdown_exception() {
    let fenced = OPERATION_REGISTRY
      .iter()
      .filter(|definition| operation_uses_fence(definition))
      .map(|definition| definition.name())
      .collect::<Vec<_>>();
    assert_eq!(
      fenced,
      vec![
        message_types::REGISTER_ENTRYPOINT_REQUEST,
        message_types::UNREGISTER_ENTRYPOINT_REQUEST,
        message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
        message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
        message_types::SET_DOMAIN_ENABLED_REQUEST,
        message_types::SET_AUTOSTART_REQUEST,
      ]
    );
    assert!(!operation_uses_fence(
      OPERATION_REGISTRY
        .lookup(message_types::SHUTDOWN_DAEMON_REQUEST)
        .unwrap()
    ));
  }

  #[test]
  fn owned_mutation_worker_tracks_registration_lifecycle_requests() {
    assert!(owned_mutation_uses_worker(
      message_types::REGISTER_ENTRYPOINT_REQUEST
    ));
    assert!(owned_mutation_uses_worker(
      message_types::UNREGISTER_ENTRYPOINT_REQUEST
    ));
    assert!(owned_mutation_uses_worker(
      message_types::HEARTBEAT_ENTRYPOINT_REQUEST
    ));
    assert!(owned_mutation_uses_worker(
      message_types::SET_ENTRYPOINT_ENABLED_REQUEST
    ));
    assert!(owned_mutation_uses_worker(
      message_types::SET_DOMAIN_ENABLED_REQUEST
    ));
    assert!(owned_mutation_uses_worker(
      message_types::SET_AUTOSTART_REQUEST
    ));
    assert!(!owned_mutation_uses_worker(
      message_types::QUERY_STATE_REQUEST
    ));
  }

  #[tokio::test]
  async fn finalized_owned_mutation_uses_a_fresh_response_deadline() {
    let authority = OperationFenceAuthority::default();
    let fence = authority.issue().unwrap();
    fence.commit_final(|| ()).unwrap();
    let request_id = RequestId::parse("finalized-mutation-1").unwrap();
    let response = OwnedMutationResponse::Basic(BasicResponse {
      request_id: request_id.to_string(),
      accepted: true,
      message: "The domain is enabled.".to_string(),
    });
    let definition = OPERATION_REGISTRY
      .lookup(message_types::SET_DOMAIN_ENABLED_REQUEST)
      .unwrap();
    let (reader, mut writer) = tokio::io::duplex(4096);

    let wrote_response = write_owned_mutation_result(
      &mut writer,
      &response,
      OwnedMutationWriteContext {
        fence: &fence,
        request_id: Some(&request_id),
        definition,
        response_type: message_types::SET_DOMAIN_ENABLED_RESPONSE,
        deadline: Instant::now() - Duration::from_millis(1),
        limits: IpcLimits::default(),
      },
    )
    .await
    .unwrap();

    assert!(wrote_response);
    let mut line = String::new();
    BufReader::new(reader).read_line(&mut line).await.unwrap();
    let envelope: IpcEnvelope = serde_json::from_str(&line).unwrap();
    assert_eq!(
      envelope.message_type,
      message_types::SET_DOMAIN_ENABLED_RESPONSE
    );
    let decoded: BasicResponse = envelope.decode().unwrap();
    assert_eq!(decoded.request_id, request_id.as_str());
    assert!(decoded.accepted);
  }

  #[tokio::test]
  async fn expired_owned_mutation_revokes_commit_and_reports_timeout() {
    let authority = OperationFenceAuthority::default();
    let fence = authority.issue().unwrap();
    let request_id = RequestId::parse("expired-mutation-1").unwrap();
    let response = OwnedMutationResponse::Basic(BasicResponse {
      request_id: request_id.to_string(),
      accepted: true,
      message: "This response must not be published.".to_string(),
    });
    let definition = OPERATION_REGISTRY
      .lookup(message_types::SET_DOMAIN_ENABLED_REQUEST)
      .unwrap();
    let (reader, mut writer) = tokio::io::duplex(4096);

    let wrote_response = write_owned_mutation_result(
      &mut writer,
      &response,
      OwnedMutationWriteContext {
        fence: &fence,
        request_id: Some(&request_id),
        definition,
        response_type: message_types::SET_DOMAIN_ENABLED_RESPONSE,
        deadline: Instant::now() - Duration::from_millis(1),
        limits: IpcLimits::default(),
      },
    )
    .await
    .unwrap();

    assert!(!wrote_response);
    assert_eq!(fence.commit(|| ()).unwrap_err(), CommitRejection::Revoked);
    let mut line = String::new();
    BufReader::new(reader).read_line(&mut line).await.unwrap();
    let envelope: IpcEnvelope = serde_json::from_str(&line).unwrap();
    assert_eq!(
      envelope.message_type,
      message_types::PROTOCOL_ERROR_RESPONSE
    );
    let decoded: ProtocolErrorResponse = envelope.decode().unwrap();
    assert_eq!(decoded.request_id, request_id.as_str());
    assert_eq!(decoded.error.kind, ProtocolErrorKind::Timeout);
    assert_eq!(decoded.error.code.as_str(), "timeout");
  }

  #[tokio::test]
  async fn operation_fence_external_shutdown_drains_tracked_connections_before_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::with_runtime_paths(
      CaddyConfigCoordinator::new_mock(paths.clone()),
      paths.clone(),
    )
    .await
    .unwrap();
    let observed_state = state.clone();
    let (shutdown, shutdown_rx) = watch::channel(false);
    let server_paths = paths.clone();
    let server = tokio::spawn(async move {
      DaemonServer::new(server_paths, state)
        .run_until(shutdown_rx)
        .await
    });
    wait_for_ready(&paths).await;
    let mut connection = connect_authenticated(&paths).await;

    shutdown.send(true).unwrap();
    timeout(Duration::from_secs(4), server)
      .await
      .expect("server should join tracked connections during drain")
      .unwrap()
      .unwrap();

    assert_eq!(
      observed_state.issue_operation_fence().unwrap_err(),
      CommitRejection::Draining
    );
    assert!(discover_ipc_endpoint(&paths).is_err());
    let mut byte = [0_u8; 1];
    let read = timeout(Duration::from_secs(1), connection.read(&mut byte))
      .await
      .expect("idle drained connection should close without an unsolicited frame")
      .unwrap();
    assert_eq!(read, 0);
  }

  #[tokio::test]
  async fn shutdown_coordinator_closes_registry_before_waiting_for_owned_mutations() {
    let registry = MutationTaskRegistry::default();
    let release = Arc::new(Notify::new());
    let worker_release = release.clone();
    let worker = registry
      .spawn(async move {
        worker_release.notified().await;
      })
      .expect("open registry should accept owned mutations");

    registry.close();

    assert!(registry.spawn(async {}).is_none());
    assert!(
      timeout(Duration::from_millis(25), registry.wait())
        .await
        .is_err(),
      "the registry must retain a mutation accepted before close"
    );

    release.notify_one();
    timeout(Duration::from_secs(1), registry.wait())
      .await
      .expect("the registry should drain after its accepted mutation completes")
      .unwrap();
    worker.await.unwrap();
  }

  #[test]
  fn shutdown_coordinator_default_phase_budgets_total_thirty_seconds() {
    let limits = IpcLimits::default();
    let started_at = Instant::now();
    let timeline = ShutdownTimeline::new(started_at, limits);

    assert_eq!(limits.shutdown_accept, Duration::from_secs(2));
    assert_eq!(limits.shutdown_handler_grace, Duration::from_secs(8));
    assert_eq!(limits.shutdown_runtime, Duration::from_secs(10));
    assert_eq!(limits.shutdown_storage, Duration::from_secs(5));
    assert_eq!(limits.shutdown_cleanup, Duration::from_secs(5));
    assert_eq!(
      limits.shutdown_accept
        + limits.shutdown_handler_grace
        + limits.shutdown_runtime
        + limits.shutdown_storage
        + limits.shutdown_cleanup,
      Duration::from_secs(30)
    );
    assert_eq!(
      timeline.accept_deadline(),
      started_at + Duration::from_secs(2)
    );
    assert_eq!(
      timeline.handler_deadline(),
      started_at + Duration::from_secs(10)
    );
    assert_eq!(
      timeline.runtime_deadline(started_at + Duration::from_secs(15)),
      started_at + Duration::from_secs(20)
    );
    assert_eq!(
      timeline.storage_deadline(started_at + Duration::from_secs(23)),
      started_at + Duration::from_secs(25)
    );
    assert_eq!(
      timeline.cleanup_deadline(started_at + Duration::from_secs(29)),
      started_at + Duration::from_secs(30)
    );
  }

  #[tokio::test]
  async fn shutdown_coordinator_reports_a_detached_owned_mutation_panic() {
    let registry = MutationTaskRegistry::default();
    let worker = registry
      .spawn(async {
        panic!("owned-mutation-panic");
      })
      .expect("open registry should accept owned mutations");
    drop(worker);
    timeout(Duration::from_secs(1), registry.panic_detected())
      .await
      .expect("the registry should signal an owned mutation panic immediately");
    registry.close();

    let error = timeout(Duration::from_secs(1), registry.wait())
      .await
      .expect("the registry should observe the detached task")
      .unwrap_err();

    assert!(error.to_string().contains("owned-mutation-panic"));
  }

  #[tokio::test]
  #[ignore = "discovery publication is removed"]
  async fn shutdown_coordinator_keeps_discovery_until_a_revoked_mutation_rolls_back() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let mut state = DaemonState::with_runtime_paths(
      CaddyConfigCoordinator::new_mock(paths.clone()),
      paths.clone(),
    )
    .await
    .unwrap();
    let hook = RegistrationPublishTestHook::new();
    state.set_registration_publish_hook(hook.clone());
    let observed_state = state.clone();
    let limits = IpcLimits {
      shutdown_accept: Duration::from_millis(25),
      shutdown_handler_grace: Duration::from_millis(50),
      ..IpcLimits::default()
    };
    let (shutdown, shutdown_rx) = watch::channel(false);
    let server_paths = paths.clone();
    let server = tokio::spawn(async move {
      DaemonServer::new(server_paths, state)
        .with_limits(limits)
        .run_until(shutdown_rx)
        .await
    });
    wait_for_ready(&paths).await;
    let mut connection = connect_authenticated(&paths).await;
    write_envelope(
      &mut connection,
      message_types::REGISTER_ENTRYPOINT_REQUEST,
      &RegisterEntrypointRequest {
        request_id: "shutdown-contained-registration".to_string(),
        registration: shutdown_test_registration(),
      },
    )
    .await
    .unwrap();
    timeout(Duration::from_secs(2), hook.wait_until_reached())
      .await
      .expect("registration should reach the pre-publish containment hook");

    shutdown.send(true).unwrap();
    sleep(Duration::from_millis(100)).await;

    assert!(!server.is_finished());
    assert!(discover_ipc_endpoint(&paths).is_ok());

    hook.release();
    let error = timeout(Duration::from_secs(2), server)
      .await
      .expect("server should finish after the contained mutation rolls back")
      .unwrap()
      .unwrap_err();
    let snapshot = observed_state.snapshot().await;
    let history = observed_state
      .query_history(cadder_ipc::QueryHistoryRequest {
        request_id: "shutdown-contained-history".to_string(),
        kind: None,
        limit: Some(10),
      })
      .await;

    assert!(
      error
        .to_string()
        .contains("drain local IPC connection tasks")
    );
    assert!(discover_ipc_endpoint(&paths).is_err());
    assert!(snapshot.registrations.is_empty());
    assert!(history.records.iter().all(|record| {
      record.registration_id.as_deref() != Some("shutdown-contained-entrypoint")
    }));
  }

  fn shutdown_test_registration() -> EntrypointRegistration {
    let now = Utc::now();
    EntrypointRegistration {
      registration_id: "shutdown-contained-entrypoint".to_string(),
      entrypoint_instance: EntrypointInstanceIdentity {
        instance_id: "shutdown-contained-entrypoint".to_string(),
        started_at_utc: now,
        shim_session_nonce: "shutdown-contained-nonce".to_string(),
      },
      source_working_directory: SourcePath::new(".", None),
      source_config_path: SourcePath::new("Caddyfile", None),
      registered_domains: Vec::new(),
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 1,
        process_start_time_utc: now,
        shim_session_nonce: "shutdown-contained-nonce".to_string(),
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint("shutdown-contained-entrypoint"),
      shim_run: Some(ShimRunMetadata {
        adapter: Some("caddyfile".to_string()),
        raw_arguments: vec!["run".to_string()],
        command_line: "run".to_string(),
      }),
      created_at_utc: now,
      last_heartbeat_utc: now,
    }
  }

  #[tokio::test]
  async fn shutdown_coordinator_cancels_slow_request_with_typed_terminal_error() {
    let limits = IpcLimits {
      shutdown_handler_grace: Duration::from_millis(50),
      dispatch_delay: Duration::from_secs(5),
      ..IpcLimits::default()
    };
    let daemon = RunningTestDaemon::start_with_limits(limits).await;
    let mut connection = connect_authenticated(&daemon.paths).await;
    let request_id = "shutdown-coordinator-slow-request";
    write_envelope(
      &mut connection,
      message_types::QUERY_STATE_REQUEST,
      &QueryStateRequest {
        request_id: request_id.to_string(),
      },
    )
    .await
    .unwrap();
    sleep(Duration::from_millis(50)).await;

    daemon.shutdown.send(true).unwrap();

    let envelope = timeout(Duration::from_secs(1), read_raw_envelope(&mut connection))
      .await
      .expect("slow request should receive a bounded shutdown outcome");
    let response: ProtocolErrorResponse = envelope.decode().unwrap();
    assert_eq!(response.request_id, request_id);
    assert_eq!(response.error.kind, ProtocolErrorKind::ShuttingDown);
    assert_eq!(response.error.code.as_str(), "shutting_down");
    assert_eq!(
      response.error.message.as_ref(),
      "This request did not complete because the daemon is shutting down."
    );
    assert!(!response.error.retryable);
    timeout(Duration::from_secs(1), daemon.task)
      .await
      .expect("shutdown coordinator should join the slow handler")
      .unwrap()
      .unwrap();
  }

  #[tokio::test]
  async fn shutdown_coordinator_joins_a_stalled_owned_mutation_worker() {
    let limits = IpcLimits {
      shutdown_accept: Duration::from_millis(25),
      shutdown_handler_grace: Duration::from_millis(50),
      dispatch_delay: Duration::from_secs(5),
      ..IpcLimits::default()
    };
    let daemon = RunningTestDaemon::start_with_limits(limits).await;
    let mut connection = connect_authenticated(&daemon.paths).await;
    let request_id = "shutdown-coordinator-owned-mutation";
    write_envelope(
      &mut connection,
      message_types::SET_AUTOSTART_REQUEST,
      &SetAutostartRequest {
        request_id: request_id.to_string(),
        mode: cadder_ipc::AutostartMode::Daemon,
      },
    )
    .await
    .unwrap();
    sleep(Duration::from_millis(50)).await;

    daemon.shutdown.send(true).unwrap();

    let envelope = timeout(Duration::from_secs(1), read_raw_envelope(&mut connection))
      .await
      .expect("owned mutation should receive a bounded shutdown outcome");
    let response: ProtocolErrorResponse = envelope.decode().unwrap();
    assert_eq!(response.request_id, request_id);
    assert_eq!(response.error.kind, ProtocolErrorKind::ShuttingDown);
    assert_eq!(
      response.error.message.as_ref(),
      "This request did not complete because the daemon is shutting down."
    );
    assert!(!response.error.retryable);
    timeout(Duration::from_secs(1), daemon.task)
      .await
      .expect("shutdown coordinator should join the owned mutation worker")
      .unwrap()
      .unwrap();
  }

  #[tokio::test]
  async fn shutdown_coordinator_rejects_a_new_request_on_an_authenticated_connection() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::with_runtime_paths(
      CaddyConfigCoordinator::new_mock(paths.clone()),
      paths.clone(),
    )
    .await
    .unwrap();
    let observed_state = state.clone();
    let limits = IpcLimits {
      shutdown_accept: Duration::from_millis(500),
      shutdown_handler_grace: Duration::from_millis(100),
      ..IpcLimits::default()
    };
    let (shutdown, shutdown_rx) = watch::channel(false);
    let server_paths = paths.clone();
    let server = tokio::spawn(async move {
      DaemonServer::new(server_paths, state)
        .with_limits(limits)
        .run_until(shutdown_rx)
        .await
    });
    wait_for_ready(&paths).await;
    let mut connection = connect_authenticated(&paths).await;

    shutdown.send(true).unwrap();
    timeout(Duration::from_secs(1), async {
      loop {
        if matches!(
          observed_state.issue_operation_fence(),
          Err(CommitRejection::Draining)
        ) {
          break;
        }
        tokio::task::yield_now().await;
      }
    })
    .await
    .expect("shutdown coordinator should enter operation drain");

    let request_id = "shutdown-coordinator-new-request";
    write_envelope(
      &mut connection,
      message_types::QUERY_STATE_REQUEST,
      &QueryStateRequest {
        request_id: request_id.to_string(),
      },
    )
    .await
    .unwrap();
    let envelope = timeout(Duration::from_secs(1), read_raw_envelope(&mut connection))
      .await
      .expect("the draining connection should receive a terminal response");
    assert_eq!(
      envelope.message_type,
      message_types::PROTOCOL_ERROR_RESPONSE
    );
    let response: ProtocolErrorResponse = envelope.decode().unwrap();

    assert_eq!(response.request_id, request_id);
    assert_eq!(response.error.kind, ProtocolErrorKind::ShuttingDown);
    assert_eq!(response.error.code.as_str(), "shutting_down");
    assert!(!response.error.retryable);
    timeout(Duration::from_secs(2), server)
      .await
      .expect("shutdown coordinator should finish within its phase budgets")
      .unwrap()
      .unwrap();
  }

  #[tokio::test]
  async fn shutdown_coordinator_starts_drain_before_a_slow_shutdown_ack() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::with_runtime_paths(
      CaddyConfigCoordinator::new_mock(paths.clone()),
      paths.clone(),
    )
    .await
    .unwrap();
    let observed_state = state.clone();
    let limits = IpcLimits {
      shutdown_accept: Duration::from_millis(400),
      shutdown_handler_grace: Duration::from_millis(400),
      shutdown_connection_abort_join: Duration::from_millis(100),
      shutdown_response_delay: Duration::from_millis(250),
      ..IpcLimits::default()
    };
    let (_external_shutdown, shutdown_rx) = watch::channel(false);
    let server_paths = paths.clone();
    let server = tokio::spawn(async move {
      DaemonServer::new(server_paths, state)
        .with_limits(limits)
        .run_until(shutdown_rx)
        .await
    });
    wait_for_ready(&paths).await;
    let mut shutdown_connection = connect_authenticated(&paths).await;
    let _idle_connection = connect_authenticated(&paths).await;
    let mut probe_connection = connect_authenticated(&paths).await;
    let started_at = Instant::now();

    write_envelope(
      &mut shutdown_connection,
      message_types::SHUTDOWN_DAEMON_REQUEST,
      &ShutdownDaemonRequest {
        request_id: "shutdown-coordinator-slow-ack".to_string(),
      },
    )
    .await
    .unwrap();
    timeout(Duration::from_millis(300), async {
      loop {
        if matches!(
          observed_state.issue_operation_fence(),
          Err(CommitRejection::Draining)
        ) {
          break;
        }
        tokio::task::yield_now().await;
      }
    })
    .await
    .expect("the shutdown request should enter drain before its ACK completes");

    let request_id = "shutdown-coordinator-during-slow-ack";
    write_envelope(
      &mut probe_connection,
      message_types::QUERY_STATE_REQUEST,
      &QueryStateRequest {
        request_id: request_id.to_string(),
      },
    )
    .await
    .unwrap();
    let envelope = timeout(
      Duration::from_millis(300),
      read_raw_envelope(&mut probe_connection),
    )
    .await
    .expect("a peer should be rejected while the shutdown ACK is pending");
    let response: ProtocolErrorResponse = envelope.decode().unwrap();
    assert_eq!(response.request_id, request_id);
    assert_eq!(response.error.kind, ProtocolErrorKind::ShuttingDown);

    let shutdown_envelope = timeout(
      Duration::from_millis(400),
      read_raw_envelope(&mut shutdown_connection),
    )
    .await
    .expect("the shutdown requester should receive its ACK within the accept phase");
    assert_eq!(
      shutdown_envelope.message_type,
      message_types::SHUTDOWN_DAEMON_RESPONSE
    );
    let shutdown_response: BasicResponse = shutdown_envelope.decode().unwrap();
    assert!(shutdown_response.accepted);

    timeout(Duration::from_millis(900), server)
      .await
      .expect("the slow ACK must not reset the absolute shutdown timeline")
      .unwrap()
      .unwrap();
    assert!(started_at.elapsed() < Duration::from_millis(900));
  }

  #[tokio::test]
  async fn shutdown_coordinator_does_not_reset_the_timeline_after_an_expired_ack() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::with_runtime_paths(
      CaddyConfigCoordinator::new_mock(paths.clone()),
      paths.clone(),
    )
    .await
    .unwrap();
    let limits = IpcLimits {
      shutdown_accept: Duration::from_millis(400),
      shutdown_handler_grace: Duration::from_millis(400),
      shutdown_connection_abort_join: Duration::from_millis(100),
      shutdown_response_delay: Duration::from_millis(600),
      ..IpcLimits::default()
    };
    let (_external_shutdown, shutdown_rx) = watch::channel(false);
    let server_paths = paths.clone();
    let server = tokio::spawn(async move {
      DaemonServer::new(server_paths, state)
        .with_limits(limits)
        .run_until(shutdown_rx)
        .await
    });
    wait_for_ready(&paths).await;
    let mut shutdown_connection = connect_authenticated(&paths).await;
    let _idle_connection = connect_authenticated(&paths).await;
    let started_at = Instant::now();

    write_envelope(
      &mut shutdown_connection,
      message_types::SHUTDOWN_DAEMON_REQUEST,
      &ShutdownDaemonRequest {
        request_id: "shutdown-coordinator-expired-ack".to_string(),
      },
    )
    .await
    .unwrap();
    timeout(Duration::from_millis(900), server)
      .await
      .expect("an expired ACK must not reset the absolute shutdown timeline")
      .unwrap()
      .unwrap();
    assert!(started_at.elapsed() < Duration::from_millis(900));

    let mut byte = [0_u8; 1];
    let read = timeout(
      Duration::from_millis(100),
      shutdown_connection.read(&mut byte),
    )
    .await
    .expect("the expired ACK connection should close")
    .unwrap();
    assert_eq!(read, 0);
  }

  #[tokio::test]
  async fn shutdown_coordinator_cancels_slow_stream_setup_with_typed_error() {
    let limits = IpcLimits {
      shutdown_handler_grace: Duration::from_millis(50),
      dispatch_delay: Duration::from_secs(5),
      ..IpcLimits::default()
    };
    let daemon = RunningTestDaemon::start_with_limits(limits).await;
    let mut connection = connect_authenticated(&daemon.paths).await;
    let request_id = "shutdown-coordinator-stream-setup";
    write_envelope(
      &mut connection,
      message_types::SUBSCRIBE_STATE_REQUEST,
      &SubscribeStateRequest {
        request_id: request_id.to_string(),
      },
    )
    .await
    .unwrap();
    sleep(Duration::from_millis(50)).await;

    daemon.shutdown.send(true).unwrap();

    let envelope = timeout(Duration::from_secs(1), read_raw_envelope(&mut connection))
      .await
      .expect("stream setup should receive a bounded shutdown outcome");
    let response: ProtocolErrorResponse = envelope.decode().unwrap();
    assert_eq!(response.request_id, request_id);
    assert_eq!(response.error.kind, ProtocolErrorKind::ShuttingDown);
    assert_eq!(
      response.error.message.as_ref(),
      "This request did not complete because the daemon is shutting down."
    );
    assert!(!response.error.retryable);
    timeout(Duration::from_secs(1), daemon.task)
      .await
      .expect("shutdown coordinator should join stream setup")
      .unwrap()
      .unwrap();
  }

  #[tokio::test]
  async fn shutdown_signal_preserves_the_prepared_start_time() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state =
      DaemonState::with_runtime_paths(CaddyConfigCoordinator::new_mock(paths.clone()), paths)
        .await
        .unwrap();
    let shutdown = state.shutdown_signal();
    let started_at = Instant::now();

    state.prepare_shutdown_at(started_at);
    state.request_shutdown();

    timeout(Duration::from_secs(1), shutdown.wait())
      .await
      .expect("a prepared shutdown must wake the server");
    assert_eq!(shutdown.started_at(), Some(started_at));
  }

  #[tokio::test]
  async fn write_envelope_serializes_newline_delimited_json() {
    let (mut reader, mut writer) = tokio::io::duplex(1024);
    let payload = QueryStateRequest {
      request_id: "state-1".to_string(),
    };

    write_envelope(&mut writer, message_types::QUERY_STATE_REQUEST, &payload)
      .await
      .unwrap();
    drop(writer);

    let mut rendered = String::new();
    reader.read_to_string(&mut rendered).await.unwrap();
    let envelope: IpcEnvelope = serde_json::from_str(rendered.trim_end()).unwrap();
    let decoded: QueryStateRequest = envelope.decode().unwrap();

    assert!(rendered.ends_with('\n'));
    assert_eq!(envelope.message_type, message_types::QUERY_STATE_REQUEST);
    assert_eq!(decoded.request_id, "state-1");
  }

  #[tokio::test]
  async fn ipc_limits_frame_write_allows_continuous_progress_past_one_progress_window() {
    let (mut reader, mut writer) = tokio::io::duplex(2);
    let expected = b"continuous-progress";
    let reader_task = tokio::spawn(async move {
      let mut received = Vec::new();
      let mut chunk = [0_u8; 2];
      while received.len() < expected.len() {
        let count = reader.read(&mut chunk).await.unwrap();
        received.extend_from_slice(&chunk[..count]);
        sleep(Duration::from_millis(25)).await;
      }
      received
    });

    write_frame_until(
      &mut writer,
      expected,
      Instant::now() + Duration::from_secs(1),
      Duration::from_millis(60),
    )
    .await
    .unwrap();

    assert_eq!(reader_task.await.unwrap(), expected);
  }

  #[tokio::test]
  async fn ipc_limits_frame_write_fails_after_no_progress_deadline() {
    let (_reader, mut writer) = tokio::io::duplex(1);

    let error = write_frame_until(
      &mut writer,
      b"blocked",
      Instant::now() + Duration::from_secs(1),
      Duration::from_millis(50),
    )
    .await
    .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
  }

  #[tokio::test]
  async fn ipc_limits_frame_write_rejects_an_elapsed_terminal_deadline() {
    let (mut reader, mut writer) = tokio::io::duplex(64);

    let error = write_frame_until(
      &mut writer,
      b"expired",
      Instant::now(),
      Duration::from_secs(1),
    )
    .await
    .unwrap_err();
    drop(writer);
    let mut written = Vec::new();
    reader.read_to_end(&mut written).await.unwrap();

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(written.is_empty());
  }

  #[tokio::test]
  async fn stream_limits_queue_replaces_record_overflow_with_one_gap() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths));
    let snapshot = state.snapshot().await;
    let mut buffer = StateStreamBuffer::new(3, 512 * 1024);
    let deadline = Instant::now() + Duration::from_secs(5);

    for sequence_number in 1..=3 {
      buffer
        .enqueue_event(
          &StateChangedEvent {
            request_id: "stream-limits-records".to_string(),
            sequence_number,
            change_kind: cadder_ipc::StateChangeKind::RuntimeChanged,
            snapshot: snapshot.clone(),
            registration_id: None,
          },
          deadline,
        )
        .unwrap();
    }

    assert_eq!(buffer.total_records, 1);
    assert!(buffer.total_bytes <= buffer.max_bytes);
    assert!(matches!(
      buffer.records.front().map(|record| record.kind),
      Some(StateStreamRecordKind::Gap { first: 1, last: 3 })
    ));
  }

  #[test]
  fn stream_limits_defaults_match_the_ipc_contract() {
    let limits = IpcLimits::default();

    assert_eq!(limits.stream_max_records, 256);
    assert_eq!(limits.stream_max_bytes, 524_288);
    assert_eq!(limits.stream_heartbeat, Duration::from_secs(15));
    assert_eq!(limits.write_no_progress, Duration::from_secs(5));
  }

  #[tokio::test]
  async fn stream_limits_queue_replaces_byte_overflow_with_a_gap() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths));
    let event = StateChangedEvent {
      request_id: "stream-limits-bytes".to_string(),
      sequence_number: 1,
      change_kind: cadder_ipc::StateChangeKind::RuntimeChanged,
      snapshot: state.snapshot().await,
      registration_id: None,
    };
    let event_size = EncodedStateStreamRecord::event(&event).unwrap().frame.len();
    let mut buffer = StateStreamBuffer::new(256, event_size + STREAM_CONTROL_RESERVE_BYTES - 1);

    buffer
      .enqueue_event(&event, Instant::now() + Duration::from_secs(5))
      .unwrap();

    assert_eq!(buffer.total_records, 1);
    assert!(buffer.total_bytes <= buffer.max_bytes);
    assert!(matches!(
      buffer.records.front().map(|record| record.kind),
      Some(StateStreamRecordKind::Gap { first: 1, last: 1 })
    ));
  }

  #[tokio::test]
  async fn stream_limits_queue_counts_in_flight_and_keeps_the_first_gap_deadline() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths));
    let snapshot = state.snapshot().await;
    let event = |sequence_number| StateChangedEvent {
      request_id: "stream-limits-in-flight".to_string(),
      sequence_number,
      change_kind: cadder_ipc::StateChangeKind::RuntimeChanged,
      snapshot: snapshot.clone(),
      registration_id: None,
    };
    let mut buffer = StateStreamBuffer::new(3, 512 * 1024);
    let first_deadline = Instant::now() + Duration::from_secs(5);

    buffer.enqueue_event(&event(1), first_deadline).unwrap();
    let in_flight = buffer.take_next().unwrap();
    buffer.enqueue_event(&event(2), first_deadline).unwrap();
    buffer.enqueue_event(&event(3), first_deadline).unwrap();
    buffer
      .enqueue_event(&event(4), first_deadline + Duration::from_secs(3))
      .unwrap();

    assert_eq!(buffer.total_records, 2);
    assert_eq!(buffer.gap_deadline(), Some(first_deadline));
    assert!(matches!(
      buffer.records.front().map(|record| record.kind),
      Some(StateStreamRecordKind::Gap { first: 2, last: 4 })
    ));
    buffer.complete(&in_flight);
    assert_eq!(buffer.total_records, 1);
  }

  #[test]
  fn stream_limits_broadcast_lag_becomes_an_explicit_gap() {
    let mut buffer = StateStreamBuffer::new(256, 512 * 1024);
    let mut last_observed_sequence = 7;

    assert!(
      queue_state_stream_event(
        Err(tokio::sync::broadcast::error::RecvError::Lagged(3)),
        "stream-limits-lagged",
        &mut last_observed_sequence,
        &mut buffer,
        IpcLimits::default(),
      )
      .unwrap()
    );

    assert_eq!(last_observed_sequence, 10);
    assert!(matches!(
      buffer.records.front().map(|record| record.kind),
      Some(StateStreamRecordKind::Gap { first: 8, last: 10 })
    ));
  }

  #[tokio::test]
  async fn stream_limits_snapshot_and_receiver_share_one_sequence_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths));
    let (snapshot, mut receiver) = state
      .subscribe_snapshot("stream-limits-snapshot".to_string())
      .await;

    state.publish_test_change().await;
    let event = receiver.recv().await.unwrap();

    assert_eq!(event.sequence_number, snapshot.sequence_number + 1);
  }

  #[tokio::test]
  async fn stream_limits_writer_closes_after_five_seconds_without_progress() {
    let (_reader, mut writer) = tokio::io::duplex(1);
    let record = EncodedStateStreamRecord::heartbeat(&StateStreamHeartbeat {
      request_id: "stream-limits-stalled".to_string(),
      last_sequence_number: 0,
    })
    .unwrap();

    let error = write_state_stream_record(&mut writer, &record, Duration::from_millis(50))
      .await
      .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
  }

  #[test]
  fn stream_limits_client_decodes_control_records_and_validates_correlation() {
    let request_id = RequestId::parse("stream-limits-client").unwrap();
    let context = ClientRequestContext {
      operation: message_types::SUBSCRIBE_STATE_REQUEST.into(),
      expected_response: message_types::STATE_CHANGED_EVENT.into(),
      request_id: request_id.clone(),
    };
    let heartbeat = StateStreamHeartbeat {
      request_id: request_id.to_string(),
      last_sequence_number: 7,
    };
    let heartbeat_envelope =
      IpcEnvelope::new(message_types::STATE_STREAM_HEARTBEAT, &heartbeat).unwrap();
    assert_eq!(
      decode_state_stream_record(heartbeat_envelope, &context).unwrap(),
      StateStreamRecord::Heartbeat(heartbeat)
    );

    let gap = StateStreamGap {
      request_id: request_id.to_string(),
      first_missing_sequence_number: 8,
      last_missing_sequence_number: 11,
    };
    let gap_envelope = IpcEnvelope::new(message_types::STATE_STREAM_GAP, &gap).unwrap();
    assert_eq!(
      decode_state_stream_record(gap_envelope, &context).unwrap(),
      StateStreamRecord::Gap(gap)
    );

    let wrong_correlation = IpcEnvelope::new(
      message_types::STATE_STREAM_HEARTBEAT,
      &StateStreamHeartbeat {
        request_id: "another-request".to_string(),
        last_sequence_number: 7,
      },
    )
    .unwrap();
    assert!(decode_state_stream_record(wrong_correlation, &context).is_err());
  }

  #[tokio::test]
  async fn stream_limits_legacy_next_event_retires_after_a_gap() {
    let request_id = "stream-limits-legacy-gap";
    let server = ScriptedIpcServer::start(move |conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_envelope(
        &mut writer,
        message_types::STATE_STREAM_GAP,
        &StateStreamGap {
          request_id: request_id.to_string(),
          first_missing_sequence_number: 8,
          last_missing_sequence_number: 11,
        },
      )
      .await
      .unwrap();
    });
    let mut subscription = CadderSession::connect(&server.paths)
      .await
      .unwrap()
      .subscribe_state(request_id.to_string())
      .await
      .unwrap();

    let gap_error = subscription.next_event().await.unwrap_err();
    assert_local_error(
      &gap_error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseValidate,
      "protocol_violation",
      Some(request_id),
    );
    let terminal_error = subscription.next_record().await.unwrap_err();
    assert_local_error(
      &terminal_error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::Connect,
      "connection_closed",
      Some(request_id),
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn stream_limits_idle_subscription_emits_heartbeat() {
    let daemon = RunningTestDaemon::start_with_limits(IpcLimits {
      stream_heartbeat: Duration::from_millis(25),
      ..IpcLimits::default()
    })
    .await;
    let mut subscription = CadderSession::connect(&daemon.paths)
      .await
      .unwrap()
      .subscribe_state("stream-limits-heartbeat".to_string())
      .await
      .unwrap();

    let initial = subscription.next_record().await.unwrap();
    let initial_sequence = match initial {
      StateStreamRecord::Event(event) => event.sequence_number,
      record => panic!("expected initial state event, got {record:?}"),
    };
    let heartbeat = timeout(Duration::from_secs(1), subscription.next_record())
      .await
      .unwrap()
      .unwrap();

    assert_eq!(
      heartbeat,
      StateStreamRecord::Heartbeat(StateStreamHeartbeat {
        request_id: "stream-limits-heartbeat".to_string(),
        last_sequence_number: initial_sequence,
      })
    );
    drop(subscription);
    daemon.stop().await;
  }

  #[tokio::test]
  async fn shutdown_coordinator_closes_an_active_subscription_with_a_terminal_outcome() {
    let daemon = RunningTestDaemon::start().await;
    let mut subscription = CadderSession::connect(&daemon.paths)
      .await
      .unwrap()
      .subscribe_state("stream-limits-cancellation".to_string())
      .await
      .unwrap();
    assert!(matches!(
      subscription.next_record().await.unwrap(),
      StateStreamRecord::Event(_)
    ));

    daemon.shutdown.send(true).unwrap();
    let error = timeout(Duration::from_secs(1), subscription.next_record())
      .await
      .expect("idle stream cancellation should be observed within one second")
      .unwrap_err();

    assert_daemon_error(
      &error,
      ProtocolErrorKind::ShuttingDown,
      "shutting_down",
      "stream-limits-cancellation",
      "This stream is closing because the daemon is shutting down.",
      Some("Wait for the daemon to stop, then start it before retrying the operation."),
      false,
    );
    timeout(Duration::from_secs(2), daemon.task)
      .await
      .unwrap()
      .unwrap()
      .unwrap();
  }

  #[tokio::test]
  async fn ipc_codec_writer_rejects_oversized_envelope_before_writing() {
    #[derive(Serialize)]
    struct OversizedPayload {
      request_id: String,
      value: String,
    }

    let (mut reader, mut writer) = tokio::io::duplex(64);
    let error = write_envelope(
      &mut writer,
      "test.oversized",
      &OversizedPayload {
        request_id: "oversized-write-1".to_string(),
        value: "x".repeat(crate::ipc_codec::MAX_IPC_FRAME_LENGTH),
      },
    )
    .await
    .unwrap_err();
    drop(writer);

    assert!(error.downcast_ref::<IpcCodecError>().is_some());
    let mut written = Vec::new();
    reader.read_to_end(&mut written).await.unwrap();
    assert!(written.is_empty());
  }

  #[tokio::test]
  async fn ipc_codec_client_rejects_oversized_response_and_keeps_correlation() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      writer
        .write_all(&vec![b'x'; crate::ipc_codec::MAX_IPC_FRAME_LENGTH + 1])
        .await
        .unwrap();
      writer.write_all(b"\n").await.unwrap();
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "oversized-response-1".to_string(),
        },
      )
      .await
      .unwrap_err();
    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseRead,
      "frame",
      Some("oversized-response-1"),
    );
    assert!(!error.retryable());

    let retired = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "oversized-response-retry-1".to_string(),
        },
      )
      .await
      .unwrap_err();
    assert_local_error(
      &retired,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::Connect,
      "connection_closed",
      Some("oversized-response-retry-1"),
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn ipc_codec_daemon_dispatches_an_exact_limit_request() {
    let daemon = RunningTestDaemon::start().await;
    let mut conn = connect_authenticated(&daemon.paths).await;
    let mut envelope = IpcEnvelope::new(
      message_types::QUERY_STATE_REQUEST,
      &QueryStateRequest {
        request_id: "exact-limit-request-1".to_string(),
      },
    )
    .unwrap();
    envelope.payload.as_object_mut().unwrap().insert(
      "padding".to_string(),
      serde_json::Value::String(String::new()),
    );
    let baseline = serde_json::to_vec(&envelope).unwrap();
    let padding_length = crate::ipc_codec::MAX_IPC_FRAME_LENGTH - baseline.len();
    envelope.payload["padding"] = serde_json::Value::String("x".repeat(padding_length));
    let mut frame = serde_json::to_vec(&envelope).unwrap();
    assert_eq!(frame.len(), crate::ipc_codec::MAX_IPC_FRAME_LENGTH);
    frame.push(b'\n');

    conn.write_all(&frame).await.unwrap();
    let response = read_raw_envelope(&mut conn).await;
    let response: QueryStateResponse = response.decode().unwrap();

    assert!(response.accepted);
    assert_eq!(response.request_id, "exact-limit-request-1");
    daemon.stop().await;
  }

  #[tokio::test]
  async fn ipc_codec_daemon_returns_typed_errors_and_closes_invalid_frames() {
    let daemon = RunningTestDaemon::start().await;

    let mut oversized = connect_authenticated(&daemon.paths).await;
    oversized
      .write_all(&vec![b'x'; crate::ipc_codec::MAX_IPC_FRAME_LENGTH + 1])
      .await
      .unwrap();
    oversized.write_all(b"\n").await.unwrap();
    assert_typed_frame_error_then_eof(oversized).await;

    let mut invalid_utf8 = connect_authenticated(&daemon.paths).await;
    invalid_utf8.write_all(&[0xff, b'\n']).await.unwrap();
    assert_typed_frame_error_then_eof(invalid_utf8).await;

    daemon.stop().await;
  }

  #[tokio::test]
  async fn discovery_handshake_matching_instance_negotiates_and_dispatches() {
    let daemon = RunningTestDaemon::start().await;
    let mut session = CadderSession::connect(&daemon.paths).await.unwrap();

    assert_eq!(
      session.negotiated_version(),
      SUPPORTED_PROTOCOL_VERSIONS.maximum()
    );
    assert_eq!(
      session.negotiated_capabilities(),
      OPERATION_REGISTRY
        .advertised_capabilities(SUPPORTED_PROTOCOL_VERSIONS.maximum())
        .unwrap()
        .as_ref()
    );
    let response = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "discovery-handshake-query-1".to_string(),
        },
      )
      .await
      .unwrap();
    assert!(response.accepted);

    daemon.stop().await;
  }

  #[tokio::test]
  #[ignore = "discovery metadata is no longer part of the endpoint lease"]
  async fn discovery_handshake_rejects_a_stale_published_instance() {
    let daemon = RunningTestDaemon::start().await;
    let stale = IpcEndpointMetadata::new(&daemon.paths).unwrap();
    let _stale_publication = IpcEndpointPublication::publish(&daemon.paths, &stale).unwrap();

    let error = CadderSession::connect(&daemon.paths).await.unwrap_err();

    assert!(error.is_stale_instance());
    assert_eq!(error.code(), "stale_instance");
    assert!(error.retryable());
    assert!(error.request_id().is_some());
    daemon.stop().await;
  }

  #[tokio::test]
  #[ignore = "discovery metadata is no longer part of the endpoint lease"]
  async fn discovery_handshake_rereads_replaced_generation_once() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let listener = scripted_listener(&paths);
    let stale_metadata = IpcEndpointMetadata::new(&paths).unwrap();
    let replacement_metadata = IpcEndpointMetadata::new(&paths).unwrap();
    let expected_version = replacement_metadata.supported_versions.maximum();
    let expected_capabilities = OPERATION_REGISTRY
      .advertised_capabilities(expected_version)
      .unwrap();
    let replacement_identity = ServerHandshakeIdentity::from(&replacement_metadata);
    let _stale_instance_id = stale_metadata.daemon_instance_id.clone();
    let _stale_publication = IpcEndpointPublication::publish(&paths, &stale_metadata).unwrap();
    let replacement_paths = paths.clone();
    let server = tokio::spawn(async move {
      let mut first = listener.accept().await.unwrap();
      receive_peer_authentication_preface(&mut first)
        .await
        .unwrap();
      let (first_read, mut first_write) = tokio::io::split(first);
      let mut first_reader = FramedRead::new(first_read, BoundedNdjsonCodec::new());
      let hello: ClientHello =
        serde_json::from_str(&first_reader.next().await.unwrap().unwrap()).unwrap();
      assert_eq!(hello.daemon_instance_id.as_ref(), None);

      let _replacement_publication =
        IpcEndpointPublication::publish(&replacement_paths, &replacement_metadata).unwrap();
      let rejection = ServerHandshakeFrame::rejected(
        hello.request_id,
        replacement_identity.runtime_id.clone(),
        replacement_identity.daemon_instance_id.clone(),
        ProtocolError::stale_instance(),
      );
      write_handshake_frame(
        &mut first_write,
        &rejection,
        IpcLimits::default().write_no_progress,
      )
      .await
      .unwrap();
      drop(first_reader);
      drop(first_write);

      let mut second = listener.accept().await.unwrap();
      receive_peer_authentication_preface(&mut second)
        .await
        .unwrap();
      let _second = accept_scripted_handshake(second, &replacement_identity).await;
    });

    let session = CadderSession::connect(&paths).await.unwrap();

    assert_eq!(session.negotiated_version(), expected_version);
    assert_eq!(
      session.negotiated_capabilities(),
      expected_capabilities.as_ref()
    );
    drop(session);
    server.await.unwrap();
  }

  #[tokio::test]
  #[ignore = "discovery metadata is no longer part of the endpoint lease"]
  async fn discovery_handshake_reports_an_unreachable_published_endpoint_as_stale() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let metadata = IpcEndpointMetadata::new(&paths).unwrap();
    let _publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();

    let error = CadderSession::connect(&paths).await.unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Discovery,
      IpcClientPhase::Connect,
      "stale_instance",
      None,
    );
    assert!(error.is_stale_instance());
  }

  #[test]
  fn discovery_handshake_validates_every_accepted_server_field() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let discovery = IpcEndpointMetadata::new(&paths).unwrap();
    let request_id = RequestId::parse("hello-validation-1").unwrap();
    let requested = OPERATION_REGISTRY
      .advertised_capabilities(SUPPORTED_PROTOCOL_VERSIONS.maximum())
      .unwrap();
    let capabilities = requested
      .iter()
      .filter(|capability| discovery.capabilities.contains(capability))
      .cloned()
      .collect::<Box<[_]>>();
    let hello = ServerHello {
      request_id: request_id.clone(),
      runtime_id: discovery.runtime_id.clone().into_boxed_str(),
      daemon_instance_id: discovery.daemon_instance_id.clone().into_boxed_str(),
      selected_version: SUPPORTED_PROTOCOL_VERSIONS.maximum(),
      capabilities,
    };
    assert!(
      validate_server_hello(hello.clone(), &discovery, request_id.clone(), &requested).is_ok()
    );

    let mut wrong_instance = hello.clone();
    wrong_instance.daemon_instance_id = "00000000000000000000000000000000".into();
    assert!(
      validate_server_hello(wrong_instance, &discovery, request_id.clone(), &requested).is_ok()
    );

    let mut wrong_runtime = hello.clone();
    wrong_runtime.runtime_id = "other-runtime".into();
    let error =
      validate_server_hello(wrong_runtime, &discovery, request_id.clone(), &requested).unwrap_err();
    assert!(error.is_stale_instance());

    let mut wrong_request = hello.clone();
    wrong_request.request_id = RequestId::parse("hello-validation-other").unwrap();
    assert_eq!(
      validate_server_hello(wrong_request, &discovery, request_id.clone(), &requested)
        .unwrap_err()
        .code(),
      "protocol_violation"
    );

    let mut wrong_version = hello.clone();
    wrong_version.selected_version = ProtocolVersion::new(
      hello.selected_version.major(),
      hello.selected_version.minor().checked_add(1).unwrap(),
    )
    .unwrap();
    assert_eq!(
      validate_server_hello(wrong_version, &discovery, request_id.clone(), &requested)
        .unwrap_err()
        .code(),
      "protocol_violation"
    );

    let mut missing_capability = hello.clone();
    missing_capability.capabilities = missing_capability
      .capabilities
      .iter()
      .skip(1)
      .cloned()
      .collect();
    assert_eq!(
      validate_server_hello(
        missing_capability,
        &discovery,
        request_id.clone(),
        &requested,
      )
      .unwrap_err()
      .code(),
      "protocol_violation"
    );

    let mut extra_capability = hello.clone();
    extra_capability.capabilities = extra_capability
      .capabilities
      .iter()
      .cloned()
      .chain([CapabilityId::parse("unexpected-capability").unwrap()])
      .collect();
    assert_eq!(
      validate_server_hello(extra_capability, &discovery, request_id.clone(), &requested)
        .unwrap_err()
        .code(),
      "protocol_violation"
    );

    let mut duplicate_capability = hello;
    duplicate_capability.capabilities = duplicate_capability
      .capabilities
      .iter()
      .cloned()
      .chain(duplicate_capability.capabilities.first().cloned())
      .collect();
    assert_eq!(
      validate_server_hello(duplicate_capability, &discovery, request_id, &requested,)
        .unwrap_err()
        .code(),
      "protocol_violation"
    );
  }

  #[tokio::test]
  async fn discovery_handshake_readiness_requires_a_server_hello() {
    let server = ScriptedIpcServer::start_without_handshake(|conn| async move {
      let _conn = conn;
      pending::<()>().await;
    });
    let deadlines = IpcClientDeadlines {
      connect: Duration::from_millis(25),
      ..IpcClientDeadlines::default()
    };

    let error = daemon_is_ready_with_deadlines(&server.paths, deadlines)
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Timeout,
      IpcClientPhase::Connect,
      "timeout",
      None,
    );
    server.abort().await;
  }

  #[tokio::test]
  async fn ipc_limits_close_a_connection_that_sends_no_first_frame_byte() {
    let daemon = RunningTestDaemon::start_with_limits(IpcLimits {
      first_frame_byte: Duration::from_millis(100),
      ..IpcLimits::default()
    })
    .await;
    let conn = connect_authenticated_transport(&daemon.paths).await;

    assert_transport_closes(conn).await;

    daemon.stop().await;
  }

  #[tokio::test]
  async fn ipc_limits_close_a_connection_that_does_not_complete_its_first_frame() {
    let daemon = RunningTestDaemon::start_with_limits(IpcLimits {
      first_frame_byte: Duration::from_secs(1),
      frame_completion: Duration::from_millis(100),
      ..IpcLimits::default()
    })
    .await;
    let mut conn = connect_authenticated_transport(&daemon.paths).await;
    conn.write_all(b"{").await.unwrap();

    assert_transport_closes(conn).await;

    daemon.stop().await;
  }

  #[tokio::test]
  async fn ipc_limits_reject_capacity_without_disrupting_an_accepted_session() {
    let daemon = RunningTestDaemon::start_with_limits(IpcLimits {
      max_connections: 1,
      ..IpcLimits::default()
    })
    .await;
    let mut accepted = connect_session_eventually(&daemon.paths).await;
    let rejected = connect_transport(&daemon.paths).await;

    assert_transport_closes(rejected).await;
    let response = accepted
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "ipc-limits-capacity-query-1".to_string(),
        },
      )
      .await
      .unwrap();
    assert!(response.accepted);

    drop(accepted);
    let replacement = connect_session_eventually(&daemon.paths).await;
    drop(replacement);
    daemon.stop().await;
  }

  #[tokio::test]
  async fn ipc_limits_reject_pipelining_after_the_first_response() {
    let daemon = RunningTestDaemon::start().await;
    let conn = connect_authenticated(&daemon.paths).await;
    let (read_half, mut writer) = tokio::io::split(conn);
    let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
    let first = encode_envelope(
      message_types::QUERY_STATE_REQUEST,
      &QueryStateRequest {
        request_id: "ipc-limits-pipeline-first".to_string(),
      },
    )
    .unwrap();
    let second = encode_envelope(
      message_types::QUERY_STATE_REQUEST,
      &QueryStateRequest {
        request_id: "ipc-limits-pipeline-second".to_string(),
      },
    )
    .unwrap();
    writer.write_all(&[first, second].concat()).await.unwrap();

    let first: IpcEnvelope = serde_json::from_str(&reader.next().await.unwrap().unwrap()).unwrap();
    assert_eq!(first.message_type, message_types::QUERY_STATE_RESPONSE);
    let violation: IpcEnvelope =
      serde_json::from_str(&reader.next().await.unwrap().unwrap()).unwrap();
    let violation: ProtocolErrorResponse = violation.decode().unwrap();
    assert_eq!(violation.request_id, "ipc-limits-pipeline-second");
    assert_eq!(violation.error.kind, ProtocolErrorKind::ProtocolViolation);
    assert_eq!(violation.error.code.as_str(), "pipelined_request");
    assert!(reader.next().await.is_none());

    daemon.stop().await;
  }

  #[tokio::test]
  async fn ipc_limits_reject_a_partial_pipelined_frame_after_the_first_response() {
    let daemon = RunningTestDaemon::start().await;
    let conn = connect_authenticated(&daemon.paths).await;
    let (read_half, mut writer) = tokio::io::split(conn);
    let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
    let first = encode_envelope(
      message_types::QUERY_STATE_REQUEST,
      &QueryStateRequest {
        request_id: "ipc-limits-partial-pipeline-first".to_string(),
      },
    )
    .unwrap();
    writer
      .write_all(&[first.as_slice(), b"{"].concat())
      .await
      .unwrap();

    let first: IpcEnvelope = serde_json::from_str(&reader.next().await.unwrap().unwrap()).unwrap();
    assert_eq!(first.message_type, message_types::QUERY_STATE_RESPONSE);
    let violation: IpcEnvelope =
      serde_json::from_str(&reader.next().await.unwrap().unwrap()).unwrap();
    let violation: ProtocolErrorResponse = violation.decode().unwrap();
    assert_eq!(violation.request_id, "unknown");
    assert_eq!(violation.error.code.as_str(), "pipelined_request");
    assert!(reader.next().await.is_none());

    daemon.stop().await;
  }

  #[tokio::test]
  async fn ipc_limits_return_retryable_ordinary_operation_timeout() {
    let daemon = RunningTestDaemon::start_with_limits(IpcLimits {
      ordinary_operation: Duration::from_millis(25),
      dispatch_delay: Duration::from_millis(100),
      ..IpcLimits::default()
    })
    .await;
    let mut session = CadderSession::connect(&daemon.paths).await.unwrap();

    let error = session
      .request::<QueryStateRequest>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "ipc-limits-ordinary-timeout".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_daemon_error(
      &error,
      ProtocolErrorKind::Timeout,
      "timeout",
      "ipc-limits-ordinary-timeout",
      "Cadder did not finish `query-state-request` before its local operation deadline; the outcome is unknown.",
      Some("Check the current daemon state before retrying the operation."),
      true,
    );
    daemon.stop().await;
  }

  #[tokio::test]
  async fn ipc_limits_revoke_autostart_before_claim_without_history() {
    let daemon = RunningTestDaemon::start_with_limits(IpcLimits {
      ordinary_operation: Duration::from_millis(25),
      dispatch_delay: Duration::from_millis(100),
      ..IpcLimits::default()
    })
    .await;
    let mut session = CadderSession::connect(&daemon.paths).await.unwrap();

    let error = session
      .request::<SetAutostartRequest>(
        message_types::SET_AUTOSTART_REQUEST,
        message_types::SET_AUTOSTART_RESPONSE,
        &SetAutostartRequest {
          request_id: "ipc-limits-autostart-timeout".to_string(),
          mode: AutostartMode::Daemon,
        },
      )
      .await
      .unwrap_err();

    assert_daemon_error(
      &error,
      ProtocolErrorKind::Timeout,
      "timeout",
      "ipc-limits-autostart-timeout",
      "Cadder did not finish `set-autostart-request` before its local operation deadline; the outcome is unknown.",
      Some("Check the current daemon state before retrying the operation."),
      false,
    );
    sleep(Duration::from_millis(125)).await;
    let history = daemon
      .state
      .query_history(cadder_ipc::QueryHistoryRequest {
        request_id: "autostart-timeout-history".to_string(),
        kind: Some(cadder_ipc::HistoryKind::Autostart),
        limit: Some(10),
      })
      .await;
    assert!(history.records.is_empty());
    daemon.stop().await;
  }

  #[tokio::test]
  async fn ipc_limits_return_non_retryable_reload_timeout() {
    let daemon = RunningTestDaemon::start_with_limits(IpcLimits {
      reload_operation: Duration::from_millis(25),
      dispatch_delay: Duration::from_millis(100),
      ..IpcLimits::default()
    })
    .await;
    let mut session = CadderSession::connect(&daemon.paths).await.unwrap();

    let error = session
      .request::<SetEntrypointEnabledRequest>(
        message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
        message_types::SET_ENTRYPOINT_ENABLED_RESPONSE,
        &SetEntrypointEnabledRequest {
          request_id: "ipc-limits-reload-timeout".to_string(),
          registration_id: "missing".to_string(),
          shim_session_nonce: None,
          enabled: false,
        },
      )
      .await
      .unwrap_err();

    assert_daemon_error(
      &error,
      ProtocolErrorKind::Timeout,
      "timeout",
      "ipc-limits-reload-timeout",
      "Cadder did not finish `set-entrypoint-enabled-request` before its local operation deadline; the outcome is unknown.",
      Some("Check the current daemon state before retrying the operation."),
      false,
    );
    daemon.stop().await;
  }

  #[tokio::test]
  async fn ipc_limits_allow_reload_past_the_ordinary_budget() {
    let daemon = RunningTestDaemon::start_with_limits(IpcLimits {
      ordinary_operation: Duration::from_millis(25),
      reload_operation: Duration::from_millis(250),
      dispatch_delay: Duration::from_millis(75),
      ..IpcLimits::default()
    })
    .await;
    let mut session = CadderSession::connect(&daemon.paths).await.unwrap();

    let response: BasicResponse = session
      .request::<SetEntrypointEnabledRequest>(
        message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
        message_types::SET_ENTRYPOINT_ENABLED_RESPONSE,
        &SetEntrypointEnabledRequest {
          request_id: "ipc-limits-reload-extended".to_string(),
          registration_id: "missing".to_string(),
          shim_session_nonce: None,
          enabled: false,
        },
      )
      .await
      .unwrap();

    assert!(!response.accepted);
    daemon.stop().await;
  }

  #[tokio::test]
  async fn ipc_limits_stream_setup_deadline_does_not_end_an_active_stream() {
    let daemon = RunningTestDaemon::start_with_limits(IpcLimits {
      stream_setup: Duration::from_millis(25),
      ..IpcLimits::default()
    })
    .await;
    let conn = connect_authenticated(&daemon.paths).await;
    let (read_half, mut writer) = tokio::io::split(conn);
    let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
    write_envelope(
      &mut writer,
      message_types::SUBSCRIBE_STATE_REQUEST,
      &SubscribeStateRequest {
        request_id: "ipc-limits-stream".to_string(),
      },
    )
    .await
    .unwrap();
    let initial: IpcEnvelope =
      serde_json::from_str(&reader.next().await.unwrap().unwrap()).unwrap();
    assert_eq!(initial.message_type, message_types::STATE_CHANGED_EVENT);

    sleep(Duration::from_millis(75)).await;
    write_envelope(
      &mut writer,
      message_types::QUERY_STATE_REQUEST,
      &QueryStateRequest {
        request_id: "ipc-limits-stream-pipeline".to_string(),
      },
    )
    .await
    .unwrap();
    let violation: IpcEnvelope =
      serde_json::from_str(&reader.next().await.unwrap().unwrap()).unwrap();
    let violation: ProtocolErrorResponse = violation.decode().unwrap();
    assert_eq!(violation.request_id, "ipc-limits-stream-pipeline");
    assert_eq!(violation.error.code.as_str(), "pipelined_request");

    daemon.stop().await;
  }

  #[tokio::test]
  async fn ipc_limits_stream_pipeline_preserves_initial_response_order() {
    let daemon = RunningTestDaemon::start_with_limits(IpcLimits {
      stream_setup: Duration::from_millis(250),
      dispatch_delay: Duration::from_millis(75),
      ..IpcLimits::default()
    })
    .await;
    let conn = connect_authenticated(&daemon.paths).await;
    let (read_half, mut writer) = tokio::io::split(conn);
    let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
    let subscription = encode_envelope(
      message_types::SUBSCRIBE_STATE_REQUEST,
      &SubscribeStateRequest {
        request_id: "ipc-limits-stream-ordered".to_string(),
      },
    )
    .unwrap();
    let pipelined = encode_envelope(
      message_types::QUERY_STATE_REQUEST,
      &QueryStateRequest {
        request_id: "ipc-limits-stream-ordered-pipeline".to_string(),
      },
    )
    .unwrap();
    writer
      .write_all(&[subscription, pipelined].concat())
      .await
      .unwrap();

    let initial: IpcEnvelope =
      serde_json::from_str(&reader.next().await.unwrap().unwrap()).unwrap();
    assert_eq!(initial.message_type, message_types::STATE_CHANGED_EVENT);
    let violation: IpcEnvelope =
      serde_json::from_str(&reader.next().await.unwrap().unwrap()).unwrap();
    let violation: ProtocolErrorResponse = violation.decode().unwrap();
    assert_eq!(violation.request_id, "ipc-limits-stream-ordered-pipeline");
    assert_eq!(violation.error.code.as_str(), "pipelined_request");

    daemon.stop().await;
  }

  #[test]
  fn daemon_launch_lock_serializes_start_attempts() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();

    let first = DaemonLaunchLock::try_acquire(&paths).unwrap();
    assert!(first.is_some());
    assert!(DaemonLaunchLock::try_acquire(&paths).unwrap().is_none());

    drop(first);

    assert!(DaemonLaunchLock::try_acquire(&paths).unwrap().is_some());
  }

  #[test]
  fn connection_ownership_replaces_removes_and_takes_owned_entries() {
    let registrations = ConnectionOwnership::default();

    registrations.insert("shim-1".to_string(), "nonce-1".to_string());
    registrations.insert("shim-1".to_string(), "nonce-2".to_string());
    registrations.insert("shim-2".to_string(), "nonce-3".to_string());
    registrations.remove("missing");
    registrations.remove("shim-2");

    let entries = registrations.take_entries().collect::<Vec<_>>();
    assert_eq!(entries, vec![("shim-1".to_string(), "nonce-2".to_string())]);
  }

  #[tokio::test]
  async fn connection_ownership_cleanup_waits_for_a_late_successful_mutation() {
    let ownership = ConnectionOwnership::default();
    let mutation = ownership
      .begin_mutation()
      .expect("connected ownership should accept a mutation");
    let cleanup_ownership = ownership.clone();
    let cleanup =
      tokio::spawn(async move { cleanup_ownership.disconnect_and_take_entries().await });
    while !ownership.is_disconnected() {
      tokio::task::yield_now().await;
    }

    ownership.insert("shim-late".to_string(), "nonce-late".to_string());
    assert!(ownership.begin_mutation().is_none());
    drop(mutation);

    let entries = timeout(Duration::from_secs(1), cleanup)
      .await
      .expect("disconnect cleanup should finish after the mutation")
      .unwrap();
    assert_eq!(
      entries,
      BTreeMap::from([("shim-late".to_string(), "nonce-late".to_string())])
    );
  }

  #[test]
  fn find_on_path_uses_platform_executable_name() {
    let _lock = lock_env();
    let _snapshot = EnvSnapshot::capture("PATH");
    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join(exe_name("cadderd"));
    fs::write(&executable, b"").unwrap();
    unsafe {
      env::set_var("PATH", env::join_paths([temp.path()]).unwrap());
    }

    assert_eq!(find_on_path("cadderd"), Some(executable));
  }

  #[test]
  fn find_on_path_reports_missing_path_and_missing_executable() {
    let _lock = lock_env();
    let _snapshot = EnvSnapshot::capture("PATH");

    unsafe {
      env::remove_var("PATH");
    }
    assert!(find_on_path("cadderd").is_none());

    let temp = tempfile::tempdir().unwrap();
    unsafe {
      env::set_var("PATH", env::join_paths([temp.path()]).unwrap());
    }
    assert!(find_on_path("cadderd").is_none());
  }

  #[tokio::test]
  async fn typed_error_session_preserves_daemon_error_fields() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_protocol_error(
        &mut writer,
        "typed-session-1",
        ProtocolError::new(
          ProtocolErrorKind::Conflict,
          ProtocolErrorCode::parse("registration_conflict").unwrap(),
          "The registration conflicts with an active owner.",
          Some("Refresh registration state before retrying.".into()),
          false,
        ),
      )
      .await;
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-session-1".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_daemon_error(
      &error,
      ProtocolErrorKind::Conflict,
      "registration_conflict",
      "typed-session-1",
      "The registration conflicts with an active owner.",
      Some("Refresh registration state before retrying."),
      false,
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_client_preserves_daemon_error_fields() {
    let expected = ProtocolError::access_denied(
      message_types::QUERY_STATE_REQUEST,
      "The runtime belongs to another account.",
      Some("Use the account that owns this runtime.".to_string()),
    )
    .with_request_id(RequestId::parse("typed-client-1").unwrap());
    let wire_error = expected.clone();
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_protocol_error(&mut writer, "typed-client-1", wire_error).await;
    });
    let client = CadderClient::new(server.paths.clone());

    let error = client
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-client-1".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_daemon_error(
      &error,
      ProtocolErrorKind::AccessDenied,
      "permission_denied",
      "typed-client-1",
      "The runtime belongs to another account.",
      Some("Use the account that owns this runtime."),
      false,
    );
    assert_eq!(error.daemon_error(), Some(&expected));
    assert_eq!(
      error
        .daemon_error()
        .and_then(|error| error.denied_operation.as_deref()),
      Some(message_types::QUERY_STATE_REQUEST)
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_subscription_preserves_daemon_error_fields() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_protocol_error(
        &mut writer,
        "typed-subscription-1",
        ProtocolError::new(
          ProtocolErrorKind::ShuttingDown,
          ProtocolErrorCode::parse("shutting_down").unwrap(),
          "The daemon is shutting down.",
          Some("Start the daemon again, then retry.".into()),
          true,
        ),
      )
      .await;
    });
    let session = CadderSession::connect(&server.paths).await.unwrap();
    let mut subscription = session
      .subscribe_state("typed-subscription-1".to_string())
      .await
      .unwrap();

    let error = subscription.next_event().await.unwrap_err();

    assert_daemon_error(
      &error,
      ProtocolErrorKind::ShuttingDown,
      "shutting_down",
      "typed-subscription-1",
      "The daemon is shutting down.",
      Some("Start the daemon again, then retry."),
      true,
    );
    server.finish().await;
  }

  #[tokio::test]
  #[ignore = "discovery metadata is no longer authoritative"]
  async fn typed_error_session_requires_authoritative_discovery() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();

    let error = CadderSession::connect(&paths).await.unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Discovery,
      IpcClientPhase::DiscoveryRead,
      "discovery_unavailable",
      None,
    );
  }

  #[test]
  fn typed_error_connect_classification_distinguishes_unavailable_permission_and_transport() {
    let cases = [
      (
        io::ErrorKind::NotFound,
        LocalIpcErrorKind::Transport,
        LocalIpcErrorCode::DaemonUnavailable,
        true,
      ),
      (
        io::ErrorKind::PermissionDenied,
        LocalIpcErrorKind::Transport,
        LocalIpcErrorCode::PermissionDenied,
        false,
      ),
      (
        io::ErrorKind::InvalidData,
        LocalIpcErrorKind::Transport,
        LocalIpcErrorCode::TransportConnect,
        false,
      ),
      (
        io::ErrorKind::TimedOut,
        LocalIpcErrorKind::Timeout,
        LocalIpcErrorCode::Timeout,
        true,
      ),
    ];

    for (kind, expected_kind, code, retryable) in cases {
      let error = connection_error(io::Error::new(kind, "test connection failure"));
      let local = error.local_error().expect("expected a local connect error");
      assert_eq!(local.kind(), expected_kind);
      assert_eq!(local.phase(), IpcClientPhase::Connect);
      assert_eq!(local.code(), code);
      assert_eq!(local.retryable(), retryable);
      assert!(std::error::Error::source(local).is_some());
    }

    assert_eq!(
      launch_code_for_io(io::ErrorKind::PermissionDenied),
      LocalIpcErrorCode::PermissionDenied
    );
    assert_eq!(
      launch_code_for_io(io::ErrorKind::NotFound),
      LocalIpcErrorCode::DaemonStartFailed
    );
  }

  #[tokio::test]
  #[ignore = "discovery metadata is no longer authoritative"]
  async fn typed_error_client_discovery_failure_keeps_request_id() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let client = CadderClient::new(paths);

    let error = client
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-connect-1".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Discovery,
      IpcClientPhase::DiscoveryRead,
      "discovery_unavailable",
      Some("typed-connect-1"),
    );
  }

  #[tokio::test]
  #[ignore = "discovery metadata is no longer authoritative"]
  async fn typed_error_client_subscription_discovery_failure_keeps_request_id() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let client = CadderClient::new(paths);

    let error = client
      .subscribe_state("typed-connect-subscription-1".to_string())
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Discovery,
      IpcClientPhase::DiscoveryRead,
      "discovery_unavailable",
      Some("typed-connect-subscription-1"),
    );
  }

  #[tokio::test]
  async fn typed_error_request_write_io_failure_keeps_context_and_source() {
    let prepared = PreparedClientRequest::new(
      message_types::QUERY_STATE_REQUEST,
      message_types::QUERY_STATE_RESPONSE,
      &QueryStateRequest {
        request_id: "typed-write-io-1".to_string(),
      },
    )
    .unwrap();
    let (mut writer, peer) = tokio::io::duplex(64);
    drop(peer);

    let error = write_prepared_request(&mut writer, &prepared)
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::RequestWrite,
      "transport_write",
      Some("typed-write-io-1"),
    );
    assert_eq!(error.operation(), Some(message_types::QUERY_STATE_REQUEST));
    assert!(error.retryable());
    let source = std::error::Error::source(
      error
        .local_error()
        .expect("write failure should be a local IPC error"),
    )
    .and_then(|source| source.downcast_ref::<io::Error>())
    .expect("write failure should retain its I/O source");
    assert_eq!(source.kind(), io::ErrorKind::BrokenPipe);
  }

  #[tokio::test]
  async fn typed_error_request_write_timeout_has_request_write_phase() {
    let prepared = PreparedClientRequest::new(
      message_types::QUERY_STATE_REQUEST,
      message_types::QUERY_STATE_RESPONSE,
      &QueryStateRequest {
        request_id: "typed-write-timeout-1".to_string(),
      },
    )
    .unwrap();
    let (mut writer, _peer) = tokio::io::duplex(1);

    let error = write_prepared_request_until(
      &mut writer,
      &prepared,
      tokio::time::Instant::now() + Duration::from_millis(25),
    )
    .await
    .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Timeout,
      IpcClientPhase::RequestWrite,
      "timeout",
      Some("typed-write-timeout-1"),
    );
    assert_eq!(error.operation(), Some(message_types::QUERY_STATE_REQUEST));
    assert!(error.retryable());
  }

  #[test]
  fn typed_error_response_read_io_failure_keeps_context_and_source() {
    let prepared = PreparedClientRequest::new(
      message_types::QUERY_STATE_REQUEST,
      message_types::QUERY_STATE_RESPONSE,
      &QueryStateRequest {
        request_id: "typed-read-io-1".to_string(),
      },
    )
    .unwrap();

    let error = response_read_error(
      &prepared.context,
      io::Error::new(io::ErrorKind::ConnectionReset, "test read reset"),
    );

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseRead,
      "transport_read",
      Some("typed-read-io-1"),
    );
    assert!(error.retryable());
    assert!(
      std::error::Error::source(
        error
          .local_error()
          .expect("read failure should be a local IPC error")
      )
      .is_some_and(|source| source.downcast_ref::<io::Error>().is_some())
    );
  }

  #[tokio::test]
  async fn typed_error_request_timeout_is_local_and_correlated() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, _writer) = read_one_request(conn).await;
      pending::<()>().await;
    });
    let client = CadderClient::new(server.paths.clone()).with_deadlines(short_client_deadlines());

    let error = client
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-timeout-1".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Timeout,
      IpcClientPhase::ResponseRead,
      "timeout",
      Some("typed-timeout-1"),
    );
    assert!(error.retryable());
    server.abort().await;
  }

  #[tokio::test]
  async fn typed_error_mutation_timeout_is_not_retryable() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, _writer) = read_one_request(conn).await;
      pending::<()>().await;
    });
    let client = CadderClient::new(server.paths.clone()).with_deadlines(short_client_deadlines());

    let error = client
      .request::<_>(
        message_types::SET_AUTOSTART_REQUEST,
        message_types::SET_AUTOSTART_RESPONSE,
        &SetAutostartRequest {
          request_id: "typed-mutation-timeout-1".to_string(),
          mode: AutostartMode::Daemon,
        },
      )
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Timeout,
      IpcClientPhase::ResponseRead,
      "timeout",
      Some("typed-mutation-timeout-1"),
    );
    assert!(!error.retryable());
    server.abort().await;
  }

  #[tokio::test]
  async fn typed_error_closed_session_marks_unsent_mutation_retryable() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      writer.write_all(b"{not-json}\n").await.unwrap();
      writer.flush().await.unwrap();
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let first_error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-poison-session-1".to_string(),
        },
      )
      .await
      .unwrap_err();
    assert_local_error(
      &first_error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseDecode,
      "frame",
      Some("typed-poison-session-1"),
    );

    let retry_error = session
      .request::<_>(
        message_types::SET_AUTOSTART_REQUEST,
        message_types::SET_AUTOSTART_RESPONSE,
        &SetAutostartRequest {
          request_id: "typed-unsent-mutation-1".to_string(),
          mode: AutostartMode::Daemon,
        },
      )
      .await
      .unwrap_err();
    assert_local_error(
      &retry_error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::Connect,
      "connection_closed",
      Some("typed-unsent-mutation-1"),
    );
    assert!(retry_error.retryable());
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_daemon_timeout_remains_remote() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_protocol_error(
        &mut writer,
        "typed-remote-timeout-1",
        ProtocolError::new(
          ProtocolErrorKind::Timeout,
          ProtocolErrorCode::parse("timeout").unwrap(),
          "The daemon operation exceeded its deadline.",
          Some("Check current state before retrying.".into()),
          false,
        ),
      )
      .await;
    });
    let client = CadderClient::new(server.paths.clone());

    let error = client
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-remote-timeout-1".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_daemon_error(
      &error,
      ProtocolErrorKind::Timeout,
      "timeout",
      "typed-remote-timeout-1",
      "The daemon operation exceeded its deadline.",
      Some("Check current state before retrying."),
      false,
    );
    server.finish().await;
  }

  #[test]
  fn typed_error_missing_and_malformed_discovery_are_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();

    let missing = discover_ipc_endpoint(&paths).unwrap_err();
    assert_local_error(
      &missing,
      LocalIpcErrorKind::Discovery,
      IpcClientPhase::DiscoveryRead,
      "discovery_unavailable",
      None,
    );

    let missing_source = std::error::Error::source(
      missing
        .local_error()
        .expect("missing discovery should be a local error"),
    )
    .and_then(|source| source.downcast_ref::<io::Error>())
    .expect("missing discovery should retain its I/O source");
    assert_eq!(missing_source.kind(), io::ErrorKind::NotFound);

    paths.ensure_dirs().unwrap();
    fs::write(paths.ipc_endpoint_path(), b"{not-json}").unwrap();
    let malformed = discover_ipc_endpoint(&paths).unwrap_err();
    assert_local_error(
      &malformed,
      LocalIpcErrorKind::Discovery,
      IpcClientPhase::DiscoveryDecode,
      "invalid_discovery",
      None,
    );
    let malformed_source = std::error::Error::source(
      malformed
        .local_error()
        .expect("malformed discovery should be a local error"),
    )
    .expect("malformed discovery should retain its decode source");
    assert!(
      malformed_source
        .downcast_ref::<serde_json::Error>()
        .is_some()
    );

    fs::write(paths.ipc_endpoint_path(), [0xff]).unwrap();
    let invalid_utf8 = discover_ipc_endpoint(&paths).unwrap_err();
    assert_local_error(
      &invalid_utf8,
      LocalIpcErrorKind::Discovery,
      IpcClientPhase::DiscoveryDecode,
      "invalid_discovery",
      None,
    );
    let invalid_utf8_source = std::error::Error::source(
      invalid_utf8
        .local_error()
        .expect("invalid UTF-8 discovery should be a local error"),
    )
    .expect("invalid UTF-8 discovery should retain its decode source");
    assert!(
      invalid_utf8_source
        .downcast_ref::<serde_json::Error>()
        .is_some()
    );
  }

  #[tokio::test]
  async fn typed_error_legacy_adapter_defaults_fields_and_preserves_outer_correlation() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      let envelope = serde_json::json!({
        "protocolVersion": cadder_ipc::PROTOCOL_VERSION,
        "type": message_types::PROTOCOL_ERROR_RESPONSE,
        "payload": {
          "requestId": "typed-legacy-1",
          "accepted": false,
          "error": {
            "kind": "conflict",
            "message": "Legacy conflict."
          }
        }
      });
      writer
        .write_all(format!("{envelope}\n").as_bytes())
        .await
        .unwrap();
      writer.flush().await.unwrap();
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-legacy-1".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_daemon_error(
      &error,
      ProtocolErrorKind::Conflict,
      "conflict",
      "typed-legacy-1",
      "Legacy conflict.",
      None,
      false,
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_rejects_daemon_error_for_another_request() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_protocol_error(
        &mut writer,
        "typed-other-1",
        ProtocolError::new(
          ProtocolErrorKind::Conflict,
          ProtocolErrorCode::parse("conflict").unwrap(),
          "Other request failed.",
          None,
          false,
        ),
      )
      .await;
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-original-1".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseValidate,
      "protocol_violation",
      Some("typed-original-1"),
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_rejects_mismatched_nested_daemon_error_correlation() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      let envelope = serde_json::json!({
        "protocolVersion": cadder_ipc::PROTOCOL_VERSION,
        "type": message_types::PROTOCOL_ERROR_RESPONSE,
        "payload": {
          "requestId": "typed-nested-original-1",
          "accepted": false,
          "error": {
            "kind": "conflict",
            "code": "conflict",
            "message": "Another request failed.",
            "guidance": null,
            "retryable": false,
            "requestId": "typed-nested-other-1"
          }
        }
      });
      writer
        .write_all(format!("{envelope}\n").as_bytes())
        .await
        .unwrap();
      writer.flush().await.unwrap();
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-nested-original-1".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseValidate,
      "protocol_violation",
      Some("typed-nested-original-1"),
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_accepts_null_nested_request_id_as_legacy_outer_correlation() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      let envelope = serde_json::json!({
        "protocolVersion": cadder_ipc::PROTOCOL_VERSION,
        "type": message_types::PROTOCOL_ERROR_RESPONSE,
        "payload": {
          "requestId": "typed-null-nested-1",
          "accepted": false,
          "error": {
            "kind": "conflict",
            "code": "conflict",
            "message": "Legacy daemon conflict.",
            "guidance": "Resolve the conflicting operation, then retry.",
            "retryable": false,
            "requestId": null
          }
        }
      });
      writer
        .write_all(format!("{envelope}\n").as_bytes())
        .await
        .unwrap();
      writer.flush().await.unwrap();
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-null-nested-1".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_daemon_error(
      &error,
      ProtocolErrorKind::Conflict,
      "conflict",
      "typed-null-nested-1",
      "Legacy daemon conflict.",
      Some("Resolve the conflicting operation, then retry."),
      false,
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_rejects_success_for_another_request() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_envelope(
        &mut writer,
        message_types::QUERY_STATE_RESPONSE,
        &BasicResponse {
          request_id: "typed-other-success-1".to_string(),
          accepted: true,
          message: "Completed.".to_string(),
        },
      )
      .await
      .unwrap();
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-original-success-1".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseValidate,
      "protocol_violation",
      Some("typed-original-success-1"),
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_request_eof_keeps_request_id() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (read_half, write_half) = tokio::io::split(conn);
      let mut reader = BufReader::new(read_half);
      let mut line = String::new();
      reader.read_line(&mut line).await.unwrap();
      drop(write_half);
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let request_id = "typed-eof-1";
    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: request_id.to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseRead,
      "unexpected_eof",
      Some(request_id),
    );
    let terminal_error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-eof-retry-1".to_string(),
        },
      )
      .await
      .unwrap_err();
    assert_local_error(
      &terminal_error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::Connect,
      "connection_closed",
      Some("typed-eof-retry-1"),
    );
    assert!(terminal_error.retryable());
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_cancelled_request_retires_session_and_closes_transport() {
    let (request_received_tx, request_received_rx) = oneshot::channel();
    let server = ScriptedIpcServer::start(|conn| async move {
      let (read_half, _writer) = tokio::io::split(conn);
      let mut reader = BufReader::new(read_half);
      let mut line = String::new();
      reader.read_line(&mut line).await.unwrap();
      assert!(!line.is_empty());
      request_received_tx.send(()).unwrap();
      let mut trailing = Vec::new();
      reader.read_to_end(&mut trailing).await.unwrap();
      assert!(trailing.is_empty());
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();
    let request = QueryStateRequest {
      request_id: "typed-cancelled-request-1".to_string(),
    };
    let mut in_flight = Box::pin(session.request::<_>(
      message_types::QUERY_STATE_REQUEST,
      message_types::QUERY_STATE_RESPONSE,
      &request,
    ));

    tokio::select! {
      result = &mut in_flight => panic!("request unexpectedly completed: {result:?}"),
      received = request_received_rx => received.expect("server should receive the request"),
    }
    drop(in_flight);

    let terminal_error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-after-cancel-1".to_string(),
        },
      )
      .await
      .unwrap_err();
    assert_local_error(
      &terminal_error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::Connect,
      "connection_closed",
      Some("typed-after-cancel-1"),
    );
    timeout(Duration::from_secs(1), server.finish())
      .await
      .expect("cancelled request should close the transport");
  }

  #[tokio::test]
  async fn typed_error_session_rejects_unexpected_response_type() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_basic_response(&mut writer, message_types::QUERY_LOGS_RESPONSE).await;
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();
    let request_id = "unexpected-response-type-1";

    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: request_id.to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseValidate,
      "protocol_violation",
      Some(request_id),
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_incompatible_response_precedes_payload_interpretation() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      let envelope = serde_json::json!({
        "protocolVersion": cadder_ipc::MIN_COMPATIBLE_PROTOCOL_VERSION.saturating_sub(1),
        "type": "unexpected-response",
        "payload": {}
      });
      writer
        .write_all(format!("{envelope}\n").as_bytes())
        .await
        .unwrap();
      writer.flush().await.unwrap();
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: "typed-incompatible-response-1".to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseValidate,
      "incompatible_protocol",
      Some("typed-incompatible-response-1"),
    );
    assert!(!error.retryable());
    assert!(error.is_protocol_incompatible());
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_session_rejects_malformed_response_json() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      writer.write_all(b"{not-json}\n").await.unwrap();
      writer.flush().await.unwrap();
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();
    let request_id = "malformed-response-1";

    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: request_id.to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseDecode,
      "frame",
      Some(request_id),
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_session_rejects_response_payload_shape() {
    let request_id = "invalid-response-payload-1";
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_envelope(
        &mut writer,
        message_types::QUERY_STATE_RESPONSE,
        &serde_json::json!({
          "requestId": "invalid-response-payload-1",
          "accepted": true
        }),
      )
      .await
      .unwrap();
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: request_id.to_string(),
        },
      )
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseDecode,
      "frame",
      Some(request_id),
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_subscription_eof_keeps_request_id() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (read_half, write_half) = tokio::io::split(conn);
      let mut reader = BufReader::new(read_half);
      let mut line = String::new();
      reader.read_line(&mut line).await.unwrap();
      drop(write_half);
    });
    let request_id = "typed-subscription-eof-1";
    let session = CadderSession::connect(&server.paths).await.unwrap();
    let mut subscription = session
      .subscribe_state(request_id.to_string())
      .await
      .unwrap();

    let error = subscription.next_event().await.unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseRead,
      "unexpected_eof",
      Some(request_id),
    );
    let terminal_error = subscription.next_event().await.unwrap_err();
    assert_local_error(
      &terminal_error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::Connect,
      "connection_closed",
      Some(request_id),
    );
    assert!(terminal_error.retryable());
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_cancelled_subscription_read_retires_transport() {
    let (subscription_received_tx, subscription_received_rx) = oneshot::channel();
    let server = ScriptedIpcServer::start(|conn| async move {
      let (read_half, _writer) = tokio::io::split(conn);
      let mut reader = BufReader::new(read_half);
      let mut line = String::new();
      reader.read_line(&mut line).await.unwrap();
      assert!(!line.is_empty());
      subscription_received_tx.send(()).unwrap();
      let mut trailing = Vec::new();
      reader.read_to_end(&mut trailing).await.unwrap();
      assert!(trailing.is_empty());
    });
    let request_id = "typed-cancelled-subscription-1";
    let session = CadderSession::connect(&server.paths).await.unwrap();
    let mut subscription = session
      .subscribe_state(request_id.to_string())
      .await
      .unwrap();
    subscription_received_rx
      .await
      .expect("server should receive the subscription request");
    let mut in_flight = Box::pin(subscription.next_event());

    tokio::select! {
      result = &mut in_flight => panic!("subscription unexpectedly completed: {result:?}"),
      _ = sleep(Duration::from_millis(10)) => {},
    }
    drop(in_flight);

    let terminal_error = subscription.next_event().await.unwrap_err();
    assert_local_error(
      &terminal_error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::Connect,
      "connection_closed",
      Some(request_id),
    );
    timeout(Duration::from_secs(1), server.finish())
      .await
      .expect("cancelled subscription read should close the transport");
  }

  #[tokio::test]
  async fn typed_error_subscription_rejects_unexpected_event_type() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_basic_response(&mut writer, message_types::QUERY_STATE_RESPONSE).await;
    });
    let request_id = "unexpected-subscription-type-1";
    let session = CadderSession::connect(&server.paths).await.unwrap();
    let mut subscription = session
      .subscribe_state(request_id.to_string())
      .await
      .unwrap();

    let error = subscription.next_event().await.unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseValidate,
      "protocol_violation",
      Some(request_id),
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_subscription_rejects_malformed_event_json() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (read_half, mut writer) = tokio::io::split(conn);
      let mut reader = BufReader::new(read_half);
      let mut line = String::new();
      reader.read_line(&mut line).await.unwrap();
      writer.write_all(b"{not-json}\n").await.unwrap();
      writer.flush().await.unwrap();
      let mut trailing = Vec::new();
      reader.read_to_end(&mut trailing).await.unwrap();
      assert!(trailing.is_empty());
    });
    let request_id = "malformed-subscription-event-1";
    let session = CadderSession::connect(&server.paths).await.unwrap();
    let mut subscription = session
      .subscribe_state(request_id.to_string())
      .await
      .unwrap();

    let error = subscription.next_event().await.unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseDecode,
      "frame",
      Some(request_id),
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_subscription_rejects_event_payload_shape() {
    let request_id = "invalid-subscription-event-1";
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_envelope(
        &mut writer,
        message_types::STATE_CHANGED_EVENT,
        &BasicResponse {
          request_id: "invalid-subscription-event-1".to_string(),
          accepted: true,
          message: "Not an event.".to_string(),
        },
      )
      .await
      .unwrap();
    });
    let session = CadderSession::connect(&server.paths).await.unwrap();
    let mut subscription = session
      .subscribe_state(request_id.to_string())
      .await
      .unwrap();

    let error = subscription.next_event().await.unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::ResponseDecode,
      "frame",
      Some(request_id),
    );
    server.finish().await;
  }

  #[tokio::test]
  #[ignore = "test helper still waits for removed discovery publication"]
  async fn peer_identity_mismatch_closes_without_protocol_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::with_runtime_paths(
      CaddyConfigCoordinator::new_mock(paths.clone()),
      paths.clone(),
    )
    .await
    .unwrap();
    let logs = state.logs();
    let denied_identity = "other-token=secret";
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon = tokio::spawn(
      DaemonServer::new(paths.clone(), state)
        .with_peer_principal(IpcPrincipal::test(
          denied_identity,
          PrivilegeStatus::NormalUser,
        ))
        .run_until(shutdown_rx),
    );
    wait_for_discovery(&paths).await;

    let endpoint = discover_ipc_endpoint(&paths).unwrap();
    assert_peer_denied_without_request(&paths).await;

    for _ in 0..50 {
      if logs
        .query(
          LogQuery {
            stream: LogStreamIdentity::runtime_control(),
            limit: 10,
            after_sequence: None,
            minimum_severity: Some(LogSeverity::Warn),
          },
          true,
        )
        .entries
        .iter()
        .any(|entry| entry.operation.as_deref() == Some("ipc-peer-denied"))
      {
        break;
      }
      sleep(Duration::from_millis(10)).await;
    }
    let denial_log = logs.query(
      LogQuery {
        stream: LogStreamIdentity::runtime_control(),
        limit: 10,
        after_sequence: None,
        minimum_severity: Some(LogSeverity::Warn),
      },
      true,
    );

    assert!(matches!(
      endpoint.endpoint,
      IpcEndpoint::UnixSocket { .. } | IpcEndpoint::WindowsNamedPipe { .. }
    ));
    assert!(denial_log.entries.iter().any(|entry| {
      entry.operation.as_deref() == Some("ipc-peer-denied")
        && entry
          .raw_message
          .contains("principal-outside-runtime-owner")
        && !entry.raw_message.contains(denied_identity)
        && !entry.raw_message.contains("secret")
        && !entry
          .raw_message
          .contains(message_types::SET_AUTOSTART_REQUEST)
    }));

    shutdown_tx.send(true).unwrap();
    timeout(Duration::from_secs(2), daemon)
      .await
      .unwrap()
      .unwrap()
      .unwrap();
  }

  #[tokio::test]
  #[ignore = "test helper still waits for removed discovery publication"]
  async fn peer_identity_failure_does_not_stop_listener() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::with_runtime_paths(
      CaddyConfigCoordinator::new_mock(paths.clone()),
      paths.clone(),
    )
    .await
    .unwrap();
    let logs = state.logs();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon = tokio::spawn(
      DaemonServer::new(paths.clone(), state)
        .with_peer_identity_failure(io::ErrorKind::PermissionDenied)
        .run_until(shutdown_rx),
    );
    wait_for_discovery(&paths).await;

    assert_peer_denied_without_request(&paths).await;
    assert_peer_denied_without_request(&paths).await;

    for _ in 0..50 {
      let denial_count = logs
        .query(
          LogQuery {
            stream: LogStreamIdentity::runtime_control(),
            limit: 10,
            after_sequence: None,
            minimum_severity: Some(LogSeverity::Warn),
          },
          true,
        )
        .entries
        .iter()
        .filter(|entry| entry.operation.as_deref() == Some("ipc-peer-denied"))
        .count();
      if denial_count >= 2 {
        break;
      }
      sleep(Duration::from_millis(10)).await;
    }
    let denial_log = logs.query(
      LogQuery {
        stream: LogStreamIdentity::runtime_control(),
        limit: 10,
        after_sequence: None,
        minimum_severity: Some(LogSeverity::Warn),
      },
      true,
    );
    assert!(denial_log.entries.iter().any(|entry| {
      entry.operation.as_deref() == Some("ipc-peer-denied")
        && entry.raw_message.contains("peer-identity-unavailable")
        && !entry.raw_message.contains("test peer identity failure")
    }));

    shutdown_tx.send(true).unwrap();
    timeout(Duration::from_secs(2), daemon)
      .await
      .unwrap()
      .unwrap()
      .unwrap();
  }

  #[tokio::test]
  async fn peer_identity_authenticates_once_per_connection_and_reaches_dispatch() {
    use std::sync::{
      Arc,
      atomic::{AtomicUsize, Ordering},
    };

    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::with_runtime_paths(
      CaddyConfigCoordinator::new_mock(paths.clone()),
      paths.clone(),
    )
    .await
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let owner = IpcPrincipal::current_process(crate::current_privilege_status()).unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon = tokio::spawn(
      DaemonServer::new(paths.clone(), state)
        .with_counting_peer_principal(owner, calls.clone())
        .run_until(shutdown_rx),
    );
    wait_for_ready(&paths).await;
    for _ in 0..50 {
      if calls.load(Ordering::SeqCst) > 0 {
        break;
      }
      sleep(Duration::from_millis(10)).await;
    }
    let baseline = calls.load(Ordering::SeqCst);

    let mut session = CadderSession::connect(&paths).await.unwrap();
    let response = session
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: new_request_id("peer-identity-count"),
        },
      )
      .await
      .unwrap();

    assert!(response.accepted);
    assert_eq!(calls.load(Ordering::SeqCst), baseline + 1);
    drop(session);
    shutdown_tx.send(true).unwrap();
    timeout(Duration::from_secs(2), daemon)
      .await
      .unwrap()
      .unwrap()
      .unwrap();
  }

  #[tokio::test]
  async fn ensure_daemon_running_returns_when_socket_already_available() {
    let server = ScriptedIpcServer::start(|_conn| async move {});

    ensure_daemon_running_with_options(
      &server.paths,
      DaemonLaunchOptions {
        explicit_daemon: Some(PathBuf::from("missing-cadderd")),
        real_caddy_override: Some(PathBuf::from("real-caddy")),
        ..DaemonLaunchOptions::default()
      },
    )
    .await
    .unwrap();

    server.finish().await;
  }

  #[tokio::test]
  #[ignore = "file launch ownership is replaced by the endpoint lease"]
  async fn ensure_daemon_running_waits_for_concurrent_launch_owner() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let _launch_lock = DaemonLaunchLock::try_acquire(&paths).unwrap().unwrap();
    let server = ScriptedIpcServer::start_after(
      paths.clone(),
      Duration::from_millis(50),
      |_conn| async move {},
    );

    ensure_daemon_running_with_options(
      &paths,
      DaemonLaunchOptions {
        explicit_daemon: Some(PathBuf::from("missing-cadderd")),
        real_caddy_override: None,
        ..DaemonLaunchOptions::default()
      },
    )
    .await
    .unwrap();

    server.finish().await;
  }

  #[tokio::test]
  async fn typed_error_ensure_daemon_running_takes_over_after_failed_concurrent_launch_owner() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let launch_lock = DaemonLaunchLock::try_acquire(&paths).unwrap().unwrap();
    let release_lock = tokio::spawn(async move {
      sleep(Duration::from_millis(40)).await;
      drop(launch_lock);
    });
    let missing_daemon = temp.path().join(if cfg!(windows) {
      "missing-cadderd.exe"
    } else {
      "missing-cadderd"
    });

    let error = ensure_daemon_running_with_options(
      &paths,
      DaemonLaunchOptions {
        explicit_daemon: Some(missing_daemon),
        real_caddy_override: None,
        ..DaemonLaunchOptions::default()
      },
    )
    .await
    .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::DaemonLaunch,
      "daemon_start_failed",
      None,
    );
    assert!(!error.retryable());
    release_lock.await.unwrap();
  }

  #[tokio::test]
  #[ignore = "file launch ownership is replaced by the endpoint lease"]
  async fn typed_error_concurrent_daemon_launch_readiness_timeout_is_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let _launch_lock = DaemonLaunchLock::try_acquire(&paths).unwrap().unwrap();

    let error = acquire_launch_lock_or_wait_for_ready_with_policy(&paths, 1, Duration::ZERO)
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Timeout,
      IpcClientPhase::DaemonReadiness,
      "timeout",
      None,
    );
    assert!(error.retryable());
  }

  #[tokio::test]
  async fn typed_error_ensure_daemon_running_reports_missing_explicit_daemon_start_failure() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let missing_daemon = temp.path().join(if cfg!(windows) {
      "missing-cadderd.exe"
    } else {
      "missing-cadderd"
    });

    let error = ensure_daemon_running(&paths, Some(missing_daemon))
      .await
      .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::DaemonLaunch,
      "daemon_start_failed",
      None,
    );
    let source = std::error::Error::source(
      error
        .local_error()
        .expect("spawn failure should be a local IPC error"),
    )
    .and_then(|source| source.downcast_ref::<io::Error>())
    .expect("spawn failure should retain its I/O source");
    assert_eq!(source.kind(), io::ErrorKind::NotFound);
    assert!(!error.retryable());
  }

  #[tokio::test]
  async fn typed_error_ensure_daemon_running_applies_launch_options_before_spawn_failure() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let missing_daemon = temp.path().join(if cfg!(windows) {
      "missing-cadderd.exe"
    } else {
      "missing-cadderd"
    });

    let error = ensure_daemon_running_with_options(
      &paths,
      DaemonLaunchOptions {
        explicit_daemon: Some(missing_daemon),
        real_caddy_override: Some(PathBuf::from("real-caddy")),
        ..DaemonLaunchOptions::default()
      },
    )
    .await
    .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::DaemonLaunch,
      "daemon_start_failed",
      None,
    );
    assert!(!error.retryable());
  }

  #[tokio::test]
  async fn typed_error_ensure_daemon_running_rejects_mock_backend_with_real_command_before_spawn() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let missing_daemon = temp.path().join(if cfg!(windows) {
      "missing-cadderd.exe"
    } else {
      "missing-cadderd"
    });

    let error = ensure_daemon_running_with_options(
      &paths,
      DaemonLaunchOptions {
        explicit_daemon: Some(missing_daemon),
        real_caddy_override: Some(PathBuf::from("real-caddy")),
        caddy_backend: Some(CaddyBackendMode::Mock),
        ..DaemonLaunchOptions::default()
      },
    )
    .await
    .unwrap_err();

    assert_local_error(
      &error,
      LocalIpcErrorKind::Transport,
      IpcClientPhase::DaemonLaunch,
      "invalid_input",
      None,
    );
    assert!(!error.retryable());
  }

  #[test]
  fn daemon_launch_options_default_to_background_mode() {
    assert_eq!(
      DaemonLaunchOptions::default().launch_mode,
      DaemonLaunchMode::Background
    );
  }

  #[test]
  fn daemon_process_config_distinguishes_background_from_foreground() {
    let background = DaemonProcessConfig::for_launch_mode(DaemonLaunchMode::Background);
    let foreground = DaemonProcessConfig::for_launch_mode(DaemonLaunchMode::ForegroundDiagnostic);

    assert_eq!(background.stdio, DaemonStdioMode::Null);
    assert_eq!(foreground.stdio, DaemonStdioMode::Inherit);
  }

  #[cfg(windows)]
  #[test]
  fn background_daemon_process_uses_no_terminal_windows_flags() {
    let background = DaemonProcessConfig::for_launch_mode(DaemonLaunchMode::Background);
    let foreground = DaemonProcessConfig::for_launch_mode(DaemonLaunchMode::ForegroundDiagnostic);

    assert_eq!(
      background.creation_flags,
      DETACHED_PROCESS | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
    );
    assert_eq!(foreground.creation_flags, 0);
  }

  #[cfg(unix)]
  #[test]
  fn background_daemon_process_starts_new_unix_session() {
    let background = DaemonProcessConfig::for_launch_mode(DaemonLaunchMode::Background);
    let foreground = DaemonProcessConfig::for_launch_mode(DaemonLaunchMode::ForegroundDiagnostic);

    assert!(background.starts_new_session);
    assert!(!foreground.starts_new_session);
  }

  struct RunningTestDaemon {
    paths: RuntimePaths,
    state: DaemonState,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<Result<()>>,
    _temp: tempfile::TempDir,
  }

  impl RunningTestDaemon {
    async fn start() -> Self {
      Self::start_with_limits(IpcLimits::default()).await
    }

    async fn start_with_limits(limits: IpcLimits) -> Self {
      let temp = tempfile::tempdir().unwrap();
      let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
      let state = DaemonState::with_runtime_paths(
        CaddyConfigCoordinator::new_mock(paths.clone()),
        paths.clone(),
      )
      .await
      .unwrap();
      let (shutdown, shutdown_rx) = watch::channel(false);
      let task = tokio::spawn(
        DaemonServer::new(paths.clone(), state.clone())
          .with_limits(limits)
          .run_until(shutdown_rx),
      );
      wait_for_ready(&paths).await;
      Self {
        paths,
        state,
        shutdown,
        task,
        _temp: temp,
      }
    }

    async fn stop(self) {
      self.shutdown.send(true).unwrap();
      timeout(Duration::from_secs(4), self.task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    }
  }

  async fn connect_transport(paths: &RuntimePaths) -> Stream {
    Stream::connect(local_socket_name(paths).unwrap())
      .await
      .unwrap()
  }

  async fn connect_authenticated_transport(paths: &RuntimePaths) -> Stream {
    let mut conn = connect_transport(paths).await;
    send_peer_authentication_preface(&mut conn).await.unwrap();
    conn
  }

  async fn connect_authenticated(paths: &RuntimePaths) -> Stream {
    let conn = connect_authenticated_transport(paths).await;
    let (read_half, mut writer) = tokio::io::split(conn);
    let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
    perform_client_handshake(&mut reader, &mut writer, paths)
      .await
      .unwrap();
    reader.into_inner().unsplit(writer)
  }

  async fn connect_session_eventually(paths: &RuntimePaths) -> CadderSession {
    for _ in 0..50 {
      match CadderSession::connect(paths).await {
        Ok(session) => return session,
        Err(_) => sleep(Duration::from_millis(10)).await,
      }
    }
    panic!("the daemon did not release an IPC connection permit");
  }

  async fn assert_transport_closes(mut conn: Stream) {
    let mut byte = [0_u8; 1];
    let result = timeout(Duration::from_secs(1), conn.read(&mut byte))
      .await
      .expect("daemon should close the rejected connection");
    assert!(result.is_err() || result.unwrap() == 0);
  }

  async fn read_raw_envelope(conn: &mut Stream) -> IpcEnvelope {
    let mut reader = BufReader::new(conn);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    assert!(!line.is_empty(), "daemon did not return an IPC response");
    serde_json::from_str(&line).unwrap()
  }

  async fn assert_typed_frame_error_then_eof(conn: Stream) {
    let (mut reader, _writer) = tokio::io::split(conn);
    let mut reader = BufReader::new(&mut reader);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    assert!(!line.is_empty(), "daemon did not return a frame error");
    let envelope: IpcEnvelope = serde_json::from_str(&line).unwrap();
    assert_eq!(
      envelope.message_type,
      message_types::PROTOCOL_ERROR_RESPONSE
    );
    let response: ProtocolErrorResponse = envelope.decode().unwrap();
    assert!(!response.accepted);
    assert_eq!(response.request_id, "unknown");
    assert_eq!(response.error.kind, ProtocolErrorKind::Frame);
    assert_eq!(response.error.code.as_str(), "frame");
    assert!(!response.error.retryable);

    let mut trailing = Vec::new();
    timeout(Duration::from_secs(1), reader.read_to_end(&mut trailing))
      .await
      .expect("daemon should close after a framing violation")
      .unwrap();
    assert!(trailing.is_empty());
  }

  struct ScriptedIpcServer {
    paths: RuntimePaths,
    task: JoinHandle<()>,
    _publication: Option<IpcEndpointPublication>,
    _temp: tempfile::TempDir,
  }

  impl ScriptedIpcServer {
    fn start<F, Fut>(handler: F) -> Self
    where
      F: FnOnce(Stream) -> Fut + Send + 'static,
      Fut: Future<Output = ()> + Send + 'static,
    {
      let temp = tempfile::tempdir().unwrap();
      let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
      let listener = scripted_listener(&paths);
      let metadata = IpcEndpointMetadata::new(&paths).unwrap();
      let publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();
      let handshake_identity = ServerHandshakeIdentity::from(&metadata);
      let task = tokio::spawn(async move {
        let mut conn = listener.accept().await.unwrap();
        receive_peer_authentication_preface(&mut conn)
          .await
          .unwrap();
        let conn = accept_scripted_handshake(conn, &handshake_identity).await;
        handler(conn).await;
      });
      Self {
        paths,
        task,
        _publication: Some(publication),
        _temp: temp,
      }
    }

    fn start_without_handshake<F, Fut>(handler: F) -> Self
    where
      F: FnOnce(Stream) -> Fut + Send + 'static,
      Fut: Future<Output = ()> + Send + 'static,
    {
      let temp = tempfile::tempdir().unwrap();
      let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
      let listener = scripted_listener(&paths);
      let metadata = IpcEndpointMetadata::new(&paths).unwrap();
      let publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();
      let task = tokio::spawn(async move {
        let mut conn = listener.accept().await.unwrap();
        receive_peer_authentication_preface(&mut conn)
          .await
          .unwrap();
        handler(conn).await;
      });
      Self {
        paths,
        task,
        _publication: Some(publication),
        _temp: temp,
      }
    }

    fn start_after<F, Fut>(paths: RuntimePaths, delay: Duration, handler: F) -> Self
    where
      F: FnOnce(Stream) -> Fut + Send + 'static,
      Fut: Future<Output = ()> + Send + 'static,
    {
      let temp = tempfile::tempdir().unwrap();
      let server_paths = paths.clone();
      let task = tokio::spawn(async move {
        if !delay.is_zero() {
          sleep(delay).await;
        }
        let listener = scripted_listener(&server_paths);
        let metadata = IpcEndpointMetadata::new(&server_paths).unwrap();
        let _publication = IpcEndpointPublication::publish(&server_paths, &metadata).unwrap();
        let handshake_identity = ServerHandshakeIdentity::from(&metadata);
        let mut conn = listener.accept().await.unwrap();
        receive_peer_authentication_preface(&mut conn)
          .await
          .unwrap();
        let conn = accept_scripted_handshake(conn, &handshake_identity).await;
        handler(conn).await;
      });
      Self {
        paths,
        task,
        _publication: None,
        _temp: temp,
      }
    }

    async fn finish(self) {
      self.task.await.unwrap();
    }

    async fn abort(self) {
      self.task.abort();
      let _ = self.task.await;
    }
  }

  async fn accept_scripted_handshake(conn: Stream, identity: &ServerHandshakeIdentity) -> Stream {
    let (read_half, mut writer) = tokio::io::split(conn);
    let mut reader = FramedRead::new(read_half, BoundedNdjsonCodec::new());
    assert!(
      accept_client_handshake(
        &mut reader,
        &mut writer,
        identity,
        Instant::now(),
        IpcLimits::default(),
      )
      .await
      .unwrap()
      .is_some(),
      "scripted client handshake should be accepted"
    );
    reader.into_inner().unsplit(writer)
  }

  fn scripted_listener(paths: &RuntimePaths) -> interprocess::local_socket::tokio::Listener {
    paths.ensure_dirs().unwrap();
    let owner = IpcPrincipal::current_process(crate::current_privilege_status()).unwrap();
    let name = local_socket_name(paths).unwrap();
    let listener = secure_listener_options(
      ListenerOptions::new().name(name).try_overwrite(true),
      &owner,
    )
    .unwrap()
    .create_tokio()
    .unwrap();
    secure_bound_socket(paths).unwrap();
    listener
  }

  fn short_client_deadlines() -> IpcClientDeadlines {
    IpcClientDeadlines {
      connect: Duration::from_secs(1),
      ordinary: Duration::from_millis(25),
      reload: Duration::from_millis(25),
      stream: Duration::from_millis(25),
      shutdown: Duration::from_millis(25),
    }
  }

  fn assert_daemon_error(
    error: &IpcClientError,
    kind: ProtocolErrorKind,
    code: &str,
    request_id: &str,
    message: &str,
    guidance: Option<&str>,
    retryable: bool,
  ) {
    let daemon = error
      .daemon_error()
      .unwrap_or_else(|| panic!("expected a daemon protocol error, got {error:?}"));
    assert_eq!(daemon.kind, kind);
    assert_eq!(error.code(), code);
    assert_eq!(error.request_id().map(RequestId::as_str), Some(request_id));
    assert_eq!(error.message(), message);
    assert_eq!(error.guidance(), guidance);
    assert_eq!(error.retryable(), retryable);
    assert!(error.local_error().is_none());
  }

  fn assert_local_error(
    error: &IpcClientError,
    kind: LocalIpcErrorKind,
    phase: IpcClientPhase,
    code: &str,
    request_id: Option<&str>,
  ) {
    let local = error.local_error().expect("expected a local IPC error");
    assert_eq!(local.kind(), kind);
    assert_eq!(local.phase(), phase);
    assert_eq!(local.code().as_str(), code);
    assert_eq!(local.request_id().map(RequestId::as_str), request_id);
    assert!(error.daemon_error().is_none());
  }

  async fn read_one_request(conn: Stream) -> (String, tokio::io::WriteHalf<Stream>) {
    let (read_half, writer) = tokio::io::split(conn);
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    assert!(!line.is_empty(), "client did not send an IPC request");
    (line, writer)
  }

  async fn write_protocol_error(
    writer: &mut tokio::io::WriteHalf<Stream>,
    request_id: &str,
    error: ProtocolError,
  ) {
    let response =
      ProtocolErrorResponse::rejected(Some(RequestId::parse(request_id).unwrap()), error);
    write_envelope(writer, message_types::PROTOCOL_ERROR_RESPONSE, &response)
      .await
      .unwrap();
  }

  async fn wait_for_ready(paths: &RuntimePaths) {
    for _ in 0..50 {
      if daemon_is_ready(paths).await.unwrap_or(false) {
        return;
      }
      sleep(Duration::from_millis(20)).await;
    }

    panic!("daemon server did not become ready");
  }

  async fn wait_for_discovery(paths: &RuntimePaths) {
    for _ in 0..50 {
      if discover_ipc_endpoint(paths).is_ok() {
        return;
      }
      sleep(Duration::from_millis(20)).await;
    }

    panic!("daemon server did not publish IPC discovery");
  }

  async fn assert_peer_denied_without_request(paths: &RuntimePaths) {
    let name = local_socket_name(paths).unwrap();
    let mut conn = Stream::connect(name).await.unwrap();
    send_peer_authentication_preface(&mut conn).await.unwrap();
    let mut byte = [0_u8; 1];
    let read = timeout(Duration::from_secs(1), conn.read(&mut byte))
      .await
      .expect("peer denial should not wait for a protocol request")
      .unwrap();
    assert_eq!(read, 0, "peer denial should close without a response");
  }

  async fn write_basic_response(writer: &mut tokio::io::WriteHalf<Stream>, message_type: &str) {
    write_envelope(
      writer,
      message_type,
      &BasicResponse {
        request_id: "test-response".to_string(),
        accepted: true,
        message: "ok".to_string(),
      },
    )
    .await
    .unwrap();
  }

  struct EnvSnapshot {
    key: &'static str,
    value: Option<OsString>,
  }

  impl EnvSnapshot {
    fn capture(key: &'static str) -> Self {
      Self {
        key,
        value: env::var_os(key),
      }
    }
  }

  impl Drop for EnvSnapshot {
    fn drop(&mut self) {
      unsafe {
        match &self.value {
          Some(value) => env::set_var(self.key, value),
          None => env::remove_var(self.key),
        }
      }
    }
  }

  fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    crate::TEST_ENV_LOCK
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner())
  }
}
