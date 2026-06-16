use crate::{DaemonState, RuntimePaths};
use anyhow::{Context, Result, anyhow};
use cadder_protocol::{
  BasicResponse, HeartbeatEntrypointRequest, IpcEnvelope, QueryIisBindingsRequest,
  QueryLogsRequest, QueryStateRequest, RegisterEntrypointRequest, SetDomainEnabledRequest,
  SetEntrypointEnabledRequest, SetIisHandoffRequest, ShutdownDaemonRequest, StateChangedEvent,
  SubscribeStateRequest, UnregisterEntrypointRequest, message_types,
};
use fs4::{FileExt, TryLockError};
use interprocess::local_socket::{
  GenericNamespaced, ListenerOptions, ToNsName,
  tokio::{Stream, prelude::*},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
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
}

impl DaemonServer {
  pub fn new(paths: RuntimePaths, state: DaemonState) -> Self {
    Self { paths, state }
  }

  pub async fn run_until(self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    let name = self.paths.socket_name().to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new()
      .name(name)
      .try_overwrite(true)
      .create_tokio()
      .context("create local IPC listener")?;
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
                      tokio::spawn(async move {
                          let _ = handle_connection(conn, state).await;
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

async fn handle_connection(conn: Stream, state: DaemonState) -> Result<()> {
  let mut owned = Vec::<(String, String)>::new();
  let (read_half, mut write_half) = tokio::io::split(conn);
  let mut reader = BufReader::new(read_half);
  let mut line = String::new();

  loop {
    line.clear();
    let read = reader.read_line(&mut line).await?;
    if read == 0 {
      break;
    }

    let envelope: IpcEnvelope = serde_json::from_str(line.trim_end())?;
    match envelope.message_type.as_str() {
      message_types::REGISTER_ENTRYPOINT_REQUEST => {
        let request: RegisterEntrypointRequest = envelope.decode()?;
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
          owned.push((id.clone(), nonce));
        }
        write_envelope(
          &mut write_half,
          message_types::REGISTER_ENTRYPOINT_RESPONSE,
          &response,
        )
        .await?;
      }
      message_types::UNREGISTER_ENTRYPOINT_REQUEST => {
        let request: UnregisterEntrypointRequest = envelope.decode()?;
        let response = state
          .unregister(
            request.request_id,
            &request.registration_id,
            &request.shim_session_nonce,
          )
          .await;
        owned.retain(|(id, _)| id != &request.registration_id);
        write_envelope(
          &mut write_half,
          message_types::UNREGISTER_ENTRYPOINT_RESPONSE,
          &response,
        )
        .await?;
      }
      message_types::HEARTBEAT_ENTRYPOINT_REQUEST => {
        let request: HeartbeatEntrypointRequest = envelope.decode()?;
        let response = state.heartbeat(request).await;
        write_envelope(
          &mut write_half,
          message_types::HEARTBEAT_ENTRYPOINT_RESPONSE,
          &response,
        )
        .await?;
      }
      message_types::QUERY_STATE_REQUEST => {
        let request: QueryStateRequest = envelope.decode()?;
        let response = state.query_state(request.request_id).await;
        write_envelope(
          &mut write_half,
          message_types::QUERY_STATE_RESPONSE,
          &response,
        )
        .await?;
      }
      message_types::SET_ENTRYPOINT_ENABLED_REQUEST => {
        let request: SetEntrypointEnabledRequest = envelope.decode()?;
        let response = state.set_entrypoint_enabled(request).await;
        write_envelope(
          &mut write_half,
          message_types::SET_ENTRYPOINT_ENABLED_RESPONSE,
          &response,
        )
        .await?;
      }
      message_types::SET_DOMAIN_ENABLED_REQUEST => {
        let request: SetDomainEnabledRequest = envelope.decode()?;
        let response = state.set_domain_enabled(request).await;
        write_envelope(
          &mut write_half,
          message_types::SET_DOMAIN_ENABLED_RESPONSE,
          &response,
        )
        .await?;
      }
      message_types::QUERY_IIS_BINDINGS_REQUEST => {
        let request: QueryIisBindingsRequest = envelope.decode()?;
        let response = state.query_iis_bindings(request.request_id).await;
        write_envelope(
          &mut write_half,
          message_types::QUERY_IIS_BINDINGS_RESPONSE,
          &response,
        )
        .await?;
      }
      message_types::SET_IIS_HANDOFF_REQUEST => {
        let request: SetIisHandoffRequest = envelope.decode()?;
        let response = state.set_iis_handoff(request).await;
        write_envelope(
          &mut write_half,
          message_types::SET_IIS_HANDOFF_RESPONSE,
          &response,
        )
        .await?;
      }
      message_types::QUERY_LOGS_REQUEST => {
        let request: QueryLogsRequest = envelope.decode()?;
        let response = state.query_logs(request).await;
        write_envelope(
          &mut write_half,
          message_types::QUERY_LOGS_RESPONSE,
          &response,
        )
        .await?;
      }
      message_types::SUBSCRIBE_STATE_REQUEST => {
        let request: SubscribeStateRequest = envelope.decode()?;
        let snapshot = state.snapshot().await;
        let initial = cadder_protocol::StateChangedEvent {
          request_id: request.request_id.clone(),
          sequence_number: 0,
          change_kind: cadder_protocol::StateChangeKind::Snapshot,
          snapshot,
          registration_id: None,
        };
        write_envelope(
          &mut write_half,
          message_types::STATE_CHANGED_EVENT,
          &initial,
        )
        .await?;
        let mut subscription = state.subscribe();
        while let Ok(mut event) = subscription.recv().await {
          event.request_id = request.request_id.clone();
          write_envelope(&mut write_half, message_types::STATE_CHANGED_EVENT, &event).await?;
        }
      }
      message_types::SHUTDOWN_DAEMON_REQUEST => {
        let request: ShutdownDaemonRequest = envelope.decode()?;
        let mut response = state.shutdown().await;
        response.request_id = request.request_id;
        write_envelope(
          &mut write_half,
          message_types::SHUTDOWN_DAEMON_RESPONSE,
          &response,
        )
        .await?;
        break;
      }
      other => {
        let response = BasicResponse {
          request_id: "unsupported".to_string(),
          accepted: false,
          message: format!("Unsupported IPC message type `{other}`."),
        };
        write_envelope(&mut write_half, "error-response", &response).await?;
      }
    }
  }

  for (id, nonce) in owned {
    let _ = state
      .unregister("pipe-disconnect".to_string(), &id, &nonce)
      .await;
  }

  Ok(())
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
  pub real_caddy_command: Option<String>,
  pub shim_path: Option<PathBuf>,
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

  let Some(_launch_lock) = DaemonLaunchLock::try_acquire(paths)? else {
    wait_for_daemon_ready(paths)
      .await
      .context("wait for concurrent cadderd launch")?;
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

  let mut command = Command::new(daemon);
  configure_background_daemon(&mut command);
  command
    .arg("--runtime-dir")
    .arg(paths.runtime_dir())
    .arg("--detach-ready")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
  if let Some(real_caddy_command) = options.real_caddy_command {
    command.arg("--real-caddy-command").arg(real_caddy_command);
  }
  if let Some(shim_path) = options.shim_path {
    command.env("CADDER_CADDY_SHIM_PATH", shim_path);
  }
  command.spawn().context("start cadderd")?;

  wait_for_daemon_ready(paths).await
}

async fn wait_for_daemon_ready(paths: &RuntimePaths) -> Result<()> {
  for _ in 0..50 {
    if can_connect(paths).await {
      return Ok(());
    }
    sleep(Duration::from_millis(100)).await;
  }

  Err(anyhow!("cadderd did not become ready before timeout"))
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

fn configure_background_daemon(command: &mut Command) {
  #[cfg(windows)]
  {
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
  }

  #[cfg(unix)]
  {
    command.process_group(0);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_protocol::{IpcEnvelope, QueryStateRequest};
  use tokio::io::AsyncReadExt;

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
}
