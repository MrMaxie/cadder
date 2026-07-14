//! Hidden runtime-guard process and its authenticated owner channel.

use crate::{
  RuntimeGuardBootstrapAuthenticatedResponse, RuntimeGuardBootstrapRequest,
  RuntimeGuardCommandRequest, RuntimeGuardFinalizedResponse, RuntimeGuardLogFrame,
  RuntimeGuardProtocol, RuntimeGuardReadyResponse, RuntimeGuardRecordState, RuntimeGuardRequest,
  RuntimeGuardResponse, RuntimeGuardStartRequest, RuntimeGuardStartedResponse,
  RuntimeGuardStatusResponse, RuntimeGuardTerminatedResponse, RuntimePaths,
  caddy_image::OpenedCaddyImage,
  ipc_codec::{BoundedNdjsonCodec, MAX_IPC_FRAME_LENGTH, encode_json_frame},
  process_tree::ProcessTreeChild,
  runtime_file::validated_candidate_path,
  runtime_guard_identity::{
    PinnedRuntimeGuardImage, child_process_identity, current_guard_identity,
  },
  runtime_guard_record::{
    RuntimeGuardChildIdentity, RuntimeGuardGeneration, RuntimeGuardGenerationContext,
    RuntimeGuardGenerationLock, RuntimeGuardPinnedCaddyIdentity, RuntimeGuardRecord,
    RuntimeGuardTerminalOutcome, RuntimeGuardTerminalReason,
  },
};
use anyhow::{Context, Result, bail, ensure};
use bytes::BytesMut;
use chrono::Utc;
use futures_util::StreamExt;
use std::{path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
  io::{AsyncWrite, AsyncWriteExt, BufWriter},
  process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
  sync::Mutex,
  task::yield_now,
  time::{Instant, sleep, timeout},
};
use tokio_util::{
  codec::{Encoder, FramedRead, LinesCodec},
  task::TaskTracker,
};

const CONTROL_DEADLINE: Duration = Duration::from_secs(30);
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(20);
const PARTIAL_FRAME_POLL_INTERVAL: Duration = Duration::from_millis(100);
const GUARD_EXIT_DEADLINE: Duration = Duration::from_secs(5);
const CHILD_START_CHECK: Duration = Duration::from_millis(250);
const CHILD_EXIT_DEADLINE: Duration = Duration::from_secs(10);

/// Immutable inputs accepted by the hidden `cadderd` runtime-guard mode.
#[derive(Debug, Clone)]
pub struct RuntimeGuardHiddenOptions {
  pub paths: RuntimePaths,
  pub daemon_instance_id: String,
  pub owner_generation: String,
  pub nonce_commitment: String,
}

impl RuntimeGuardHiddenOptions {
  fn context(&self) -> RuntimeGuardGenerationContext {
    RuntimeGuardGenerationContext {
      profile: self.paths.runtime_profile().to_string(),
      runtime_id: self.paths.instance_key().to_string(),
      daemon_instance_id: self.daemon_instance_id.clone(),
      owner_generation: self.owner_generation.clone(),
      nonce_commitment: self.nonce_commitment.clone(),
    }
  }
}

