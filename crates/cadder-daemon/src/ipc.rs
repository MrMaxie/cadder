use crate::{
  CaddyBackendMode, DaemonState, IpcEndpointMetadata, IpcEndpointPublication, IpcOperation,
  IpcPrincipal, IpcSecurityPolicy, RuntimePaths, RuntimeProfile,
};
use anyhow::{Context, Result, anyhow};
use cadder_protocol::{
  HeartbeatEntrypointRequest, IpcEnvelope, LogAttributionKind, LogSeverity, LogStreamIdentity,
  ProtocolError, ProtocolErrorResponse, QueryAutostartRequest, QueryIisBindingsRequest,
  QueryLogsRequest, QueryStateRequest, RegisterEntrypointRequest, SetAutostartRequest,
  SetDomainEnabledRequest, SetEntrypointEnabledRequest, SetIisHandoffRequest,
  ShutdownDaemonRequest, StateChangedEvent, SubscribeStateRequest, UnregisterEntrypointRequest,
  message_types,
};
use fs4::{FileExt, TryLockError};
use interprocess::local_socket::{
  GenericNamespaced, ListenerOptions, ToNsName,
  tokio::{Stream, prelude::*},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
  collections::BTreeMap,
  env,
  fs::{File, OpenOptions, create_dir_all},
  io,
  path::PathBuf,
  process::Stdio,
  time::Duration,
};
use tokio::{
  io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
  process::Command,
  sync::watch,
  time::sleep,
};

#[derive(Debug)]
pub struct DaemonServer {
  paths: RuntimePaths,
  state: DaemonState,
  security_policy: IpcSecurityPolicy,
  peer_principal_override: Option<IpcPrincipal>,
}

impl DaemonServer {
  pub fn new(paths: RuntimePaths, state: DaemonState) -> Self {
    Self {
      paths,
      state,
      security_policy: IpcSecurityPolicy,
      peer_principal_override: None,
    }
  }

  pub fn with_peer_principal(mut self, peer_principal: IpcPrincipal) -> Self {
    self.peer_principal_override = Some(peer_principal);
    self
  }

  pub async fn run_until(self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    let name = self.paths.socket_name().to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new()
      .name(name)
      .try_overwrite(true)
      .create_tokio()
      .context("create local IPC listener")?;
    let endpoint = IpcEndpointMetadata::current(&self.paths);
    let _endpoint_publication = IpcEndpointPublication::publish(&self.paths, &endpoint)?;
    let shutdown_signal = self.state.shutdown_signal();

    loop {
      tokio::select! {
          _ = shutdown_signal.wait() => break,
          changed = shutdown.changed() => {
              if changed.is_ok() && *shutdown.borrow() {
                  break;
              }
          }
          accepted = listener.accept() => {
              match accepted {
                  Ok(conn) => {
                      let state = self.state.clone();
                      let peer_principal = self
                          .peer_principal_override
                          .clone()
                          .unwrap_or_else(|| IpcPrincipal::from_peer_credentials(conn.peer_creds().ok()));
                      let security = ConnectionSecurityContext {
                          endpoint: endpoint.clone(),
                          policy: self.security_policy.clone(),
                          peer_principal,
                      };
                      tokio::spawn(async move {
                          let _ = handle_connection(conn, state, security).await;
                      });
                  }
                  Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                  Err(error) => return Err(error).context("accept local IPC connection"),
              }
          }
      }
    }

    Ok(())
  }
}

#[derive(Debug, Clone)]
struct ConnectionSecurityContext {
  endpoint: IpcEndpointMetadata,
  policy: IpcSecurityPolicy,
  peer_principal: IpcPrincipal,
}

async fn handle_connection(
  conn: Stream,
  state: DaemonState,
  security: ConnectionSecurityContext,
) -> Result<()> {
  let mut owned = ConnectionRegistrations::default();
  let result = handle_connection_loop(conn, state.clone(), &mut owned, &security).await;
  for (id, nonce) in owned.into_entries() {
    state.unregister_for_ipc_disconnect(&id, &nonce).await;
  }
  result
}

