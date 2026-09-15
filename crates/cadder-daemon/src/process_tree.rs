use anyhow::{Context, Result};
#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::{io, process::Output, time::Duration};
use tokio::{
  io::AsyncReadExt,
  process::{ChildStderr, ChildStdout, Command},
  time::timeout,
};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct ProcessTreeChild {
  inner: Box<dyn ChildWrapper>,
}

impl ProcessTreeChild {
  pub(crate) fn spawn(command: Command) -> io::Result<Self> {
    let mut command = CommandWrap::from(command);
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);
    command.wrap(KillOnDrop);
    Ok(Self {
      inner: command.spawn()?,
    })
  }

  pub(crate) fn id(&self) -> Option<u32> {
    self.inner.id()
  }

  pub(crate) fn take_stdout(&mut self) -> Option<ChildStdout> {
    self.inner.stdout().take()
  }

  pub(crate) fn take_stderr(&mut self) -> Option<ChildStderr> {
    self.inner.stderr().take()
  }

  pub(crate) fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
    self.inner.try_wait()
  }

  pub(crate) async fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
    self.inner.wait().await
  }

  #[cfg(test)]
  pub(crate) async fn kill(&mut self) -> io::Result<()> {
    self.start_kill()?;
    self.wait().await?;
    Ok(())
  }

  pub(crate) fn start_kill(&mut self) -> io::Result<()> {
    self.inner.start_kill()
  }

  pub(crate) async fn terminate_and_join(&mut self, operation: &str) -> Result<()> {
    self
      .start_kill()
      .with_context(|| format!("terminate {operation}"))?;
    timeout(CLEANUP_TIMEOUT, self.wait())
      .await
      .with_context(|| {
        format!(
          "join terminated {operation} within {} seconds",
          CLEANUP_TIMEOUT.as_secs()
        )
      })?
      .with_context(|| format!("join terminated {operation}"))?;
    Ok(())
  }

  pub(crate) async fn wait_for_output(self, deadline: Duration, operation: &str) -> Result<Output> {
    self
      .wait_for_bounded_output(deadline, operation, usize::MAX)
      .await
  }

  pub(crate) async fn wait_for_bounded_output(
    mut self,
    deadline: Duration,
    operation: &str,
    max_stream_bytes: usize,
  ) -> Result<Output> {
    let mut stdout = self.take_stdout();
    let mut stderr = self.take_stderr();
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();

    let completed = timeout(deadline, async {
      let (status, stdout_result, stderr_result) = tokio::join!(
        self.wait(),
        read_pipe_bounded(&mut stdout, &mut stdout_bytes, max_stream_bytes),
        read_pipe_bounded(&mut stderr, &mut stderr_bytes, max_stream_bytes),
      );
      stdout_result.with_context(|| format!("read {operation} stdout"))?;
      stderr_result.with_context(|| format!("read {operation} stderr"))?;
      Ok::<_, anyhow::Error>(Output {
        status: status.with_context(|| format!("wait for {operation}"))?,
        stdout: std::mem::take(&mut stdout_bytes),
        stderr: std::mem::take(&mut stderr_bytes),
      })
    })
    .await;

    match completed {
      Ok(output) => output,
      Err(_) => {
        let _ = self.start_kill();
        timeout(CLEANUP_TIMEOUT, async {
          let (status, stdout_result, stderr_result) = tokio::join!(
            self.wait(),
            read_pipe_bounded(&mut stdout, &mut stdout_bytes, max_stream_bytes),
            read_pipe_bounded(&mut stderr, &mut stderr_bytes, max_stream_bytes),
          );
          status.with_context(|| format!("join timed-out {operation}"))?;
          stdout_result.with_context(|| format!("drain timed-out {operation} stdout"))?;
          stderr_result.with_context(|| format!("drain timed-out {operation} stderr"))?;
          Ok::<_, anyhow::Error>(())
        })
        .await
        .with_context(|| {
          format!(
            "cleanup timed-out {operation} exceeded {} seconds",
            CLEANUP_TIMEOUT.as_secs()
          )
        })??;
        anyhow::bail!("{operation} timed out after {} ms", deadline.as_millis());
      }
    }
  }
}

#[cfg(test)]
async fn read_pipe<R>(pipe: &mut Option<R>, bytes: &mut Vec<u8>) -> io::Result<()>
where
  R: tokio::io::AsyncRead + Unpin,
{
  read_pipe_bounded(pipe, bytes, usize::MAX).await
}

async fn read_pipe_bounded<R>(
  pipe: &mut Option<R>,
  bytes: &mut Vec<u8>,
  max_bytes: usize,
) -> io::Result<()>
where
  R: tokio::io::AsyncRead + Unpin,
{
  let Some(pipe) = pipe else {
    return Ok(());
  };
  let mut buffer = vec![0_u8; 16 * 1024];
  let mut exceeded = false;
  loop {
    let read = pipe.read(&mut buffer).await?;
    if read == 0 {
      break;
    }
    let remaining = max_bytes.saturating_sub(bytes.len());
    let retained = read.min(remaining);
    bytes.extend_from_slice(&buffer[..retained]);
    exceeded |= retained < read;
  }
  if exceeded {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      format!("process output exceeded the {max_bytes} byte stream limit"),
    ));
  }
  Ok(())
}

#[cfg(test)]
mod tests;
