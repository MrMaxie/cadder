use anyhow::{Context, Result};
use process_wrap::tokio::ChildWrapper;
#[cfg(unix)]
use process_wrap::tokio::{CommandWrap, KillOnDrop};
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
  #[cfg(unix)]
  active_process_group_id: Option<libc::pid_t>,
  #[cfg(windows)]
  job: WindowsJob,
}

impl ProcessTreeChild {
  pub(crate) fn spawn(command: Command) -> io::Result<Self> {
    #[cfg(windows)]
    {
      use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};

      let mut command = command;
      command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
      command.kill_on_drop(true);
      let child = command.spawn()?;
      let process_id = child
        .id()
        .ok_or_else(|| io::Error::other("spawned Windows child does not expose a process ID"))?;
      let process_handle = child.raw_handle().ok_or_else(|| {
        io::Error::other("spawned Windows child does not expose a process handle")
      })?;
      let job = WindowsJob::create_and_assign(process_handle)?;
      resume_windows_process(process_id)?;
      Ok(Self {
        inner: Box::new(child),
        job,
      })
    }

    #[cfg(unix)]
    {
      let mut command = CommandWrap::from(command);
      command.wrap(process_wrap::tokio::ProcessGroup::leader());
      command.wrap(KillOnDrop);
      let inner = command.spawn()?;
      let process_group_id = inner
        .id()
        .ok_or_else(|| io::Error::other("spawned Unix child does not expose a process ID"))?
        .try_into()
        .map_err(|_| io::Error::other("spawned Unix child process ID exceeds pid_t"))?;
      Ok(Self {
        inner,
        active_process_group_id: Some(process_group_id),
      })
    }
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
    let status = self.inner.try_wait()?;
    #[cfg(unix)]
    if status.is_some()
      && let Some(process_group_id) = self.active_process_group_id
    {
      if !unix_process_group_is_empty(process_group_id)? {
        return Ok(None);
      }
      self.active_process_group_id = None;
    }
    #[cfg(windows)]
    if status.is_some() && self.job.active_processes()? != 0 {
      return Ok(None);
    }
    Ok(status)
  }

  pub(crate) async fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
    let status = self.inner.wait().await?;
    #[cfg(unix)]
    self.wait_until_unix_process_group_empty().await?;
    #[cfg(windows)]
    self.job.wait_until_empty().await?;
    Ok(status)
  }

  #[cfg(test)]
  pub(crate) async fn kill(&mut self) -> io::Result<()> {
    self.start_kill()?;
    self.wait().await?;
    Ok(())
  }

  pub(crate) fn start_kill(&mut self) -> io::Result<()> {
    #[cfg(windows)]
    return self.job.terminate();
    #[cfg(unix)]
    {
      if self.active_process_group_id.is_none() {
        return Ok(());
      }
      self.inner.start_kill()
    }
  }

  pub(crate) async fn terminate_and_join(&mut self, operation: &str) -> Result<()> {
    let kill_error = self.start_kill().err();
    let cleanup_overran = match timeout(CLEANUP_TIMEOUT, self.wait()).await {
      Ok(result) => {
        result.with_context(|| format!("join terminated {operation}"))?;
        false
      }
      Err(_) => {
        self
          .wait()
          .await
          .with_context(|| format!("complete fail-stop containment for {operation}"))?;
        true
      }
    };
    if cleanup_overran {
      let kill_diagnostic = kill_error
        .map(|error| format!("; the initial termination request also failed: {error}"))
        .unwrap_or_default();
      anyhow::bail!(
        "{operation} containment exceeded its {} second cleanup deadline{kill_diagnostic}",
        CLEANUP_TIMEOUT.as_secs()
      );
    }
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

  #[cfg(unix)]
  async fn wait_until_unix_process_group_empty(&mut self) -> io::Result<()> {
    let Some(process_group_id) = self.active_process_group_id else {
      return Ok(());
    };
    loop {
      if unix_process_group_is_empty(process_group_id)? {
        self.active_process_group_id = None;
        return Ok(());
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  }
}

#[cfg(unix)]
fn unix_process_group_is_empty(process_group_id: libc::pid_t) -> io::Result<bool> {
  // SAFETY: signal zero does not deliver a signal; a negative PID queries the process group.
  if unsafe { libc::kill(-process_group_id, 0) } == 0 {
    return Ok(false);
  }

  let error = io::Error::last_os_error();
  match error.raw_os_error() {
    Some(libc::ESRCH) => Ok(true),
    Some(libc::EPERM) => Ok(false),
    _ => Err(error),
  }
}

#[cfg(windows)]
#[derive(Debug)]
struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
// SAFETY: the owned job handle can be used from any thread, and all operations synchronize in the
// Windows kernel. `Drop` closes it exactly once.
unsafe impl Send for WindowsJob {}

#[cfg(windows)]
// SAFETY: the job handle APIs used by this type accept concurrent queries and termination.
unsafe impl Sync for WindowsJob {}

#[cfg(windows)]
impl WindowsJob {
  fn create_and_assign(process: std::os::windows::io::RawHandle) -> io::Result<Self> {
    use std::{ffi::c_void, mem::size_of, ptr};
    use windows_sys::Win32::System::JobObjects::{
      AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
      JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
      SetInformationJobObject,
    };

    // SAFETY: null security attributes and name request a private anonymous job object.
    let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if handle.is_null() {
      return Err(io::Error::last_os_error());
    }
    let job = Self(handle);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `job` owns a live handle and `limits` is the matching information structure.
    if unsafe {
      SetInformationJobObject(
        job.0,
        JobObjectExtendedLimitInformation,
        (&raw const limits).cast::<c_void>(),
        size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
      )
    } == 0
    {
      return Err(io::Error::last_os_error());
    }
    // SAFETY: `process` is the live child handle retained by `tokio::process::Child`.
    if unsafe { AssignProcessToJobObject(job.0, process) } == 0 {
      return Err(io::Error::last_os_error());
    }
    Ok(job)
  }

  fn terminate(&self) -> io::Result<()> {
    use windows_sys::Win32::System::JobObjects::TerminateJobObject;

    // SAFETY: this type owns a live job handle.
    if unsafe { TerminateJobObject(self.0, 1) } == 0 {
      return Err(io::Error::last_os_error());
    }
    Ok(())
  }

  fn active_processes(&self) -> io::Result<u32> {
    use std::{ffi::c_void, mem::size_of, ptr};
    use windows_sys::Win32::System::JobObjects::{
      JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
      QueryInformationJobObject,
    };

    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    // SAFETY: `accounting` is the correctly sized output structure for this information class.
    if unsafe {
      QueryInformationJobObject(
        self.0,
        JobObjectBasicAccountingInformation,
        (&raw mut accounting).cast::<c_void>(),
        size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
        ptr::null_mut(),
      )
    } == 0
    {
      return Err(io::Error::last_os_error());
    }
    Ok(accounting.ActiveProcesses)
  }

  async fn wait_until_empty(&self) -> io::Result<()> {
    while self.active_processes()? != 0 {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Ok(())
  }
}

#[cfg(windows)]
impl Drop for WindowsJob {
  fn drop(&mut self) {
    use windows_sys::Win32::Foundation::CloseHandle;

    // SAFETY: this type owns the handle and drops it exactly once. The job's configured close
    // policy terminates any process still associated with it.
    unsafe { CloseHandle(self.0) };
  }
}

#[cfg(windows)]
fn resume_windows_process(process_id: u32) -> io::Result<()> {
  use std::{mem::size_of, thread::sleep};
  use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::{
      Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
      },
      Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
    },
  };

  const SNAPSHOT_ATTEMPTS: usize = 10;
  for attempt in 0..SNAPSHOT_ATTEMPTS {
    // SAFETY: the snapshot handle is checked and closed before this iteration returns.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
      return Err(io::Error::last_os_error());
    }
    let mut entry = THREADENTRY32 {
      dwSize: size_of::<THREADENTRY32>() as u32,
      ..Default::default()
    };
    let mut found = false;
    let mut resumed = false;
    // SAFETY: `snapshot` is live and `entry` has the required size for ToolHelp iteration.
    let mut has_entry = unsafe { Thread32First(snapshot, &mut entry) } != 0;
    while has_entry {
      if entry.th32OwnerProcessID == process_id {
        found = true;
        // SAFETY: the thread identifier comes from the live ToolHelp snapshot.
        let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
        if thread.is_null() {
          // SAFETY: `snapshot` is a valid owned handle.
          unsafe { CloseHandle(snapshot) };
          return Err(io::Error::last_os_error());
        }
        // Cadder explicitly creates the child with one suspension. Any count other than one means
        // this is not the untouched primary thread created by this spawn operation.
        let previous_count = unsafe { ResumeThread(thread) };
        // SAFETY: `thread` is a valid owned handle returned by `OpenThread`.
        unsafe { CloseHandle(thread) };
        if previous_count == u32::MAX {
          // SAFETY: `snapshot` is a valid owned handle.
          unsafe { CloseHandle(snapshot) };
          return Err(io::Error::last_os_error());
        }
        if previous_count > 1 {
          // SAFETY: `snapshot` is a valid owned handle.
          unsafe { CloseHandle(snapshot) };
          return Err(io::Error::other(format!(
            "child process {process_id} had unexpected suspend count {previous_count}"
          )));
        }
        if previous_count == 1 {
          resumed = true;
          break;
        }
      }
      // SAFETY: `snapshot` and `entry` remain valid throughout iteration.
      has_entry = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
    }
    // SAFETY: `snapshot` is a valid owned handle.
    unsafe { CloseHandle(snapshot) };
    if resumed {
      return Ok(());
    }
    if found {
      return Err(io::Error::other(format!(
        "child process {process_id} was already running before its verified resume"
      )));
    }
    if attempt + 1 < SNAPSHOT_ATTEMPTS {
      sleep(Duration::from_millis(1));
    }
  }
  Err(io::Error::new(
    io::ErrorKind::TimedOut,
    format!("could not find a resumable thread for child process {process_id}"),
  ))
}