/// Runs the one-shot hidden guard over inherited stdin and stdout.
pub async fn run_runtime_guard(options: RuntimeGuardHiddenOptions) -> Result<()> {
  let context = options.context();
  let mut requests = FramedRead::new(
    tokio::io::stdin(),
    RuntimeGuardProtocol::new(context.clone()),
  );
  let mut responses = BufWriter::new(tokio::io::stdout());

  let bootstrap = timeout(CONTROL_DEADLINE, requests.next())
    .await
    .context("runtime guard bootstrap timed out")?
    .ok_or_else(|| anyhow::anyhow!("runtime guard owner closed before bootstrap"))??;
  let RuntimeGuardRequest::Bootstrap(bootstrap) = bootstrap else {
    bail!("runtime guard received a non-bootstrap first operation");
  };
  let pinned_caddy = bootstrap.pinned_caddy.clone();
  let caddy = pinned_caddy
    .as_ref()
    .map(OpenedCaddyImage::verify_runtime_guard_claim)
    .transpose()?;
  send_response(
    &mut requests,
    &mut responses,
    RuntimeGuardResponse::BootstrapAuthenticated(RuntimeGuardBootstrapAuthenticatedResponse {
      protocol_revision: crate::RUNTIME_GUARD_PROTOCOL_REVISION,
      context: context.clone(),
      request_id: bootstrap.request_id,
    }),
  )
  .await?;

  let guard_identity = current_guard_identity()?;
  let (generation_lock, owner_lost_during_handoff) =
    acquire_generation_lock(&options.paths, &mut requests).await?;
  if owner_lost_during_handoff {
    generation_lock.publish(&RuntimeGuardRecord::terminal(
      context,
      guard_identity,
      None,
      terminal_outcome(RuntimeGuardTerminalReason::OwnerChannelClosed, None),
    ))?;
    return Ok(());
  }
  generation_lock.publish(&RuntimeGuardRecord::ready(
    context.clone(),
    guard_identity.clone(),
    None,
  ))?;
  if let Err(error) = send_response(
    &mut requests,
    &mut responses,
    RuntimeGuardResponse::GuardReady(RuntimeGuardReadyResponse {
      context: context.clone(),
      request_id: bootstrap.request_id,
      guard: guard_identity.clone(),
    }),
  )
  .await
  {
    generation_lock.publish(&RuntimeGuardRecord::terminal(
      context,
      guard_identity,
      None,
      terminal_outcome(RuntimeGuardTerminalReason::OwnerChannelClosed, None),
    ))?;
    return Err(error).context("publish runtime guard readiness to owner");
  }

  let log_forwarder = GuardLogForwarder::new(tokio::io::stderr());
  let mut runtime = GuardOwnedRuntime::new(options.paths, pinned_caddy, caddy, log_forwarder);

  let control_result: Result<(RuntimeGuardTerminalReason, Option<u64>)> = async {
    loop {
      match next_control_request(&mut requests).await? {
        Some(RuntimeGuardRequest::Start(request)) => {
          let request_id = request.request_id;
          let child = runtime.start(request).await?;
          generation_lock.publish(&RuntimeGuardRecord::ready(
            context.clone(),
            guard_identity.clone(),
            Some(child.clone()),
          ))?;
          send_response(
            &mut requests,
            &mut responses,
            RuntimeGuardResponse::Started(RuntimeGuardStartedResponse {
              context: context.clone(),
              request_id,
              child,
            }),
          )
          .await?;
        }
        Some(RuntimeGuardRequest::Terminate(request)) => {
          let child_exit_code = runtime.terminate().await?;
          publish_runtime_ready(&generation_lock, &context, &guard_identity, &runtime)?;
          send_response(
            &mut requests,
            &mut responses,
            RuntimeGuardResponse::Terminated(RuntimeGuardTerminatedResponse {
              context: context.clone(),
              request_id: request.request_id,
              child_exit_code,
            }),
          )
          .await?;
        }
        Some(RuntimeGuardRequest::Status(request)) => {
          runtime.refresh_status().await?;
          publish_runtime_ready(&generation_lock, &context, &guard_identity, &runtime)?;
          send_response(
            &mut requests,
            &mut responses,
            RuntimeGuardResponse::Status(RuntimeGuardStatusResponse {
              context: context.clone(),
              request_id: request.request_id,
              state: RuntimeGuardRecordState::Ready,
              guard: guard_identity.clone(),
              child: runtime.active_identity().cloned(),
            }),
          )
          .await?;
        }
        Some(RuntimeGuardRequest::Finalize(request)) => {
          return Ok((
            RuntimeGuardTerminalReason::CleanFinalize,
            Some(request.request_id),
          ));
        }
        None => {
          return Ok((RuntimeGuardTerminalReason::OwnerChannelClosed, None));
        }
        Some(_) => {
          bail!("runtime guard owner channel failed closed");
        }
      }
    }
  }
  .await;

  let (reason, finalize_request_id, control_error) = match control_result {
    Ok((reason, request_id)) => (reason, request_id, None),
    Err(error) => (RuntimeGuardTerminalReason::GuardFailure, None, Some(error)),
  };

  let child_exit_code = runtime.terminate().await?;
  let terminal = terminal_outcome(reason, child_exit_code.or(runtime.last_exit_code()));
  generation_lock.publish(&RuntimeGuardRecord::terminal(
    context.clone(),
    guard_identity,
    runtime.last_identity().cloned(),
    terminal.clone(),
  ))?;
  if let Some(request_id) = finalize_request_id {
    send_response(
      &mut requests,
      &mut responses,
      RuntimeGuardResponse::Finalized(RuntimeGuardFinalizedResponse {
        context,
        request_id,
        terminal,
      }),
    )
    .await?;
  }
  match control_error {
    Some(error) => Err(error),
    None => Ok(()),
  }
}