async fn handle_connection_loop(
  conn: Stream,
  state: DaemonState,
  owned: &mut ConnectionRegistrations,
  security: &ConnectionSecurityContext,
) -> Result<()> {
  let (read_half, mut write_half) = tokio::io::split(conn);
  let mut reader = BufReader::new(read_half);
  let mut line = String::new();
  macro_rules! send_response {
    ($message_type:expr, $response:expr) => {
      write_envelope(&mut write_half, $message_type, &$response).await?;
    };
  }
  macro_rules! decode_request {
    ($envelope:expr, $request:ty) => {
      match decode_or_reject::<$request, _>(&mut write_half, $envelope).await? {
        Some(request) => request,
        None => continue,
      }
    };
  }

  loop {
    line.clear();
    let read = reader.read_line(&mut line).await?;
    if read == 0 {
      break;
    }

    let envelope: IpcEnvelope = match serde_json::from_str(line.trim_end()) {
      Ok(envelope) => envelope,
      Err(error) => {
        let response = ProtocolErrorResponse::rejected(
          "unparseable",
          ProtocolError::payload_decode_failed(error),
        );
        send_response!(message_types::PROTOCOL_ERROR_RESPONSE, response);
        continue;
      }
    };
    if !authorize_or_reject(&mut write_half, &state, &envelope, security).await? {
      continue;
    }
    match envelope.message_type.as_str() {
      message_types::REGISTER_ENTRYPOINT_REQUEST => {
        let request = decode_request!(&envelope, RegisterEntrypointRequest);
        let nonce = request
          .registration
          .entrypoint_instance
          .shim_session_nonce
          .clone();
        let response = state
          .register(request.request_id, request.registration)
          .await;
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
        let request = decode_request!(&envelope, UnregisterEntrypointRequest);
        let response = state
          .unregister(
            request.request_id,
            &request.registration_id,
            &request.shim_session_nonce,
          )
          .await;
        if response.accepted {
          owned.remove(&request.registration_id);
        }
        send_response!(message_types::UNREGISTER_ENTRYPOINT_RESPONSE, response);
      }
      message_types::HEARTBEAT_ENTRYPOINT_REQUEST => {
        let request = decode_request!(&envelope, HeartbeatEntrypointRequest);
        let response = state.heartbeat(request).await;
        send_response!(message_types::HEARTBEAT_ENTRYPOINT_RESPONSE, response);
      }
      message_types::QUERY_STATE_REQUEST => {
        let request = decode_request!(&envelope, QueryStateRequest);
        let response = state.query_state(request.request_id).await;
        send_response!(message_types::QUERY_STATE_RESPONSE, response);
      }
      message_types::SET_ENTRYPOINT_ENABLED_REQUEST => {
        let request = decode_request!(&envelope, SetEntrypointEnabledRequest);
        let response = state.set_entrypoint_enabled(request).await;
        send_response!(message_types::SET_ENTRYPOINT_ENABLED_RESPONSE, response);
      }
      message_types::SET_DOMAIN_ENABLED_REQUEST => {
        let request = decode_request!(&envelope, SetDomainEnabledRequest);
        let response = state.set_domain_enabled(request).await;
        send_response!(message_types::SET_DOMAIN_ENABLED_RESPONSE, response);
      }
      message_types::QUERY_IIS_BINDINGS_REQUEST => {
        let request = decode_request!(&envelope, QueryIisBindingsRequest);
        let response = state.query_iis_bindings(request.request_id).await;
        send_response!(message_types::QUERY_IIS_BINDINGS_RESPONSE, response);
      }
      message_types::SET_IIS_HANDOFF_REQUEST => {
        let request = decode_request!(&envelope, SetIisHandoffRequest);
        let response = state.set_iis_handoff(request).await;
        send_response!(message_types::SET_IIS_HANDOFF_RESPONSE, response);
      }
      message_types::QUERY_LOGS_REQUEST => {
        let request = decode_request!(&envelope, QueryLogsRequest);
        let response = state.query_logs(request).await;
        send_response!(message_types::QUERY_LOGS_RESPONSE, response);
      }
      message_types::QUERY_HISTORY_REQUEST => {
        let request = decode_request!(&envelope, cadder_protocol::QueryHistoryRequest);
        let response = state.query_history(request).await;
        send_response!(message_types::QUERY_HISTORY_RESPONSE, response);
      }
      message_types::QUERY_AUTOSTART_REQUEST => {
        let request = decode_request!(&envelope, QueryAutostartRequest);
        let response = state.query_autostart(request.request_id).await;
        send_response!(message_types::QUERY_AUTOSTART_RESPONSE, response);
      }
      message_types::SET_AUTOSTART_REQUEST => {
        let request = decode_request!(&envelope, SetAutostartRequest);
        let response = state.set_autostart(request).await;
        send_response!(message_types::SET_AUTOSTART_RESPONSE, response);
      }
      message_types::SUBSCRIBE_STATE_REQUEST => {
        let request = decode_request!(&envelope, SubscribeStateRequest);
        let snapshot = state.snapshot().await;
        let initial = cadder_protocol::StateChangedEvent {
          request_id: request.request_id.clone(),
          sequence_number: 0,
          change_kind: cadder_protocol::StateChangeKind::Snapshot,
          snapshot,
          registration_id: None,
        };
        send_response!(message_types::STATE_CHANGED_EVENT, initial);
        let mut subscription = state.subscribe();
        while let Ok(mut event) = subscription.recv().await {
          event.request_id = request.request_id.clone();
          send_response!(message_types::STATE_CHANGED_EVENT, event);
        }
      }
      message_types::SHUTDOWN_DAEMON_REQUEST => {
        let request = decode_request!(&envelope, ShutdownDaemonRequest);
        let mut response = state.shutdown().await;
        response.request_id = request.request_id;
        send_response!(message_types::SHUTDOWN_DAEMON_RESPONSE, response);
        break;
      }
      other => {
        let response = ProtocolErrorResponse::rejected(
          request_id_from_payload(&envelope),
          ProtocolError::unsupported_capability(
            format!("message-type:{other}"),
            cadder_protocol::current_capabilities(),
          ),
        );
        send_response!(message_types::PROTOCOL_ERROR_RESPONSE, response);
      }
    }
  }

  Ok(())
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
  let decision = security
    .policy
    .evaluate(&security.endpoint, &security.peer_principal, &operation);
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
  match message_type {
    message_types::REGISTER_ENTRYPOINT_REQUEST
    | message_types::UNREGISTER_ENTRYPOINT_REQUEST
    | message_types::HEARTBEAT_ENTRYPOINT_REQUEST
    | message_types::SET_ENTRYPOINT_ENABLED_REQUEST
    | message_types::SET_DOMAIN_ENABLED_REQUEST
    | message_types::SET_IIS_HANDOFF_REQUEST
    | message_types::SET_AUTOSTART_REQUEST
    | message_types::SHUTDOWN_DAEMON_REQUEST => IpcOperation::state_changing(message_type),
    _ => IpcOperation::read_only(message_type),
  }
}

