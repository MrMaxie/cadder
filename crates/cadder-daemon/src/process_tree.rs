use anyhow::{Context, Result};
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
    command.wrap(process_wrap::tokio::ProcessGroup::leader());

    #[cfg(windows)]
    {
      use process_wrap::tokio::{CreationFlags, JobObject};
      use windows::Win32::System::Threading::CREATE_NO_WINDOW;

      command.wrap(CreationFlags(CREATE_NO_WINDOW));
      command.wrap(JobObject);
    }

    command.wrap(KillOnDrop);
    command.spawn().map(|inner| Self { inner })
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
    self.inner.wait().await?;
    Ok(())
  }

  pub(crate) fn start_kill(&mut self) -> io::Result<()> {
    self.inner.start_kill()
  }

  pub(crate) async fn wait_for_output(
    mut self,
    deadline: Duration,
    operation: &str,
  ) -> Result<Output> {
    let mut stdout = self.take_stdout();
    let mut stderr = self.take_stderr();
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();

    let completed = timeout(deadline, async {
      let (status, stdout_result, stderr_result) = tokio::join!(
        self.wait(),
        read_pipe(&mut stdout, &mut stdout_bytes),
        read_pipe(&mut stderr, &mut stderr_bytes),
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
        let _ = self.inner.start_kill();
        timeout(CLEANUP_TIMEOUT, async {
          let (status, stdout_result, stderr_result) = tokio::join!(
            self.wait(),
            read_pipe(&mut stdout, &mut stdout_bytes),
            read_pipe(&mut stderr, &mut stderr_bytes),
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

impl Drop for ProcessTreeChild {
  fn drop(&mut self) {
    let _ = self.inner.start_kill();
  }
}

async fn read_pipe<R>(pipe: &mut Option<R>, bytes: &mut Vec<u8>) -> io::Result<()>
where
  R: tokio::io::AsyncRead + Unpin,
{
  if let Some(pipe) = pipe {
    pipe.read_to_end(bytes).await?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{fs, path::Path};
  use tokio::time::{Instant, sleep};

  #[tokio::test]
  async fn kill_terminates_descendants_and_closes_their_pipes() {
    let temp = tempfile::tempdir().unwrap();
    let started = temp.path().join("started");
    let program = write_blocking_process_tree(temp.path(), &started);
    let mut command = Command::new(program);
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    let mut child = ProcessTreeChild::spawn(command).unwrap();

    wait_for_file(&started).await;
    let mut stdout = child.take_stdout();
    let mut stderr = child.take_stderr();
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    timeout(CLEANUP_TIMEOUT, async {
      let (kill_result, stdout_result, stderr_result) = tokio::join!(
        child.kill(),
        read_pipe(&mut stdout, &mut stdout_bytes),
        read_pipe(&mut stderr, &mut stderr_bytes),
      );
      kill_result.unwrap();
      stdout_result.unwrap();
      stderr_result.unwrap();
    })
    .await
    .expect("process-tree cleanup exceeded its deadline");
  }

  async fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    while !path.is_file() {
      assert!(
        Instant::now() < deadline,
        "child process did not publish its start marker"
      );
      sleep(Duration::from_millis(10)).await;
    }
  }

  fn write_blocking_process_tree(dir: &Path, started: &Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
      let path = dir.join("process-tree.cmd");
      fs::write(
        &path,
        format!(
          r#"@echo off
echo started> "{started}"
"%SystemRoot%\System32\ping.exe" -n 60 127.0.0.1 >nul
"#,
          started = started.display()
        ),
      )
      .unwrap();
      path
    }

    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;

      let path = dir.join("process-tree.sh");
      fs::write(
        &path,
        format!(
          r#"#!/bin/sh
: > '{started}'
/bin/sleep 60
"#,
          started = started.display()
        ),
      )
      .unwrap();
      let mut permissions = fs::metadata(&path).unwrap().permissions();
      permissions.set_mode(0o755);
      fs::set_permissions(&path, permissions).unwrap();
      path
    }
  }
}