fn publish_runtime_ready(
  generation_lock: &RuntimeGuardGenerationLock,
  context: &RuntimeGuardGenerationContext,
  guard_identity: &crate::RuntimeGuardIdentity,
  runtime: &GuardOwnedRuntime,
) -> Result<()> {
  let record = match runtime.active_identity() {
    Some(child) => {
      RuntimeGuardRecord::ready(context.clone(), guard_identity.clone(), Some(child.clone()))
    }
    None => match runtime.last_identity() {
      Some(child) => {
        RuntimeGuardRecord::ready_after_join(context.clone(), guard_identity.clone(), child.clone())
      }
      None => RuntimeGuardRecord::ready(context.clone(), guard_identity.clone(), None),
    },
  };
  generation_lock.publish(&record)
}

#[derive(Clone)]
struct GuardLogForwarder {
  writer: Arc<Mutex<BufWriter<Box<dyn AsyncWrite + Send + Unpin>>>>,
  tasks: TaskTracker,
}

impl GuardLogForwarder {
  fn new(writer: impl AsyncWrite + Send + Unpin + 'static) -> Self {
    Self {
      writer: Arc::new(Mutex::new(BufWriter::new(Box::new(writer)))),
      tasks: TaskTracker::new(),
    }
  }

  fn reopen(&self) {
    self.tasks.reopen();
  }

  fn spawn<R>(&self, reader: R, channel: &'static str)
  where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
  {
    let writer = Arc::clone(&self.writer);
    self.tasks.spawn(async move {
      let mut lines = FramedRead::new(
        reader,
        LinesCodec::new_with_max_length(MAX_IPC_FRAME_LENGTH),
      );
      while let Some(result) = lines.next().await {
        let Ok(message) = result else {
          break;
        };
        let frame = RuntimeGuardLogFrame {
          channel: channel.to_string(),
          message,
        };
        let Ok(frame) = encode_json_frame(&frame) else {
          break;
        };
        let mut writer = writer.lock().await;
        if writer.write_all(&frame).await.is_err() || writer.flush().await.is_err() {
          break;
        }
      }
    });
  }

  async fn close_and_wait(&self) {
    self.tasks.close();
    self.tasks.wait().await;
  }
}

struct GuardOwnedTree {
  child: ProcessTreeChild,
  identity: RuntimeGuardChildIdentity,
}

struct GuardOwnedRuntime {
  paths: RuntimePaths,
  pinned_caddy: Option<RuntimeGuardPinnedCaddyIdentity>,
  caddy: Option<OpenedCaddyImage>,
  log_forwarder: GuardLogForwarder,
  active: Option<GuardOwnedTree>,
  last_identity: Option<RuntimeGuardChildIdentity>,
  last_exit_code: Option<i32>,
}

impl GuardOwnedRuntime {
  fn new(
    paths: RuntimePaths,
    pinned_caddy: Option<RuntimeGuardPinnedCaddyIdentity>,
    caddy: Option<OpenedCaddyImage>,
    log_forwarder: GuardLogForwarder,
  ) -> Self {
    Self {
      paths,
      pinned_caddy,
      caddy,
      log_forwarder,
      active: None,
      last_identity: None,
      last_exit_code: None,
    }
  }