#[derive(Debug, Default)]
struct ConnectionRegistrations {
  by_registration_id: BTreeMap<String, String>,
}

impl ConnectionRegistrations {
  fn insert(&mut self, registration_id: String, shim_session_nonce: String) {
    self
      .by_registration_id
      .insert(registration_id, shim_session_nonce);
  }

  fn remove(&mut self, registration_id: &str) {
    self.by_registration_id.remove(registration_id);
  }

  fn into_entries(self) -> impl Iterator<Item = (String, String)> {
    self.by_registration_id.into_iter()
  }
}

async fn write_envelope<W, T>(writer: &mut W, message_type: &str, payload: &T) -> Result<()>
where
  W: AsyncWrite + Unpin,
  T: Serialize,
{
  let envelope = IpcEnvelope::new(message_type, payload)?;
  let rendered = serde_json::to_string(&envelope)?;
  writer.write_all(rendered.as_bytes()).await?;
  writer.write_all(b"\n").await?;
  writer.flush().await?;
  Ok(())
}

async fn decode_or_reject<T, W>(writer: &mut W, envelope: &IpcEnvelope) -> Result<Option<T>>
where
  T: DeserializeOwned,
  W: AsyncWrite + Unpin,
{
  match envelope.decode_typed() {
    Ok(request) => Ok(Some(request)),
    Err(error) => {
      let response = ProtocolErrorResponse::rejected(request_id_from_payload(envelope), error);
      write_envelope(writer, message_types::PROTOCOL_ERROR_RESPONSE, &response).await?;
      Ok(None)
    }
  }
}

