use super::*;

#[derive(Debug, Clone, Default)]
pub struct DaemonLaunchOptions {
  pub explicit_daemon: Option<PathBuf>,
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
pub(super) enum DaemonStdioMode {
  Null,
  Inherit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DaemonProcessConfig {
  pub(super) stdio: DaemonStdioMode,
  #[cfg(windows)]
  pub(super) creation_flags: u32,
  #[cfg(unix)]
  pub(super) starts_new_session: bool,
}

impl DaemonProcessConfig {
  pub(super) fn for_launch_mode(mode: DaemonLaunchMode) -> Self {
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
pub(super) const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
// Windows ignores CREATE_NO_WINDOW when DETACHED_PROCESS is also set.
const BACKGROUND_DAEMON_CREATION_FLAGS: u32 = CREATE_NO_WINDOW;

#[cfg(all(test, windows))]
mod background_creation_flags_tests {
  use super::{BACKGROUND_DAEMON_CREATION_FLAGS, CREATE_NO_WINDOW};

  const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
  const DETACHED_PROCESS: u32 = 0x0000_0008;

  #[test]
  fn background_daemon_flags_request_no_window_without_detaching() {
    assert_ne!(BACKGROUND_DAEMON_CREATION_FLAGS & CREATE_NO_WINDOW, 0);
    assert_eq!(
      BACKGROUND_DAEMON_CREATION_FLAGS & CREATE_NEW_PROCESS_GROUP,
      0
    );
    assert_eq!(BACKGROUND_DAEMON_CREATION_FLAGS & DETACHED_PROCESS, 0);
  }
}
const DAEMON_READY_ATTEMPTS: usize = 300;
const DAEMON_READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const DAEMON_READY_CONNECT_TIMEOUT: Duration = Duration::from_millis(100);

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
  if daemon_is_ready_for_launch(paths).await? {
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
    if daemon_is_ready_for_launch(paths).await? {
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

pub(super) fn launch_code_for_io(kind: io::ErrorKind) -> LocalIpcErrorCode {
  if kind == io::ErrorKind::PermissionDenied {
    LocalIpcErrorCode::PermissionDenied
  } else {
    LocalIpcErrorCode::DaemonStartFailed
  }
}

pub(crate) async fn is_daemon_ready(paths: &RuntimePaths) -> IpcClientResult<bool> {
  daemon_is_ready(paths).await
}

pub(super) async fn daemon_is_ready(paths: &RuntimePaths) -> IpcClientResult<bool> {
  daemon_is_ready_with_deadlines(paths, IpcClientDeadlines::default()).await
}

async fn daemon_is_ready_for_launch(paths: &RuntimePaths) -> IpcClientResult<bool> {
  daemon_is_ready_with_deadlines(
    paths,
    IpcClientDeadlines {
      connect: DAEMON_READY_CONNECT_TIMEOUT,
      ..IpcClientDeadlines::default()
    },
  )
  .await
}

pub(super) async fn daemon_is_ready_with_deadlines(
  paths: &RuntimePaths,
  deadlines: IpcClientDeadlines,
) -> IpcClientResult<bool> {
  match CadderSession::connect_with_deadlines(paths, deadlines).await {
    Ok(_) => Ok(true),
    Err(error) if daemon_not_ready_yet(&error) => Ok(false),
    Err(error) => Err(error),
  }
}

fn daemon_not_ready_yet(error: &IpcClientError) -> bool {
  error.is_stale_instance()
    || error.is_daemon_unavailable()
    || error.local_error().is_some_and(|error| {
      error.phase() == IpcClientPhase::Connect && error.code() == LocalIpcErrorCode::Timeout
    })
}

fn sibling_binary(name: &str) -> Option<PathBuf> {
  let current = env::current_exe().ok()?;
  let dir = current.parent()?;
  let candidate = dir.join(exe_name(name));
  candidate.is_file().then_some(candidate)
}

pub(super) fn find_on_path(name: &str) -> Option<PathBuf> {
  which::which(exe_name(name)).ok()
}

pub(super) fn exe_name(name: &str) -> String {
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
mod readiness_tests {
  use super::*;

  #[test]
  fn launch_readiness_retries_connection_timeouts() {
    let timeout = super::super::client::connection_timeout_error();
    assert!(daemon_not_ready_yet(&timeout));

    let permanent = super::super::client::connection_error(io::Error::other("connect failed"));
    assert!(!daemon_not_ready_yet(&permanent));
  }
}