  async fn start(
    &mut self,
    request: RuntimeGuardStartRequest,
  ) -> Result<RuntimeGuardChildIdentity> {
    ensure!(
      self.active.is_none(),
      "runtime guard already owns a Caddy child"
    );
    let config_path = validated_candidate_path(&self.paths, &request.config_generation)?;
    let caddy = self
      .caddy
      .as_ref()
      .context("runtime guard has no pinned Caddy claim")?;
    let mut child = caddy
      .spawn("runtime-guard Caddy child", |command| {
        command
          .arg("run")
          .arg("--config")
          .arg(&config_path)
          .stdin(Stdio::null())
          .stdout(Stdio::piped())
          .stderr(Stdio::piped());
      })
      .await?;
    self.log_forwarder.reopen();
    if let Some(stdout) = child.take_stdout() {
      self.log_forwarder.spawn(stdout, "stdout");
    }
    if let Some(stderr) = child.take_stderr() {
      self.log_forwarder.spawn(stderr, "stderr");
    }
    let process_id = child
      .id()
      .context("runtime-guard Caddy child has no process ID")?;
    let process = match child_process_identity(process_id) {
      Ok(identity) => identity,
      Err(error) => {
        child
          .terminate_and_join("runtime-guard Caddy child without a stable identity")
          .await?;
        self.log_forwarder.close_and_wait().await;
        return Err(error).context("capture runtime-guard Caddy child identity");
      }
    };
    yield_now().await;
    match timeout(CHILD_START_CHECK, child.wait()).await {
      Ok(Ok(status)) => {
        self.log_forwarder.close_and_wait().await;
        bail!("runtime-guard Caddy child exited immediately with status {status}");
      }
      Ok(Err(error)) => {
        self.log_forwarder.close_and_wait().await;
        return Err(error).context("inspect runtime-guard Caddy child startup");
      }
      Err(_) => {}
    }
    let pinned_caddy = self
      .pinned_caddy
      .clone()
      .context("runtime guard lost its pinned Caddy claim")?;
    let identity = RuntimeGuardChildIdentity {
      child_generation: request.child_generation,
      process,
      pinned_caddy,
    };
    self.last_identity = Some(identity.clone());
    self.last_exit_code = None;
    self.active = Some(GuardOwnedTree {
      child,
      identity: identity.clone(),
    });
    Ok(identity)
  }

  async fn refresh_status(&mut self) -> Result<()> {
    let Some(active) = self.active.as_mut() else {
      return Ok(());
    };
    if let Some(status) = active
      .child
      .try_wait()
      .context("inspect runtime-guard Caddy child")?
    {
      self.last_exit_code = status.code();
      self.active = None;
      self.log_forwarder.close_and_wait().await;
    }
    Ok(())
  }

  async fn terminate(&mut self) -> Result<Option<i32>> {
    let Some(active) = self.active.as_mut() else {
      return Ok(self.last_exit_code);
    };
    if let Some(status) = active
      .child
      .try_wait()
      .context("inspect runtime-guard Caddy child before termination")?
    {
      self.last_exit_code = status.code();
      self.active = None;
      self.log_forwarder.close_and_wait().await;
      return Ok(self.last_exit_code);
    }
    active
      .child
      .start_kill()
      .context("terminate runtime-guard Caddy tree")?;
    let status = timeout(CHILD_EXIT_DEADLINE, active.child.wait())
      .await
      .context("runtime-guard Caddy tree did not exit within ten seconds")??;
    self.last_exit_code = status.code();
    self.active = None;
    self.log_forwarder.close_and_wait().await;
    Ok(self.last_exit_code)
  }

  fn active_identity(&self) -> Option<&RuntimeGuardChildIdentity> {
    self.active.as_ref().map(|active| &active.identity)
  }

  fn last_identity(&self) -> Option<&RuntimeGuardChildIdentity> {
    self.last_identity.as_ref()
  }