fn request_id_from_payload(envelope: &IpcEnvelope) -> String {
  envelope
    .payload
    .get("requestId")
    .and_then(|value| value.as_str())
    .filter(|request_id| !request_id.is_empty())
    .unwrap_or("unknown")
    .to_string()
}

#[derive(Debug, Clone)]
pub struct CadderClient {
  paths: RuntimePaths,
}

impl CadderClient {
  pub fn new(paths: RuntimePaths) -> Self {
    Self { paths }
  }

  pub fn from_env() -> Result<Self> {
    Ok(Self::new(RuntimePaths::resolve(None)?))
  }

  pub async fn request<TRequest, TResponse>(
    &self,
    message_type: &str,
    response_type: &str,
    request: &TRequest,
  ) -> Result<TResponse>
  where
    TRequest: Serialize,
    TResponse: DeserializeOwned,
  {
    let name = self.paths.socket_name().to_ns_name::<GenericNamespaced>()?;
    let mut session =
      CadderSession::connect_name(name, self.paths.socket_name().to_string()).await?;
    session.request(message_type, response_type, request).await
  }

  pub async fn subscribe_state(&self, request_id: String) -> Result<StateSubscription> {
    let name = self.paths.socket_name().to_ns_name::<GenericNamespaced>()?;
    let session = CadderSession::connect_name(name, self.paths.socket_name().to_string()).await?;
    session.subscribe_state(request_id).await
  }
}

#[derive(Debug)]
pub struct CadderSession {
  reader: BufReader<tokio::io::ReadHalf<Stream>>,
  writer: tokio::io::WriteHalf<Stream>,
}

impl CadderSession {
  pub async fn connect(paths: &RuntimePaths) -> Result<Self> {
    let name = paths.socket_name().to_ns_name::<GenericNamespaced>()?;
    Self::connect_name(name, paths.socket_name().to_string()).await
  }

  async fn connect_name(
    name: interprocess::local_socket::Name<'_>,
    display_name: String,
  ) -> Result<Self> {
    let conn = Stream::connect(name)
      .await
      .with_context(|| format!("connect to Cadder daemon socket {display_name}"))?;
    let (read_half, writer) = tokio::io::split(conn);
    Ok(Self {
      reader: BufReader::new(read_half),
      writer,
    })
  }

  pub async fn request<TRequest, TResponse>(
    &mut self,
    message_type: &str,
    response_type: &str,
    request: &TRequest,
  ) -> Result<TResponse>
  where
    TRequest: Serialize,
    TResponse: DeserializeOwned,
  {
    write_envelope(&mut self.writer, message_type, request).await?;
    let mut line = String::new();
    self.reader.read_line(&mut line).await?;
    if line.is_empty() {
      return Err(
        io::Error::new(
          io::ErrorKind::UnexpectedEof,
          "daemon closed the IPC connection without a response",
        )
        .into(),
      );
    }
    let envelope: IpcEnvelope = serde_json::from_str(line.trim_end())?;
    if envelope.message_type == message_types::PROTOCOL_ERROR_RESPONSE
      && response_type != message_types::PROTOCOL_ERROR_RESPONSE
    {
      let response: ProtocolErrorResponse = envelope.decode()?;
      return Err(anyhow!(
        "daemon rejected IPC request `{}`: {}",
        response.request_id,
        response.error
      ));
    }
    if envelope.message_type != response_type {
      return Err(anyhow!(
        "unexpected response type `{}`, expected `{response_type}`",
        envelope.message_type
      ));
    }
    Ok(envelope.decode()?)
  }

  pub async fn subscribe_state(self, request_id: String) -> Result<StateSubscription> {
    let mut subscription = StateSubscription {
      reader: self.reader,
      writer: self.writer,
    };
    write_envelope(
      &mut subscription.writer,
      message_types::SUBSCRIBE_STATE_REQUEST,
      &SubscribeStateRequest { request_id },
    )
    .await?;
    Ok(subscription)
  }
}

#[derive(Debug)]
pub struct StateSubscription {
  reader: BufReader<tokio::io::ReadHalf<Stream>>,
  writer: tokio::io::WriteHalf<Stream>,
}

