use crate::{
  CaddyBackendMode, DaemonState, IpcClientError, IpcClientPhase, IpcClientResult, IpcOperation,
  IpcPrincipal, IpcSecurityPolicy, LocalIpcErrorCode, LocalIpcErrorKind, RuntimePaths,
  ipc_client_error::LocalIpcErrorContext,
  ipc_codec::{BoundedNdjsonCodec, IpcCodecError, encode_json_frame},
  ipc_security::{
    IpcPeerIdentityResolver, receive_peer_authentication_preface, secure_bound_socket,
    secure_listener_options, send_peer_authentication_preface,
  },
  operation_fence::{CommitRejection, OperationFence, RevokeOutcome},
};
use anyhow::{Context, Result};
use cadder_ipc::{
  AuthorizedRequestEnvelope, BasicResponse, CLIENT_HELLO_OPERATION, CURRENT_PROTOCOL_VERSION,
  ClientHello, CorrelatedRequest, HeartbeatEntrypointPayload, LogAttributionKind, LogSeverity,
  LogStreamIdentity, OPERATION_REGISTRY, OperationAccess, OperationDeadlineClass, ProtocolError,
  ProtocolErrorCode, ProtocolErrorKind, ProtocolErrorResponse, ProtocolVersion, QueryLogsPayload,
  QueryStatePayload, RawRequestEnvelope, RegisterEntrypointPayload, RequestId,
  ServerHandshakeFrame, ServerHello, SetDomainEnabledPayload, SetEntrypointEnabledPayload,
  ShutdownDaemonPayload, UnregisterEntrypointPayload, message_types, new_request_id,
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
  collections::BTreeMap,
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
  time::{Instant, sleep, sleep_until, timeout, timeout_at},
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
}

#[derive(Debug, Clone)]
struct ServerHandshakeIdentity {
  runtime_id: Box<str>,
  daemon_instance_id: Box<str>,
}

trait HandshakeRuntime {
  fn runtime_id(&self) -> &str;
}

impl HandshakeRuntime for RuntimePaths {
  fn runtime_id(&self) -> &str {
    self.instance_key()
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
    })
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
  shutdown_operation: Duration,
  shutdown_accept: Duration,
  shutdown_handler_grace: Duration,
  shutdown_connection_abort_join: Duration,
  shutdown_runtime: Duration,
  shutdown_storage: Duration,
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
      shutdown_operation: Duration::from_secs(30),
      shutdown_accept: Duration::from_secs(2),
      shutdown_handler_grace: Duration::from_secs(8),
      shutdown_connection_abort_join: Duration::from_secs(1),
      shutdown_runtime: Duration::from_secs(10),
      shutdown_storage: Duration::from_secs(5),
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
        if endpoint_listener_exists(paths).await {
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

async fn endpoint_listener_exists(paths: &RuntimePaths) -> bool {
  let Ok(name) = local_socket_name(paths) else {
    return false;
  };
  match timeout(Duration::from_millis(250), Stream::connect(name)).await {
    Ok(Ok(_)) => true,
    Ok(Err(error))
      if matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
      ) =>
    {
      false
    }
    Ok(Err(_)) | Err(_) => true,
  }
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
              "IPC handlers did not finish within the shutdown grace"
            ));
            self
              .state
              .logs()
              .append(
                LogStreamIdentity::runtime_control(),
                LogSeverity::Error,
                "An owned mutation exceeded the shutdown grace; daemon shutdown continues.",
                LogAttributionKind::RuntimeControl,
                Some("shutdown-mutation-timeout".to_string()),
              )
              .await;
          }
        }
      }
    }
    let runtime_deadline = shutdown_timeline.runtime_deadline(Instant::now());
    let runtime_shutdown = self.state.prepare_shutdown_until(runtime_deadline).await;
    if !runtime_shutdown.runtime_quiescent {
      timeout(
        self.limits.shutdown_runtime,
        self.state.force_stop_runtime(),
      )
      .await
      .context("force-stop owned Caddy runtime exceeded its bounded cleanup budget")?
      .context("force-stop owned Caddy runtime after bounded shutdown")?;
    }
    let storage_deadline = shutdown_timeline.storage_deadline(Instant::now());
    let storage_shutdown = self.state.shutdown_storage_until(storage_deadline).await;
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

fn mutation_fence(fence: Option<&OperationFence>) -> Result<&OperationFence> {
  fence.context("mutation dispatcher requires an operation fence")
}

mod authorization;
mod client;
mod connection;
mod control;
mod daemon_launch;
mod dispatch;
mod framing;
mod handshake;
mod supervision;

use authorization::{ConnectionOwnership, authorize_or_reject};
pub use client::{CadderClient, CadderSession};
use client::{IpcClientDeadlines, daemon_launch_error, daemon_readiness_timeout};
use connection::*;
use control::*;
pub(crate) use daemon_launch::is_daemon_ready;
pub use daemon_launch::{
  DaemonLaunchMode, DaemonLaunchOptions, ensure_daemon_running, ensure_daemon_running_with_options,
};
use dispatch::dispatch_authorized_request;
use framing::*;
use handshake::*;
use supervision::*;

#[cfg(test)]
mod tests;