  fn last_exit_code(&self) -> Option<i32> {
    self.last_exit_code
  }
}

async fn send_response(
  requests: &mut FramedRead<tokio::io::Stdin, RuntimeGuardProtocol>,
  responses: &mut BufWriter<tokio::io::Stdout>,
  response: RuntimeGuardResponse,
) -> Result<()> {
  let mut encoded = BytesMut::new();
  requests
    .decoder_mut()
    .encode(response, &mut encoded)
    .context("encode runtime guard response")?;
  timeout(CONTROL_DEADLINE, async {
    responses
      .write_all(&encoded)
      .await
      .context("write runtime guard response")?;
    responses
      .flush()
      .await
      .context("flush runtime guard response")
  })
  .await
  .context("runtime guard response timed out")?
}

async fn acquire_generation_lock(
  paths: &RuntimePaths,
  requests: &mut FramedRead<tokio::io::Stdin, RuntimeGuardProtocol>,
) -> Result<(RuntimeGuardGenerationLock, bool)> {
  let deadline = Instant::now() + CONTROL_DEADLINE;
  let mut owner_lost = false;
  let mut partial_since = None;
  loop {
    if let Some(lock) = RuntimeGuardGenerationLock::try_acquire(paths)? {
      return Ok((lock, owner_lost));
    }
    ensure!(
      Instant::now() < deadline,
      "runtime guard did not receive containment ownership before timeout"
    );
    if owner_lost {
      sleep(LOCK_POLL_INTERVAL).await;
      continue;
    }
    match timeout(LOCK_POLL_INTERVAL, requests.next()).await {
      Ok(Some(Ok(_))) => bail!("runtime guard received a command before containment handoff"),
      Ok(Some(Err(error))) => return Err(error).context("runtime guard handoff channel failed"),
      Ok(None) => owner_lost = true,
      Err(_) => check_partial_frame_deadline(requests, &mut partial_since)?,
    }
  }
}

async fn next_control_request(
  requests: &mut FramedRead<tokio::io::Stdin, RuntimeGuardProtocol>,
) -> Result<Option<RuntimeGuardRequest>> {
  let mut partial_since = None;
  loop {
    match timeout(PARTIAL_FRAME_POLL_INTERVAL, requests.next()).await {
      Ok(Some(Ok(request))) => return Ok(Some(request)),
      Ok(Some(Err(error))) => return Err(error).context("runtime guard control channel failed"),
      Ok(None) => return Ok(None),
      Err(_) => check_partial_frame_deadline(requests, &mut partial_since)?,
    }
  }
}

fn check_partial_frame_deadline(
  requests: &FramedRead<tokio::io::Stdin, RuntimeGuardProtocol>,
  partial_since: &mut Option<Instant>,
) -> Result<()> {
  if requests.read_buffer().is_empty() {
    *partial_since = None;
    return Ok(());
  }
  let started = partial_since.get_or_insert_with(Instant::now);
  ensure!(
    started.elapsed() < CONTROL_DEADLINE,
    "runtime guard partial frame timed out"
  );
  Ok(())
}

fn terminal_outcome(
  reason: RuntimeGuardTerminalReason,
  child_exit_code: Option<i32>,
) -> RuntimeGuardTerminalOutcome {
  RuntimeGuardTerminalOutcome {
    reason,
    child_exit_code,
    completed_at_utc: Utc::now(),
  }
}

#[derive(Debug)]
pub(crate) struct RuntimeGuardClient {
  child: Child,
  stdin: Option<ChildStdin>,
  responses: FramedRead<ChildStdout, BoundedNdjsonCodec>,
  logs: Option<ChildStderr>,
  context: RuntimeGuardGenerationContext,
  next_request_id: u64,
}

impl RuntimeGuardClient {
  pub(crate) async fn spawn_authenticated(
    executable: &Path,
    _paths: &RuntimePaths,
    context: RuntimeGuardGenerationContext,
    generation: &RuntimeGuardGeneration,
    pinned_caddy: Option<RuntimeGuardPinnedCaddyIdentity>,
  ) -> Result<Self> {
    ensure!(
      executable.is_absolute(),
      "runtime guard executable must be absolute"
    );
    let image = PinnedRuntimeGuardImage::open(executable)?;
    let mut command = Command::new(image.path());
    command
      .arg("--runtime-guard")
      .arg("--runtime-guard-instance")
      .arg(&context.daemon_instance_id)
      .arg("--runtime-guard-owner-generation")
      .arg(&context.owner_generation)
      .arg("--runtime-guard-commitment")
      .arg(&context.nonce_commitment)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    let mut child = command
      .spawn()
      .context("start hidden cadderd runtime guard")?;
    if let Err(error) = image.reverify_path() {
      let _ = child.kill().await;
      let _ = child.wait().await;
      return Err(error).context("reverify hidden cadderd after spawn");
    }
    let stdin = child
      .stdin
      .take()
      .context("open runtime guard owner channel")?;
    let stdout = child
      .stdout
      .take()
      .context("open runtime guard response channel")?;
    let logs = child
      .stderr
      .take()
      .context("open runtime guard log channel")?;
    let mut client = Self {
      child,
      stdin: Some(stdin),
      responses: FramedRead::new(stdout, BoundedNdjsonCodec::new()),
      logs: Some(logs),
      context: context.clone(),
      next_request_id: 2,
    };
    client
      .send(&RuntimeGuardRequest::Bootstrap(
        RuntimeGuardBootstrapRequest {
          protocol_revision: crate::RUNTIME_GUARD_PROTOCOL_REVISION,
          context,
          request_id: 1,
          nonce: generation.nonce(),
          pinned_caddy,
        },
      ))
      .await?;
    let response = client.read_response().await?;
    ensure!(
      matches!(
        response,
        RuntimeGuardResponse::BootstrapAuthenticated(ref authenticated)
          if authenticated.context == client.context
            && authenticated.request_id == 1
            && authenticated.protocol_revision == crate::RUNTIME_GUARD_PROTOCOL_REVISION
      ),
      "runtime guard returned an invalid bootstrap authentication response"
    );
    Ok(client)
  }