impl StateSubscription {
  pub async fn next_event(&mut self) -> Result<StateChangedEvent> {
    let mut line = String::new();
    self.reader.read_line(&mut line).await?;
    if line.is_empty() {
      return Err(
        io::Error::new(
          io::ErrorKind::UnexpectedEof,
          "daemon closed the state subscription",
        )
        .into(),
      );
    }
    let envelope: IpcEnvelope = serde_json::from_str(line.trim_end())?;
    if envelope.message_type == message_types::PROTOCOL_ERROR_RESPONSE {
      let response: ProtocolErrorResponse = envelope.decode()?;
      return Err(anyhow!(
        "daemon rejected state subscription `{}`: {}",
        response.request_id,
        response.error
      ));
    }
    if envelope.message_type != message_types::STATE_CHANGED_EVENT {
      return Err(anyhow!(
        "unexpected response type `{}`, expected `{}`",
        envelope.message_type,
        message_types::STATE_CHANGED_EVENT
      ));
    }
    Ok(envelope.decode()?)
  }
}

#[derive(Debug, Clone, Default)]
pub struct DaemonLaunchOptions {
  pub explicit_daemon: Option<PathBuf>,
  pub runtime_profile: Option<RuntimeProfile>,
  pub real_caddy_command: Option<String>,
  pub caddy_backend: Option<CaddyBackendMode>,
  pub shim_path: Option<PathBuf>,
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
) -> Result<()> {
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
) -> Result<()> {
  if can_connect(paths).await {
    return Ok(());
  }

  let Some(_launch_lock) = acquire_launch_lock_or_wait_for_ready(paths).await? else {
    return Ok(());
  };

  if can_connect(paths).await {
    return Ok(());
  }

  let daemon = options
    .explicit_daemon
    .or_else(|| sibling_binary("cadderd"))
    .or_else(|| find_on_path("cadderd"))
    .ok_or_else(|| anyhow!("could not find `cadderd`; pass --daemon-path or add it to PATH"))?;

  let process_config = DaemonProcessConfig::for_launch_mode(options.launch_mode);
  let caddy_backend = options
    .caddy_backend
    .map_or_else(CaddyBackendMode::from_env, Ok)?;
  if caddy_backend == CaddyBackendMode::Mock && options.real_caddy_command.is_some() {
    return Err(anyhow!(
      "--real-caddy-command cannot be combined with --caddy-backend mock"
    ));
  }
  let daemon_dir = daemon.parent().map(PathBuf::from);
  let mut command = Command::new(&daemon);
  process_config.configure_command(&mut command);
  if let Some(daemon_dir) = &daemon_dir {
    command.current_dir(daemon_dir);
    prepend_path_dir(&mut command, daemon_dir);
  }
  command
    .arg("--runtime-dir")
    .arg(paths.runtime_dir())
    .arg("--detach-ready");
  process_config.configure_stdio(&mut command);
  if let Some(real_caddy_command) = options.real_caddy_command {
    command.arg("--real-caddy-command").arg(real_caddy_command);
  }
  if caddy_backend != CaddyBackendMode::Real {
    command.arg("--caddy-backend").arg(caddy_backend.as_str());
  }
  if let Some(shim_path) = options.shim_path {
    command.env("CADDER_CADDY_SHIM_PATH", shim_path);
  }
  command.env("CADDER_RUNTIME_DIR", paths.runtime_dir());
  let mut child = command.spawn().context("start cadderd")?;

  wait_for_daemon_ready(paths, &mut child).await
}

async fn acquire_launch_lock_or_wait_for_ready(
  paths: &RuntimePaths,
) -> Result<Option<DaemonLaunchLock>> {
  for _ in 0..DAEMON_READY_ATTEMPTS {
    if can_connect(paths).await {
      return Ok(None);
    }
    if let Some(lock) = DaemonLaunchLock::try_acquire(paths)? {
      return Ok(Some(lock));
    }
    sleep(DAEMON_READY_POLL_INTERVAL).await;
  }

  if can_connect(paths).await {
    return Ok(None);
  }

  Err(anyhow!(
    "another cadderd launch still holds {} but no healthy daemon socket became available for runtime {}; wait for that launch to finish, then retry `cadder daemon start --runtime-dir \"{}\"`",
    paths.runtime_dir().join("cadder-launch.lock").display(),
    paths.runtime_dir().display(),
    paths.runtime_dir().display()
  ))
}

