//! Hidden runtime-guard process and its authenticated owner channel.

use crate::{
  RuntimeGuardBootstrapAuthenticatedResponse, RuntimeGuardBootstrapRequest,
  RuntimeGuardCommandRequest, RuntimeGuardFinalizedResponse, RuntimeGuardProtocol,
  RuntimeGuardReadyResponse, RuntimeGuardRecordState, RuntimeGuardRequest, RuntimeGuardResponse,
  RuntimeGuardStatusResponse, RuntimePaths,
  ipc_codec::{BoundedNdjsonCodec, encode_json_frame},
  runtime_guard_identity::{PinnedRuntimeGuardImage, current_guard_identity},
  runtime_guard_record::{
    RuntimeGuardGeneration, RuntimeGuardGenerationContext, RuntimeGuardGenerationLock,
    RuntimeGuardRecord, RuntimeGuardReplacementBinding, RuntimeGuardTerminalOutcome,
    RuntimeGuardTerminalReason,
  },
};
use anyhow::{Context, Result, bail, ensure};
use bytes::BytesMut;
use chrono::Utc;
use futures_util::StreamExt;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
  io::{AsyncWriteExt, BufWriter},
  process::{Child, ChildStdin, ChildStdout, Command},
  time::{Instant, sleep, timeout},
};
use tokio_util::codec::{Encoder, FramedRead};

const CONTROL_DEADLINE: Duration = Duration::from_secs(30);
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(20);
const GUARD_EXIT_DEADLINE: Duration = Duration::from_secs(5);

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

  let generation_lock = acquire_generation_lock(&options.paths).await?;
  let guard_identity = current_guard_identity()?;
  generation_lock.publish(&RuntimeGuardRecord::ready(
    context.clone(),
    guard_identity.clone(),
    None,
  ))?;
  send_response(
    &mut requests,
    &mut responses,
    RuntimeGuardResponse::GuardReady(RuntimeGuardReadyResponse {
      context: context.clone(),
      request_id: bootstrap.request_id,
      guard: guard_identity.clone(),
    }),
  )
  .await?;

  loop {
    match requests.next().await {
      Some(Ok(RuntimeGuardRequest::Status(request))) => {
        send_response(
          &mut requests,
          &mut responses,
          RuntimeGuardResponse::Status(RuntimeGuardStatusResponse {
            context: context.clone(),
            request_id: request.request_id,
            state: RuntimeGuardRecordState::Ready,
            guard: guard_identity.clone(),
            child: None,
          }),
        )
        .await?;
      }
      Some(Ok(RuntimeGuardRequest::Finalize(request))) => {
        let terminal = terminal_outcome(RuntimeGuardTerminalReason::CleanFinalize);
        generation_lock.publish(&RuntimeGuardRecord::terminal(
          context.clone(),
          guard_identity,
          None,
          terminal.clone(),
        ))?;
        send_response(
          &mut requests,
          &mut responses,
          RuntimeGuardResponse::Finalized(RuntimeGuardFinalizedResponse {
            context,
            request_id: request.request_id,
            terminal,
          }),
        )
        .await?;
        return Ok(());
      }
      None => {
        generation_lock.publish(&RuntimeGuardRecord::terminal(
          context,
          guard_identity,
          None,
          terminal_outcome(RuntimeGuardTerminalReason::OwnerChannelClosed),
        ))?;
        return Ok(());
      }
      Some(Ok(_)) | Some(Err(_)) => {
        generation_lock.publish(&RuntimeGuardRecord::terminal(
          context,
          guard_identity,
          None,
          terminal_outcome(RuntimeGuardTerminalReason::GuardFailure),
        ))?;
        bail!("runtime guard owner channel failed closed");
      }
    }
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
  responses
    .write_all(&encoded)
    .await
    .context("write runtime guard response")?;
  responses
    .flush()
    .await
    .context("flush runtime guard response")
}

async fn acquire_generation_lock(paths: &RuntimePaths) -> Result<RuntimeGuardGenerationLock> {
  let deadline = Instant::now() + CONTROL_DEADLINE;
  loop {
    if let Some(lock) = RuntimeGuardGenerationLock::try_acquire(paths)? {
      return Ok(lock);
    }
    ensure!(
      Instant::now() < deadline,
      "runtime guard did not receive containment ownership before timeout"
    );
    sleep(LOCK_POLL_INTERVAL).await;
  }
}

fn terminal_outcome(reason: RuntimeGuardTerminalReason) -> RuntimeGuardTerminalOutcome {
  RuntimeGuardTerminalOutcome {
    reason,
    child_exit_code: None,
    completed_at_utc: Utc::now(),
  }
}

pub(crate) struct RuntimeGuardClient {
  child: Child,
  stdin: Option<ChildStdin>,
  responses: FramedRead<ChildStdout, BoundedNdjsonCodec>,
  context: RuntimeGuardGenerationContext,
  next_request_id: u64,
}

impl RuntimeGuardClient {
  pub(crate) async fn spawn_authenticated(
    executable: &Path,
    paths: &RuntimePaths,
    context: RuntimeGuardGenerationContext,
    generation: &RuntimeGuardGeneration,
  ) -> Result<Self> {
    ensure!(
      executable.is_absolute(),
      "runtime guard executable must be absolute"
    );
    let image = PinnedRuntimeGuardImage::open(executable)?;
    let mut command = Command::new(image.path());
    command
      .arg("--runtime-guard")
      .arg("--runtime-dir")
      .arg(paths.runtime_dir())
      .arg("--runtime-guard-instance")
      .arg(&context.daemon_instance_id)
      .arg("--runtime-guard-owner-generation")
      .arg(&context.owner_generation)
      .arg("--runtime-guard-commitment")
      .arg(&context.nonce_commitment)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::inherit());
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
    let mut client = Self {
      child,
      stdin: Some(stdin),
      responses: FramedRead::new(stdout, BoundedNdjsonCodec::new()),
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

  pub(crate) async fn wait_until_ready(&mut self) -> Result<RuntimeGuardReplacementBinding> {
    let response = self.read_response().await?;
    let RuntimeGuardResponse::GuardReady(ready) = response else {
      bail!("runtime guard did not publish its ready identity");
    };
    ensure!(
      ready.context == self.context && ready.request_id == 1,
      "runtime guard ready response belongs to another generation"
    );
    RuntimeGuardRecord::ready(self.context.clone(), ready.guard, None).replacement_binding()
  }

  pub(crate) async fn finalize(mut self) -> Result<()> {
    let request_id = self.next_request_id;
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