  pub(crate) async fn wait_until_ready(&mut self) -> Result<crate::RuntimeGuardIdentity> {
    let response = self.read_response().await?;
    let RuntimeGuardResponse::GuardReady(ready) = response else {
      bail!("runtime guard did not publish its ready identity");
    };
    ensure!(
      ready.context == self.context && ready.request_id == 1,
      "runtime guard ready response belongs to another generation"
    );
    Ok(ready.guard)
  }

  pub(crate) fn take_logs(&mut self) -> Option<ChildStderr> {
    self.logs.take()
  }

  pub(crate) async fn start(
    &mut self,
    config_generation: &str,
  ) -> Result<RuntimeGuardChildIdentity> {
    let request_id = self.next_request_id();
    let child_generation = random_generation_id()?;
    self
      .send(&RuntimeGuardRequest::Start(RuntimeGuardStartRequest {
        context: self.context.clone(),
        request_id,
        child_generation,
        config_generation: config_generation.to_string(),
      }))
      .await?;
    let response = self.read_response().await?;
    let RuntimeGuardResponse::Started(started) = response else {
      bail!("runtime guard returned an invalid start response");
    };
    ensure!(
      started.context == self.context && started.request_id == request_id,
      "runtime guard start response belongs to another generation"
    );
    Ok(started.child)
  }

  pub(crate) async fn status(&mut self) -> Result<Option<RuntimeGuardChildIdentity>> {
    let request_id = self.next_request_id();
    self
      .send(&RuntimeGuardRequest::Status(RuntimeGuardCommandRequest {
        context: self.context.clone(),
        request_id,
      }))
      .await?;
    let response = self.read_response().await?;
    let RuntimeGuardResponse::Status(status) = response else {
      bail!("runtime guard returned an invalid status response");
    };
    ensure!(
      status.context == self.context
        && status.request_id == request_id
        && status.state == RuntimeGuardRecordState::Ready,
      "runtime guard status response belongs to another generation"
    );
    Ok(status.child)
  }