impl Drop for ProcessTreeChild {
  fn drop(&mut self) {
    let _ = self.start_kill();
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
mod tests {
  use super::*;
  use std::{fs, path::Path};
  use tokio::io::AsyncWriteExt;
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

  #[tokio::test]
  async fn pinned_caddy_image_metadata_reader_bounds_and_drains_output() {
    let (mut writer, reader) = tokio::io::duplex(64);
    let write = tokio::spawn(async move {
      writer.write_all(b"0123456789").await.unwrap();
      writer.shutdown().await.unwrap();
    });
    let mut pipe = Some(reader);
    let mut bytes = Vec::new();

    let error = read_pipe_bounded(&mut pipe, &mut bytes, 4)
      .await
      .unwrap_err();
    write.await.unwrap();

    assert_eq!(bytes, b"0123");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn wait_keeps_tree_non_empty_after_leader_exits_until_grandchild_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let grandchild_started = temp.path().join("grandchild-started");
    let program = write_orphaned_grandchild_process_tree(temp.path(), &grandchild_started);
    let mut command = Command::new(program);
    command.stdout(std::process::Stdio::null());
    command.stderr(std::process::Stdio::null());
    let mut child = ProcessTreeChild::spawn(command).unwrap();
    let process_group_id = child.id().unwrap() as libc::pid_t;

    wait_for_file(&grandchild_started).await;
    let grandchild_id = fs::read_to_string(&grandchild_started)
      .unwrap()
      .trim()
      .parse::<libc::pid_t>()
      .unwrap();
    wait_for_unix_process_exit(&mut child, process_group_id).await;

    assert!(unix_process_exists(grandchild_id));
    assert_eq!(
      unix_process_group_id(grandchild_id).unwrap(),
      process_group_id
    );
    assert!(child.try_wait().unwrap().is_none());
    assert!(
      timeout(Duration::from_millis(100), child.wait())
        .await
        .is_err(),
      "wait completed while the orphaned grandchild still occupied the process group"
    );

    timeout(
      CLEANUP_TIMEOUT + Duration::from_secs(1),
      child.terminate_and_join("orphaned grandchild test tree"),
    )
    .await
    .expect("orphaned grandchild cleanup exceeded its test deadline")
    .unwrap();

    assert!(unix_process_group_is_empty(process_group_id).unwrap());
  }

  #[cfg(windows)]
  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn pinned_caddy_image_windows_concurrent_children_are_resumed_and_reaped() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = temp.path().join("exit-successfully.cmd");
    fs::write(&fixture, "@exit /b 0\r\n").unwrap();
    let mut children = tokio::task::JoinSet::new();

    for _ in 0..32 {
      let fixture = fixture.clone();
      children.spawn(async move {
        let command = Command::new(fixture);
        ProcessTreeChild::spawn(command)
          .unwrap()
          .wait_for_output(Duration::from_secs(5), "concurrent Windows child")
          .await
          .unwrap()
      });
    }

    while let Some(result) = children.join_next().await {
      assert!(result.unwrap().status.success());
    }
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

  #[cfg(unix)]
  async fn wait_for_unix_process_exit(child: &mut ProcessTreeChild, process_id: libc::pid_t) {
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    while unix_process_exists(process_id) {
      assert!(
        child.try_wait().unwrap().is_none(),
        "process tree reported completion before the leader exited"
      );
      assert!(
        Instant::now() < deadline,
        "process-tree leader did not exit within its deadline"
      );
      sleep(Duration::from_millis(10)).await;
    }
  }

  #[cfg(unix)]
  fn unix_process_exists(process_id: libc::pid_t) -> bool {
    // SAFETY: signal zero only checks whether the process exists and can be signalled.
    if unsafe { libc::kill(process_id, 0) } == 0 {
      return true;
    }
    io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
  }

  #[cfg(unix)]
  fn unix_process_group_id(process_id: libc::pid_t) -> io::Result<libc::pid_t> {
    // SAFETY: the syscall only queries the process identified by this integer PID.
    let process_group_id = unsafe { libc::getpgid(process_id) };
    if process_group_id == -1 {
      return Err(io::Error::last_os_error());
    }
    Ok(process_group_id)
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

  #[cfg(unix)]
  fn write_orphaned_grandchild_process_tree(dir: &Path, started: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join("orphaned-grandchild-process-tree.sh");
    fs::write(
      &path,
      format!(
        r#"#!/bin/sh
(
  /bin/sleep 60 &
  printf '%s\n' "$!" > '{started}'
) &
wait "$!"
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
