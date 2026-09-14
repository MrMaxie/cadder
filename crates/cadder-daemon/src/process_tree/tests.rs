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
async fn terminate_and_join_cleans_an_orphaned_grandchild() {
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
  let leader_status = wait_for_unix_process_exit(&mut child).await;

  assert!(leader_status.success());
  assert!(unix_process_exists(grandchild_id));
  assert_eq!(
    unix_process_group_id(grandchild_id).unwrap(),
    process_group_id
  );
  timeout(
    CLEANUP_TIMEOUT + Duration::from_secs(1),
    child.terminate_and_join("orphaned grandchild test tree"),
  )
  .await
  .expect("orphaned grandchild cleanup exceeded its test deadline")
  .unwrap();

  wait_for_unix_process_group_empty(process_group_id).await;
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pinned_caddy_image_windows_concurrent_children_are_reaped() {
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
async fn wait_for_unix_process_exit(child: &mut ProcessTreeChild) -> std::process::ExitStatus {
  let deadline = Instant::now() + CLEANUP_TIMEOUT;
  loop {
    if let Some(status) = child.try_wait().unwrap() {
      return status;
    }
    assert!(
      Instant::now() < deadline,
      "process-tree leader did not exit"
    );
    sleep(Duration::from_millis(10)).await;
  }
}

#[cfg(unix)]
async fn wait_for_unix_process_group_empty(process_group_id: libc::pid_t) {
  let deadline = Instant::now() + CLEANUP_TIMEOUT;
  loop {
    assert!(
      Instant::now() < deadline,
      "orphaned process group was not reaped"
    );
    if unix_process_group_is_empty(process_group_id).unwrap() {
      return;
    }
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

#[cfg(unix)]
fn unix_process_group_is_empty(process_group_id: libc::pid_t) -> io::Result<bool> {
  // SAFETY: a negative PID with signal zero checks the process group without sending a signal.
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