  pub(crate) async fn terminate(&mut self) -> Result<Option<i32>> {
    let request_id = self.next_request_id();
    self
      .send(&RuntimeGuardRequest::Terminate(
        RuntimeGuardCommandRequest {
          context: self.context.clone(),
          request_id,
        },
      ))
      .await?;
    let response = self.read_response().await?;
    let RuntimeGuardResponse::Terminated(terminated) = response else {
      bail!("runtime guard returned an invalid termination response");
    };
    ensure!(
      terminated.context == self.context && terminated.request_id == request_id,
      "runtime guard termination response belongs to another generation"
    );
    Ok(terminated.child_exit_code)
  }

  pub(crate) fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>> {
    self
      .child
      .try_wait()
      .context("inspect runtime guard process")
  }

  pub(crate) async fn finalize(&mut self) -> Result<()> {
    let request_id = self.next_request_id();
    self
      .send(&RuntimeGuardRequest::Finalize(RuntimeGuardCommandRequest {
        context: self.context.clone(),
        request_id,
      }))
      .await?;
    let response = self.read_response().await?;
    ensure!(
      matches!(
        response,
        RuntimeGuardResponse::Finalized(ref finalized)
          if finalized.context == self.context && finalized.request_id == request_id
      ),
      "runtime guard returned an invalid finalization response"
    );
    self.stdin.take();
    let status = timeout(GUARD_EXIT_DEADLINE, self.child.wait())
      .await
      .context("runtime guard did not exit after finalization")??;
    ensure!(
      status.success(),
      "runtime guard exited unsuccessfully after finalization"
    );
    Ok(())
  }

  fn next_request_id(&mut self) -> u64 {
    let request_id = self.next_request_id;
    self.next_request_id += 1;
    request_id
  }

  async fn send(&mut self, request: &RuntimeGuardRequest) -> Result<()> {
    let frame = encode_json_frame(request).context("encode runtime guard request")?;
    timeout(CONTROL_DEADLINE, async {
      let stdin = self
        .stdin
        .as_mut()
        .context("runtime guard owner channel is closed")?;
      stdin
        .write_all(&frame)
        .await
        .context("write runtime guard request")?;
      stdin.flush().await.context("flush runtime guard request")
    })
    .await
    .context("runtime guard request timed out")??;
    Ok(())
  }