async fn wait_for_daemon_ready(
  paths: &RuntimePaths,
  child: &mut tokio::process::Child,
) -> Result<()> {
  for _ in 0..DAEMON_READY_ATTEMPTS {
    if can_connect(paths).await {
      return Ok(());
    }
    if let Some(status) = child.try_wait().context("inspect cadderd launch process")? {
      return Err(anyhow!(
        "cadderd exited before opening IPC for runtime {}; exit status: {status}",
        paths.runtime_dir().display()
      ));
    }
    sleep(DAEMON_READY_POLL_INTERVAL).await;
  }

  Err(anyhow!(
    "cadderd did not become ready within 30 seconds for runtime {}; the launch process is still running or left stale runtime residue",
    paths.runtime_dir().display()
  ))
}

pub(crate) async fn is_daemon_ready(paths: &RuntimePaths) -> bool {
  can_connect(paths).await
}

#[derive(Debug)]
struct DaemonLaunchLock {
  _file: File,
}

impl DaemonLaunchLock {
  fn try_acquire(paths: &RuntimePaths) -> Result<Option<Self>> {
    let path = paths.runtime_dir().join("cadder-launch.lock");
    if let Some(parent) = path.parent() {
      create_dir_all(parent)
        .with_context(|| format!("create launch lock directory {}", parent.display()))?;
    }

    let file = OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .open(&path)
      .with_context(|| format!("open daemon launch lock {}", path.display()))?;
    match FileExt::try_lock(&file) {
      Ok(()) => Ok(Some(Self { _file: file })),
      Err(TryLockError::WouldBlock) => Ok(None),
      Err(TryLockError::Error(error)) => {
        Err(error).with_context(|| format!("acquire daemon launch lock {}", path.display()))
      }
    }
  }
}