  async fn read_response(&mut self) -> Result<RuntimeGuardResponse> {
    let frame = timeout(CONTROL_DEADLINE, self.responses.next())
      .await
      .context("runtime guard response timed out")?
      .ok_or_else(|| anyhow::anyhow!("runtime guard closed without a response"))??;
    serde_json::from_str(&frame).context("decode runtime guard response")
  }
}

fn random_generation_id() -> Result<String> {
  let mut bytes = [0_u8; 16];
  getrandom::fill(&mut bytes).map_err(|error| std::io::Error::other(error.to_string()))?;
  Ok(hex::encode(bytes))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{RealCaddyResolver, runtime_file::StagedRuntimeConfig};
  use tokio::io::{AsyncBufReadExt, BufReader};

  #[tokio::test]
  async fn runtime_guard_owns_started_caddy_and_forwards_bounded_logs() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let fake_caddy = write_guard_fake_caddy(temp.path());
    let pinned = RealCaddyResolver::for_test_fixture(fake_caddy)
      .pin()
      .await
      .unwrap();
    let claim = pinned.runtime_guard_identity();
    let opened = OpenedCaddyImage::verify_runtime_guard_claim(&claim).unwrap();
    let staged = StagedRuntimeConfig::stage(&paths, b"{}\n").await.unwrap();
    let (log_writer, log_reader) = tokio::io::duplex(4096);
    let mut runtime = GuardOwnedRuntime::new(
      paths.clone(),
      Some(claim.clone()),
      Some(opened),
      GuardLogForwarder::new(log_writer),
    );
    let context = RuntimeGuardGenerationContext {
      profile: paths.runtime_profile().to_string(),
      runtime_id: paths.instance_key().to_string(),
      daemon_instance_id: "00112233445566778899aabbccddeeff".to_string(),
      owner_generation: "ffeeddccbbaa99887766554433221100".to_string(),
      nonce_commitment: "11".repeat(32),
    };

    let child = runtime
      .start(RuntimeGuardStartRequest {
        context,
        request_id: 2,
        child_generation: "1234567890abcdef1234567890abcdef".to_string(),
        config_generation: staged.generation().to_string(),
      })
      .await
      .unwrap();

    assert_eq!(child.pinned_caddy, claim);
    assert_eq!(runtime.active_identity(), Some(&child));
    let mut lines = BufReader::new(log_reader).lines();
    let first = timeout(Duration::from_secs(2), lines.next_line())
      .await
      .unwrap()
      .unwrap()
      .unwrap();
    let second = timeout(Duration::from_secs(2), lines.next_line())
      .await
      .unwrap()
      .unwrap()
      .unwrap();
    let mut frames =
      [first, second].map(|line| serde_json::from_str::<RuntimeGuardLogFrame>(&line).unwrap());
    frames.sort_by(|left, right| left.channel.cmp(&right.channel));
    assert_eq!(frames[0].channel, "stderr");
    assert_eq!(frames[0].message, "guarded stderr");
    assert_eq!(frames[1].channel, "stdout");
    assert_eq!(frames[1].message, "guarded stdout");

    runtime.terminate().await.unwrap();

    assert!(runtime.active_identity().is_none());
    assert_eq!(runtime.last_identity(), Some(&child));
  }

  fn write_guard_fake_caddy(dir: &Path) -> std::path::PathBuf {
    let modules = r#"[{"module_name":"http"},{"module_name":"http.encoders.gzip"},{"module_name":"http.encoders.zstd"},{"module_name":"http.handlers.encode"},{"module_name":"http.handlers.file_server"},{"module_name":"http.handlers.headers"},{"module_name":"http.handlers.reverse_proxy"},{"module_name":"http.handlers.rewrite"},{"module_name":"http.handlers.static_response"},{"module_name":"http.handlers.subroute"},{"module_name":"http.matchers.header"},{"module_name":"http.matchers.host"},{"module_name":"http.matchers.method"},{"module_name":"http.matchers.path"},{"module_name":"http.matchers.query"},{"module_name":"http.reverse_proxy.transport.http"},{"module_name":"pki"},{"module_name":"tls"},{"module_name":"tls.issuance.internal"}]"#;

    #[cfg(windows)]
    {
      let path = dir.join("guard-caddy.cmd");
      std::fs::write(
        &path,
        format!(
          r#"@echo off
if "%1"=="version" (
  echo 2.11.3
  exit /b 0
)
if "%1"=="list-modules" (
  echo {modules}
  exit /b 0
)
if "%1"=="run" (
  echo guarded stdout
  echo guarded stderr>&2
  "%SystemRoot%\System32\ping.exe" -n 60 127.0.0.1 >nul
  exit /b 0
)
exit /b 0
"#,
        ),
      )
      .unwrap();
      path
    }

    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let path = dir.join("guard-caddy");
      std::fs::write(
        &path,
        format!(
          r#"#!/bin/sh
if [ "$1" = "version" ]; then
  printf '%s\n' '2.11.3'
  exit 0
fi
if [ "$1" = "list-modules" ]; then
  printf '%s\n' '{modules}'
  exit 0
fi
if [ "$1" = "run" ]; then
  printf '%s\n' 'guarded stdout'
  printf '%s\n' 'guarded stderr' >&2
  /bin/sleep 60
fi
"#,
        ),
      )
      .unwrap();
      let mut permissions = std::fs::metadata(&path).unwrap().permissions();
      permissions.set_mode(0o755);
      std::fs::set_permissions(&path, permissions).unwrap();
      path
    }
  }
}