async fn can_connect(paths: &RuntimePaths) -> bool {
  let Ok(name) = paths.socket_name().to_ns_name::<GenericNamespaced>() else {
    return false;
  };
  Stream::connect(name).await.is_ok()
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
  use crate::{CaddyConfigCoordinator, PrivilegeStatus, discover_ipc_endpoint, logs::LogQuery};
  use cadder_protocol::{
    AutostartMode, BasicResponse, IpcEnvelope, ProtocolErrorKind, ProtocolErrorResponse,
    QueryStateRequest, QueryStateResponse, message_types, new_request_id,
  };
  use std::{env, ffi::OsString, fs, future::Future};
  use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
    task::JoinHandle,
    time::{Duration, sleep, timeout},
  };

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
  fn connection_registrations_replace_remove_and_iterate_owned_entries() {
    let mut registrations = ConnectionRegistrations::default();

    registrations.insert("shim-1".to_string(), "nonce-1".to_string());
    registrations.insert("shim-1".to_string(), "nonce-2".to_string());
    registrations.insert("shim-2".to_string(), "nonce-3".to_string());
    registrations.remove("missing");
    registrations.remove("shim-2");

    let entries = registrations.into_entries().collect::<Vec<_>>();
    assert_eq!(entries, vec![("shim-1".to_string(), "nonce-2".to_string())]);
  }

  #[test]
  fn cadder_client_from_env_uses_runtime_dir_override() {
    let _lock = lock_env();
    let _snapshot = EnvSnapshot::capture("CADDER_RUNTIME_DIR");
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("env-runtime");
    unsafe {
      env::set_var("CADDER_RUNTIME_DIR", &runtime_dir);
    }

    let client = CadderClient::from_env().unwrap();

    assert_eq!(client.paths.runtime_dir(), runtime_dir);
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
  async fn cadder_session_request_reports_eof_without_response() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (read_half, write_half) = tokio::io::split(conn);
      let mut reader = BufReader::new(read_half);
      let mut line = String::new();
      reader.read_line(&mut line).await.unwrap();
      drop(write_half);
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_, QueryStateResponse>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: new_request_id("test-query-state"),
        },
      )
      .await
      .unwrap_err();

    assert!(
      error.to_string().contains("without a response"),
      "{error:?}"
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn cadder_session_request_rejects_unexpected_response_type() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_basic_response(&mut writer, message_types::QUERY_LOGS_RESPONSE).await;
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_, QueryStateResponse>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: new_request_id("test-query-state"),
        },
      )
      .await
      .unwrap_err();

    assert!(
      error
        .to_string()
        .contains("unexpected response type `query-logs-response`"),
      "{error:?}"
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn cadder_session_request_rejects_malformed_response_json() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      writer.write_all(b"{not-json}\n").await.unwrap();
      writer.flush().await.unwrap();
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_, QueryStateResponse>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: new_request_id("test-query-state"),
        },
      )
      .await
      .unwrap_err();

    assert!(
      error.to_string().contains("key must be a string"),
      "{error:?}"
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn cadder_session_request_rejects_response_payload_shape() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_envelope(
        &mut writer,
        message_types::QUERY_STATE_RESPONSE,
        &serde_json::json!({ "accepted": true }),
      )
      .await
      .unwrap();
    });
    let mut session = CadderSession::connect(&server.paths).await.unwrap();

    let error = session
      .request::<_, QueryStateResponse>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: new_request_id("test-query-state"),
        },
      )
      .await
      .unwrap_err();

    assert!(error.to_string().contains("missing field"), "{error:?}");
    server.finish().await;
  }

  #[tokio::test]
  async fn state_subscription_reports_eof_without_event() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (read_half, write_half) = tokio::io::split(conn);
      let mut reader = BufReader::new(read_half);
      let mut line = String::new();
      reader.read_line(&mut line).await.unwrap();
      drop(write_half);
    });
    let session = CadderSession::connect(&server.paths).await.unwrap();
    let mut subscription = session
      .subscribe_state(new_request_id("test-subscribe"))
      .await
      .unwrap();

    let error = subscription.next_event().await.unwrap_err();

    assert!(
      error
        .to_string()
        .contains("daemon closed the state subscription"),
      "{error:?}"
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn state_subscription_rejects_unexpected_event_type() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_basic_response(&mut writer, message_types::QUERY_STATE_RESPONSE).await;
    });
    let session = CadderSession::connect(&server.paths).await.unwrap();
    let mut subscription = session
      .subscribe_state(new_request_id("test-subscribe"))
      .await
      .unwrap();

    let error = subscription.next_event().await.unwrap_err();

    assert!(
      error
        .to_string()
        .contains("unexpected response type `query-state-response`"),
      "{error:?}"
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn state_subscription_rejects_malformed_event_json() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      writer.write_all(b"{not-json}\n").await.unwrap();
      writer.flush().await.unwrap();
    });
    let session = CadderSession::connect(&server.paths).await.unwrap();
    let mut subscription = session
      .subscribe_state(new_request_id("test-subscribe"))
      .await
      .unwrap();

    let error = subscription.next_event().await.unwrap_err();

    assert!(
      error.to_string().contains("key must be a string"),
      "{error:?}"
    );
    server.finish().await;
  }

  #[tokio::test]
  async fn state_subscription_rejects_event_payload_shape() {
    let server = ScriptedIpcServer::start(|conn| async move {
      let (_line, mut writer) = read_one_request(conn).await;
      write_basic_response(&mut writer, message_types::STATE_CHANGED_EVENT).await;
    });
    let session = CadderSession::connect(&server.paths).await.unwrap();
    let mut subscription = session
      .subscribe_state(new_request_id("test-subscribe"))
      .await
      .unwrap();

    let error = subscription.next_event().await.unwrap_err();

    assert!(error.to_string().contains("missing field"), "{error:?}");
    server.finish().await;
  }

  #[tokio::test]
  async fn daemon_server_denies_unauthorized_state_changing_request_before_execution() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let state = DaemonState::with_runtime_paths(
      CaddyConfigCoordinator::new_mock(paths.clone()),
      paths.clone(),
    )
    .await
    .unwrap();
    let logs = state.logs();
    let denied_account = format!(
      "{}-other-token=secret",
      IpcPrincipal::current_process(crate::current_privilege_status()).account()
    );
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon = tokio::spawn(
      DaemonServer::new(paths.clone(), state)
        .with_peer_principal(IpcPrincipal::new(
          denied_account.clone(),
          PrivilegeStatus::NormalUser,
        ))
        .run_until(shutdown_rx),
    );
    wait_for_ready(&paths).await;

    let endpoint = discover_ipc_endpoint(&paths).unwrap();
    let mut session = CadderSession::connect(&paths).await.unwrap();
    let response: ProtocolErrorResponse = session
      .request(
        message_types::SET_AUTOSTART_REQUEST,
        message_types::PROTOCOL_ERROR_RESPONSE,
        &SetAutostartRequest {
          request_id: new_request_id("deny-autostart"),
          mode: AutostartMode::Daemon,
        },
      )
      .await
      .unwrap();
    let denial_log = logs.query(
      LogQuery {
        stream: LogStreamIdentity::runtime_control(),
        limit: 10,
        after_sequence: None,
        minimum_severity: Some(LogSeverity::Warn),
      },
      true,
    );

    assert_eq!(endpoint.socket_name, paths.socket_name());
    assert_eq!(response.error.kind, ProtocolErrorKind::AccessDenied);
    assert_eq!(
      response.error.denied_operation.as_deref(),
      Some(message_types::SET_AUTOSTART_REQUEST)
    );
    assert!(denial_log.entries.iter().any(|entry| {
      entry.operation.as_deref() == Some("ipc-access-denied")
        && entry
          .raw_message
          .contains(message_types::SET_AUTOSTART_REQUEST)
        && !entry.raw_message.contains(&denied_account)
        && !entry.raw_message.contains("secret")
    }));

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
        real_caddy_command: Some("real-caddy".to_string()),
        shim_path: Some(PathBuf::from("shim")),
        ..DaemonLaunchOptions::default()
      },
    )
    .await
    .unwrap();

    server.finish().await;
  }

  #[tokio::test]
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
        real_caddy_command: None,
        shim_path: None,
        ..DaemonLaunchOptions::default()
      },
    )
    .await
    .unwrap();

    server.finish().await;
  }

  #[tokio::test]
  async fn ensure_daemon_running_takes_over_after_failed_concurrent_launch_owner() {
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
        real_caddy_command: None,
        shim_path: None,
        ..DaemonLaunchOptions::default()
      },
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("start cadderd"), "{error:?}");
    release_lock.await.unwrap();
  }

  #[tokio::test]
  async fn ensure_daemon_running_reports_missing_explicit_daemon_start_failure() {
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

    assert!(error.to_string().contains("start cadderd"), "{error:?}");
  }

  #[tokio::test]
  async fn ensure_daemon_running_applies_launch_options_before_spawn_failure() {
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
        real_caddy_command: Some("real-caddy".to_string()),
        shim_path: Some(temp.path().join("shim")),
        ..DaemonLaunchOptions::default()
      },
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("start cadderd"), "{error:?}");
  }

  #[tokio::test]
  async fn ensure_daemon_running_rejects_mock_backend_with_real_command_before_spawn() {
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
        real_caddy_command: Some("real-caddy".to_string()),
        caddy_backend: Some(CaddyBackendMode::Mock),
        ..DaemonLaunchOptions::default()
      },
    )
    .await
    .unwrap_err();

    assert!(
      error
        .to_string()
        .contains("--real-caddy-command cannot be combined"),
      "{error:?}"
    );
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

  struct ScriptedIpcServer {
    paths: RuntimePaths,
    task: JoinHandle<()>,
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
      let name = paths
        .socket_name()
        .to_ns_name::<GenericNamespaced>()
        .unwrap();
      let listener = ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .create_tokio()
        .unwrap();
      let task = tokio::spawn(async move {
        let conn = listener.accept().await.unwrap();
        handler(conn).await;
      });
      Self {
        paths,
        task,
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
        let name = server_paths
          .socket_name()
          .to_ns_name::<GenericNamespaced>()
          .unwrap();
        let listener = ListenerOptions::new()
          .name(name)
          .try_overwrite(true)
          .create_tokio()
          .unwrap();
        let conn = listener.accept().await.unwrap();
        handler(conn).await;
      });
      Self {
        paths,
        task,
        _temp: temp,
      }
    }

    async fn finish(self) {
      self.task.await.unwrap();
    }
  }

  async fn read_one_request(conn: Stream) -> (String, tokio::io::WriteHalf<Stream>) {
    let (read_half, writer) = tokio::io::split(conn);
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    assert!(!line.is_empty(), "client did not send an IPC request");
    (line, writer)
  }

  async fn wait_for_ready(paths: &RuntimePaths) {
    for _ in 0..50 {
      if can_connect(paths).await {
        return;
      }
      sleep(Duration::from_millis(20)).await;
    }

    panic!("daemon server did not become ready");
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
