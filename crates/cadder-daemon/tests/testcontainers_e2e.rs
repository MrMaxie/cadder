use anyhow::{Context, Result, bail, ensure};
use cadder_daemon::{CadderClient, RuntimePaths};
use cadder_ipc::{
  ActivationState, BasicResponse, ConfigApplyStatus, GuiStateSnapshot, LogStreamIdentity,
  LogStreamStatus, QueryLogsPayload, QueryLogsResponse, QueryStatePayload, QueryStateResponse,
  RuntimeStatus, SetDomainEnabledPayload, ShutdownDaemonPayload, new_request_id,
};
use reqwest::{Client as HttpClient, StatusCode, Url, header::HOST, redirect::Policy};
use std::{
  env, fs,
  path::{Path, PathBuf},
  process::Stdio,
  time::Duration,
};
use tempfile::TempDir;
use testcontainers::{
  ContainerAsync, GenericImage, ImageExt,
  core::{AccessMode, IntoContainerPort, Mount, WaitFor},
  runners::AsyncRunner,
};
use tokio::{
  io::AsyncReadExt,
  process::{Child, Command},
  time::{sleep, timeout},
};

const CONTAINER_WORKSPACE: &str = "/workspace";
const CADDY_IMAGE: &str = "caddy";
const CADDY_TAG: &str = "2.11.3-alpine";
const HTTP_PORT: u16 = 80;
const ALPHA_HOST: &str = "alpha.cadder-e2e.localhost";
const BETA_HOST: &str = "beta.cadder-e2e.localhost";
const INVALID_HOST: &str = "invalid.cadder-e2e.localhost";

#[tokio::test]
#[ignore = "requires Docker, Docker CLI, the Caddy container image, and built Cadder binaries"]
async fn docker_e2e_exercises_real_caddy_container() -> Result<()> {
  let mut harness = E2eHarness::start().await?;
  let scenario = run_scenario(&mut harness).await;
  let diagnostics = if scenario.is_err() {
    Some(harness.failure_diagnostics().await)
  } else {
    None
  };
  let cleanup = harness.cleanup().await;
  if let Err(error) = scenario {
    bail!(
      "{error:#}\n{}",
      diagnostics.unwrap_or_else(|| "failure diagnostics unavailable".to_string())
    );
  }
  cleanup?;
  Ok(())
}

async fn run_scenario(harness: &mut E2eHarness) -> Result<()> {
  let alpha = harness.write_project(
    "alpha",
    &format!("http://{ALPHA_HOST} {{\n  respond \"alpha from cadder e2e\"\n}}\n"),
  )?;
  let beta = harness.write_project(
    "beta",
    &format!("http://{BETA_HOST} {{\n  respond \"beta from cadder e2e\"\n}}\n"),
  )?;

  let mut alpha_shim = harness.spawn_shim(&alpha).await?;
  harness
    .wait_for_state("alpha shim registration applied", |snapshot| {
      snapshot.registrations.len() == 1 && snapshot.config.status == ConfigApplyStatus::Applied
    })
    .await?;
  harness
    .wait_for_http_ok(ALPHA_HOST, "alpha from cadder e2e")
    .await?;

  let mut beta_shim = harness.spawn_shim(&beta).await?;
  let snapshot = harness
    .wait_for_state("two shim registrations applied", |snapshot| {
      snapshot.registrations.len() == 2 && snapshot.config.status == ConfigApplyStatus::Applied
    })
    .await?;
  ensure_domains(&snapshot, &[ALPHA_HOST, BETA_HOST])?;
  ensure_eq_status(
    snapshot.runtime.status,
    RuntimeStatus::Running,
    "runtime status",
  )?;
  ensure!(
    snapshot.runtime.process_id.is_some(),
    "runtime process id missing"
  );
  ensure!(
    snapshot.config.diagnostics.is_empty(),
    "unexpected config diagnostics: {:?}",
    snapshot.config.diagnostics
  );
  harness
    .wait_for_http_ok(ALPHA_HOST, "alpha from cadder e2e")
    .await?;
  harness
    .wait_for_http_ok(BETA_HOST, "beta from cadder e2e")
    .await?;
  harness.wait_for_proxy_command("adapt").await?;
  harness.wait_for_proxy_command("run").await?;
  ensure_runtime_control_logs(&harness.client).await?;

  let alpha_registration = registration_id_for_domain(&snapshot, ALPHA_HOST)?;
  harness
    .set_domain_enabled(&alpha_registration, ALPHA_HOST, false)
    .await?;
  harness
    .wait_for_state("alpha domain disabled", |snapshot| {
      domain_activation(snapshot, ALPHA_HOST) == Some(ActivationState::Inactive)
        && snapshot.config.status == ConfigApplyStatus::Applied
    })
    .await?;
  harness.wait_for_proxy_command("reload").await?;
  harness
    .wait_for_http_not_ok_while(ALPHA_HOST, BETA_HOST, "beta from cadder e2e")
    .await?;

  harness
    .set_domain_enabled(&alpha_registration, ALPHA_HOST, true)
    .await?;
  harness
    .wait_for_state("alpha domain enabled again", |snapshot| {
      domain_activation(snapshot, ALPHA_HOST) == Some(ActivationState::Active)
        && snapshot.config.status == ConfigApplyStatus::Applied
    })
    .await?;
  harness
    .wait_for_http_ok(ALPHA_HOST, "alpha from cadder e2e")
    .await?;

  #[cfg(unix)]
  beta_shim.interrupt().await?;
  #[cfg(not(unix))]
  beta_shim.terminate().await?;
  harness
    .wait_for_state("beta shim unregister cleanup", |snapshot| {
      snapshot.registrations.len() == 1 && registration_id_for_domain(snapshot, BETA_HOST).is_err()
    })
    .await?;
  harness
    .wait_for_http_not_ok_while(BETA_HOST, ALPHA_HOST, "alpha from cadder e2e")
    .await?;

  let conflict = harness.write_project(
    "conflict",
    &format!("http://{ALPHA_HOST} {{\n  respond \"conflict from cadder e2e\"\n}}\n"),
  )?;
  let before_conflict = query_state(&harness.client).await?;
  let mut conflict_shim = harness.spawn_shim(&conflict).await?;
  conflict_shim.wait_for_failure().await?;
  let after_conflict = query_state(&harness.client).await?;
  ensure_eq_status(
    after_conflict.registrations.len(),
    before_conflict.registrations.len(),
    "registration count after conflict",
  )?;
  ensure_eq_status(
    after_conflict.config.status,
    ConfigApplyStatus::Applied,
    "config status after conflict",
  )?;
  ensure_eq_status(
    after_conflict.config.effective_config_hash,
    before_conflict.config.effective_config_hash,
    "effective config after conflict",
  )?;
  harness
    .wait_for_http_ok(ALPHA_HOST, "alpha from cadder e2e")
    .await?;

  let invalid = harness.write_project(
    "invalid",
    &format!("http://{INVALID_HOST} {{\n  cadder_unknown_directive\n}}\n"),
  )?;
  let mut invalid_shim = harness.spawn_shim(&invalid).await?;
  let invalid_snapshot = harness
    .wait_for_state("invalid Caddyfile diagnostic", |snapshot| {
      snapshot.config.status == ConfigApplyStatus::Failed
        && snapshot
          .config
          .diagnostics
          .iter()
          .any(|diagnostic| diagnostic.code == "adapt-failed")
    })
    .await?;
  let invalid_diagnostic = invalid_snapshot
    .config
    .diagnostics
    .iter()
    .find(|diagnostic| diagnostic.code == "adapt-failed")
    .context("missing adapt-failed diagnostic")?;
  ensure_source_paths(
    &invalid_diagnostic.source_config_paths,
    &[&invalid.config_path],
  )?;
  invalid_shim.terminate().await?;
  harness
    .wait_for_state("invalid registration removed", |snapshot| {
      snapshot.config.status == ConfigApplyStatus::Applied
        && snapshot.config.diagnostics.is_empty()
        && registration_id_for_domain(snapshot, ALPHA_HOST).is_ok()
    })
    .await?;

  let shutdown = harness.shutdown_daemon_runtime().await?;
  ensure!(
    shutdown.accepted,
    "daemon shutdown rejected: {}",
    shutdown.message
  );
  harness.wait_for_daemon_exit().await?;
  harness.wait_for_proxy_command("stop").await?;
  harness.wait_for_http_not_ok(ALPHA_HOST).await?;

  alpha_shim.terminate().await?;
  Ok(())
}

struct E2eHarness {
  temp: TempDir,
  container: Option<ContainerAsync<GenericImage>>,
  container_host: String,
  container_port: u16,
  http: HttpClient,
  client: CadderClient,
  shim_path: PathBuf,
  proxy_log_path: PathBuf,
  daemon: Option<Child>,
}

impl E2eHarness {
  async fn start() -> Result<Self> {
    let daemon_source = cadder_binary("CADDER_E2E_CADDERD", "cadderd")?;
    let shim_source = cadder_binary("CADDER_E2E_CADDY_SHIM", "caddy")?;
    let temp = tempfile::tempdir().context("create e2e temp directory")?;
    let runtime_dir = temp.path().join("runtime");
    fs::create_dir_all(&runtime_dir).context("create e2e runtime directory")?;
    let daemon_path = install_portable_binary(&daemon_source, &runtime_dir)?;
    let shim_path = install_portable_binary(&shim_source, &runtime_dir)?;

    let mount = Mount::bind_mount(temp.path().display().to_string(), CONTAINER_WORKSPACE)
      .with_access_mode(AccessMode::ReadWrite);
    let container = GenericImage::new(CADDY_IMAGE, CADDY_TAG)
      .with_entrypoint("/bin/sh")
      .with_exposed_port(HTTP_PORT.tcp())
      .with_wait_for(WaitFor::seconds(1))
      .with_cmd(["-c", "sleep infinity"])
      .with_mount(mount)
      .start()
      .await
      .with_context(|| format!("start Docker container {CADDY_IMAGE}:{CADDY_TAG}"))?;
    let container_host = container
      .get_host()
      .await
      .context("read mapped Caddy container host")?
      .to_string();
    let container_port = container
      .get_host_port_ipv4(HTTP_PORT.tcp())
      .await
      .context("read mapped Caddy container HTTP port")?;
    let http = HttpClient::builder()
      .no_proxy()
      .redirect(Policy::none())
      .timeout(Duration::from_secs(3))
      .build()
      .context("build e2e HTTP client")?;
    let proxy_command = write_caddy_proxy(temp.path(), container.id())?;
    let proxy_log_path = temp.path().join("caddy-proxy.log");
    let paths = RuntimePaths::resolve(Some(runtime_dir.clone()))?;
    let client = CadderClient::new(paths);
    let mut daemon = spawn_daemon(&daemon_path, &proxy_command).await?;
    wait_for_daemon(&client, &mut daemon).await?;

    Ok(Self {
      temp,
      container: Some(container),
      container_host,
      container_port,
      http,
      client,
      shim_path,
      proxy_log_path,
      daemon: Some(daemon),
    })
  }

  fn write_project(&self, name: &str, caddyfile: &str) -> Result<Project> {
    let dir = self.temp.path().join("projects").join(name);
    fs::create_dir_all(&dir).with_context(|| format!("create project {}", dir.display()))?;
    let config_path = dir.join("Caddyfile");
    fs::write(&config_path, caddyfile)
      .with_context(|| format!("write Caddyfile {}", config_path.display()))?;
    Ok(Project { dir, config_path })
  }

  async fn spawn_shim(&self, project: &Project) -> Result<ManagedChild> {
    let mut command = Command::new(&self.shim_path);
    command
      .arg("run")
      .arg("--config")
      .arg(&project.config_path)
      .arg("--adapter")
      .arg("caddyfile")
      .current_dir(&project.dir)
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .kill_on_drop(true);
    let child = command
      .spawn()
      .with_context(|| format!("start shim for {}", project.config_path.display()))?;
    Ok(ManagedChild { child: Some(child) })
  }

  async fn wait_for_state<F>(&self, label: &str, mut condition: F) -> Result<GuiStateSnapshot>
  where
    F: FnMut(&GuiStateSnapshot) -> bool,
  {
    let mut last = None;
    let mut last_error = None;
    for _ in 0..300 {
      match query_state(&self.client).await {
        Ok(snapshot) => {
          if condition(&snapshot) {
            return Ok(snapshot);
          }
          last = Some(snapshot);
        }
        Err(error) => last_error = Some(format!("{error:#}")),
      }
      sleep(Duration::from_millis(100)).await;
    }
    bail!(
      "timed out waiting for {label}; last snapshot: {last:#?}; last query error: {}",
      last_error.as_deref().unwrap_or("none")
    );
  }

  async fn set_domain_enabled(
    &self,
    registration_id: &str,
    domain_key: &str,
    enabled: bool,
  ) -> Result<BasicResponse> {
    Ok(
      self
        .client
        .request(
          new_request_id("e2e-domain-toggle"),
          &SetDomainEnabledPayload {
            registration_id: registration_id.to_string(),
            domain_key: domain_key.to_string(),
            enabled,
          },
        )
        .await?,
    )
  }

  async fn shutdown_daemon_runtime(&self) -> Result<BasicResponse> {
    Ok(
      self
        .client
        .request(
          new_request_id("e2e-shutdown"),
          &ShutdownDaemonPayload::default(),
        )
        .await?,
    )
  }

  async fn wait_for_daemon_exit(&mut self) -> Result<()> {
    let daemon = self.daemon.as_mut().context("daemon process missing")?;
    let status = timeout(Duration::from_secs(30), daemon.wait())
      .await
      .context("daemon did not exit after accepting shutdown")??;
    ensure!(status.success(), "daemon shutdown exited with {status}");
    Ok(())
  }

  async fn wait_for_http_ok(&self, host: &str, expected_body: &str) -> Result<()> {
    for _ in 0..300 {
      if let Ok(response) = self.http_get(host).await
        && response.0 == StatusCode::OK
        && response.1.contains(expected_body)
      {
        return Ok(());
      }
      sleep(Duration::from_millis(100)).await;
    }
    bail!("timed out waiting for HTTP 200 from {host}");
  }

  async fn wait_for_http_not_ok(&self, host: &str) -> Result<()> {
    for _ in 0..300 {
      match self.http_get(host).await {
        Ok(response) if response.0 != StatusCode::OK => return Ok(()),
        Err(_) => return Ok(()),
        _ => sleep(Duration::from_millis(100)).await,
      }
    }
    bail!("timed out waiting for {host} to stop serving HTTP 200");
  }

  async fn wait_for_http_not_ok_while(
    &self,
    blocked_host: &str,
    healthy_host: &str,
    healthy_body: &str,
  ) -> Result<()> {
    for _ in 0..300 {
      let blocked_not_ok = match self.http_get(blocked_host).await {
        Ok(response) => response.0 != StatusCode::OK,
        Err(_) => false,
      };
      let healthy_ok = match self.http_get(healthy_host).await {
        Ok(response) => response.0 == StatusCode::OK && response.1.contains(healthy_body),
        Err(_) => false,
      };
      if blocked_not_ok && healthy_ok {
        return Ok(());
      }
      sleep(Duration::from_millis(100)).await;
    }
    bail!(
      "timed out waiting for {blocked_host} to stop serving while {healthy_host} stayed healthy"
    );
  }

  async fn wait_for_proxy_command(&self, command: &str) -> Result<()> {
    for _ in 0..100 {
      let log = fs::read_to_string(&self.proxy_log_path).unwrap_or_default();
      if log
        .lines()
        .any(|line| line.split_whitespace().next() == Some(command))
      {
        return Ok(());
      }
      sleep(Duration::from_millis(100)).await;
    }
    let log = fs::read_to_string(&self.proxy_log_path).unwrap_or_default();
    bail!("expected proxy command `{command}` in log:\n{log}");
  }

  async fn http_get(&self, host: &str) -> Result<(StatusCode, String)> {
    let mut url = Url::parse("http://localhost/").expect("static e2e URL is valid");
    url
      .set_host(Some(&self.container_host))
      .map_err(|_| anyhow::anyhow!("invalid Caddy container host {}", self.container_host))?;
    url
      .set_port(Some(self.container_port))
      .map_err(|_| anyhow::anyhow!("invalid Caddy container port {}", self.container_port))?;
    let response = self
      .http
      .get(url)
      .header(HOST, host)
      .send()
      .await
      .with_context(|| format!("request mapped Caddy endpoint for {host}"))?;
    let status = response.status();
    let body = response.text().await.context("read Caddy response body")?;
    Ok((status, body))
  }

  async fn cleanup(&mut self) -> Result<()> {
    if let Some(mut daemon) = self.daemon.take() {
      if daemon.try_wait()?.is_none() {
        let _ = daemon.kill().await;
      }
      let _ = daemon.wait().await;
    }
    let _ = self.container.take();
    Ok(())
  }

  async fn failure_diagnostics(&mut self) -> String {
    let daemon = match self.daemon.as_mut() {
      Some(daemon) => match daemon.try_wait() {
        Ok(Some(status)) => {
          let mut stderr = String::new();
          if let Some(mut pipe) = daemon.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr).await;
          }
          format!("daemon status: {status}\ndaemon stderr:\n{stderr}")
        }
        Ok(None) => "daemon status: running".to_string(),
        Err(error) => format!("daemon status unavailable: {error}"),
      },
      None => "daemon process missing".to_string(),
    };
    let proxy = fs::read_to_string(&self.proxy_log_path).unwrap_or_default();
    let runtime_files = fs::read_dir(self.temp.path().join("runtime"))
      .map(|entries| {
        entries
          .filter_map(|entry| entry.ok())
          .map(|entry| entry.file_name().to_string_lossy().into_owned())
          .collect::<Vec<_>>()
          .join(", ")
      })
      .unwrap_or_else(|error| format!("unavailable: {error}"));
    format!("{daemon}\nproxy commands:\n{proxy}\nruntime files: {runtime_files}")
  }
}

struct Project {
  dir: PathBuf,
  config_path: PathBuf,
}

struct ManagedChild {
  child: Option<Child>,
}

impl ManagedChild {
  async fn wait_for_failure(&mut self) -> Result<()> {
    let mut child = self.child.take().context("shim process missing")?;
    let status = timeout(Duration::from_secs(10), child.wait())
      .await
      .context("conflicting shim did not exit before timeout")??;
    ensure!(!status.success(), "conflicting shim exited successfully");
    Ok(())
  }

  #[cfg(unix)]
  async fn interrupt(&mut self) -> Result<()> {
    if let Some(child) = self.child.as_ref() {
      let process_id = child.id().context("shim process id missing")?;
      let status = Command::new("kill")
        .arg("-INT")
        .arg(process_id.to_string())
        .status()
        .await
        .context("send SIGINT to shim")?;
      ensure!(status.success(), "kill -INT failed with {status}");
    }

    if let Some(mut child) = self.child.take() {
      match timeout(Duration::from_secs(10), child.wait()).await {
        Ok(status) => {
          let status = status.context("wait for interrupted shim")?;
          ensure!(status.success(), "interrupted shim exited with {status}");
        }
        Err(_) => {
          let _ = child.kill().await;
          let _ = child.wait().await;
          bail!("interrupted shim did not exit before timeout");
        }
      }
    }
    Ok(())
  }

  async fn terminate(&mut self) -> Result<()> {
    if let Some(mut child) = self.child.take() {
      if child.try_wait()?.is_none() {
        let _ = child.kill().await;
      }
      let _ = child.wait().await;
    }
    Ok(())
  }
}

async fn spawn_daemon(daemon_path: &Path, proxy_command: &Path) -> Result<Child> {
  let mut command = Command::new(daemon_path);
  command
    .arg("--real-caddy")
    .arg(proxy_command)
    .env("CADDER_TEST_ALLOW_UNTRUSTED_CADDY", "1")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
  command
    .spawn()
    .with_context(|| format!("start cadderd {}", daemon_path.display()))
}

fn install_portable_binary(source: &Path, runtime_dir: &Path) -> Result<PathBuf> {
  let file_name = source
    .file_name()
    .context("Cadder binary path must include a file name")?;
  let destination = runtime_dir.join(file_name);
  fs::copy(source, &destination).with_context(|| {
    format!(
      "copy portable Cadder binary from {} to {}",
      source.display(),
      destination.display()
    )
  })?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(&destination)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&destination, permissions)?;
  }
  Ok(destination)
}

async fn wait_for_daemon(client: &CadderClient, daemon: &mut Child) -> Result<()> {
  for _ in 0..100 {
    if query_state(client).await.is_ok() {
      return Ok(());
    }
    if let Some(status) = daemon.try_wait()? {
      let mut stderr = String::new();
      if let Some(mut pipe) = daemon.stderr.take() {
        pipe.read_to_string(&mut stderr).await?;
      }
      bail!(
        "cadderd exited before becoming ready: {status}: {}",
        stderr.trim()
      );
    }
    sleep(Duration::from_millis(100)).await;
  }
  bail!("cadderd did not become ready before timeout");
}

async fn query_state(client: &CadderClient) -> Result<GuiStateSnapshot> {
  let response: QueryStateResponse = client
    .request(new_request_id("e2e-query"), &QueryStatePayload::default())
    .await?;
  response.snapshot.context("missing state snapshot")
}

async fn ensure_runtime_control_logs(client: &CadderClient) -> Result<()> {
  let logs: QueryLogsResponse = client
    .request(
      new_request_id("e2e-logs"),
      &QueryLogsPayload {
        stream: LogStreamIdentity::runtime_control(),
        limit: Some(20),
      },
    )
    .await?;
  ensure_eq_status(
    logs.stream_status,
    LogStreamStatus::Active,
    "runtime control log status",
  )?;
  ensure!(
    logs
      .entries
      .iter()
      .any(|entry| entry.raw_message.contains("real Caddy runtime started")),
    "runtime control log did not include start event: {:?}",
    logs.entries
  );
  Ok(())
}

fn ensure_domains(snapshot: &GuiStateSnapshot, expected: &[&str]) -> Result<()> {
  for domain in expected {
    registration_id_for_domain(snapshot, domain)?;
  }
  Ok(())
}

fn registration_id_for_domain(snapshot: &GuiStateSnapshot, domain_key: &str) -> Result<String> {
  snapshot
    .registrations
    .iter()
    .find(|registration| {
      registration
        .registered_domains
        .iter()
        .any(|domain| domain.name.canonical == domain_key)
    })
    .map(|registration| registration.registration_id.clone())
    .with_context(|| format!("missing registration for domain {domain_key}"))
}

fn domain_activation(snapshot: &GuiStateSnapshot, domain_key: &str) -> Option<ActivationState> {
  snapshot
    .registrations
    .iter()
    .flat_map(|registration| registration.registered_domains.iter())
    .find(|domain| domain.name.canonical == domain_key)
    .map(|domain| domain.activation_state)
}

fn ensure_source_paths(actual: &[String], expected: &[&Path]) -> Result<()> {
  for path in expected {
    let rendered = path.display().to_string();
    ensure!(
      actual.contains(&rendered),
      "expected source path {} in {:?}",
      path.display(),
      actual
    );
  }
  Ok(())
}

fn ensure_eq_status<T>(actual: T, expected: T, label: &str) -> Result<()>
where
  T: std::fmt::Debug + PartialEq,
{
  ensure!(
    actual == expected,
    "{label}: expected {expected:?}, got {actual:?}"
  );
  Ok(())
}

fn cadder_binary(env_var: &str, name: &str) -> Result<PathBuf> {
  if let Some(value) = env::var_os(env_var) {
    let path = PathBuf::from(value);
    ensure!(
      path.is_file(),
      "{env_var} points to a missing binary: {}",
      path.display()
    );
    return Ok(path);
  }

  let workspace = workspace_root()?;
  let target_dir = env::var_os("CARGO_TARGET_DIR")
    .map(PathBuf::from)
    .map(|path| {
      if path.is_absolute() {
        path
      } else {
        workspace.join(path)
      }
    })
    .unwrap_or_else(|| workspace.join("target"));
  let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
  let candidate = target_dir.join(profile).join(exe_name(name));
  ensure!(
    candidate.is_file(),
    "missing {}. Build e2e binaries first with `cargo build --package cadder --bin cadderd --bin caddy`, \
     or set {env_var}. Looked for {}",
    name,
    candidate.display()
  );
  Ok(candidate)
}

fn workspace_root() -> Result<PathBuf> {
  Path::new(env!("CARGO_MANIFEST_DIR"))
    .parent()
    .and_then(Path::parent)
    .map(Path::to_path_buf)
    .context("resolve workspace root")
}

fn exe_name(name: &str) -> String {
  if cfg!(windows) {
    format!("{name}.exe")
  } else {
    name.to_string()
  }
}

#[cfg(windows)]
fn write_caddy_proxy(root: &Path, container_id: &str) -> Result<PathBuf> {
  let proxy_dir = root.join("bin");
  fs::create_dir_all(&proxy_dir).context("create proxy command directory")?;
  let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
  let log_path = root.join("caddy-proxy.log");
  write_windows_proxy(&proxy_dir, root, &canonical_root, container_id, &log_path)
}

#[cfg(not(windows))]
fn write_caddy_proxy(root: &Path, container_id: &str) -> Result<PathBuf> {
  let proxy_dir = root.join("bin");
  fs::create_dir_all(&proxy_dir).context("create proxy command directory")?;
  let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
  let log_path = root.join("caddy-proxy.log");
  write_unix_proxy(&proxy_dir, root, &canonical_root, container_id, &log_path)
}

#[cfg(windows)]
fn write_windows_proxy(
  proxy_dir: &Path,
  root: &Path,
  canonical_root: &Path,
  container_id: &str,
  log_path: &Path,
) -> Result<PathBuf> {
  let ps1_path = proxy_dir.join("caddy-proxy.ps1");
  let cmd_path = proxy_dir.join("caddy-proxy.cmd");
  fs::write(
    &ps1_path,
    format!(
      r#"$ErrorActionPreference = 'Stop'
$containerId = @'
{container_id}
'@
$hostRoots = @(
@'
{root}
'@,
@'
{canonical_root}
'@
)
$containerRoot = '{container_workspace}'
$logPath = @'
{log_path}
'@
$translated = foreach ($arg in $args) {{
  $mapped = $arg
  foreach ($rootPath in $hostRoots) {{
    $trimmed = $rootPath.TrimEnd('\')
    if ([string]::Equals($mapped, $trimmed, [System.StringComparison]::OrdinalIgnoreCase)) {{
      $mapped = $containerRoot
      break
    }}
    $prefix = "$trimmed\"
    if ($mapped.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {{
      $relative = $mapped.Substring($prefix.Length).Replace('\', '/')
      $mapped = "$containerRoot/$relative"
      break
    }}
  }}
  $mapped
}}
Add-Content -LiteralPath $logPath -Value ($translated -join ' ')
& docker exec $containerId caddy @translated
exit $LASTEXITCODE
"#,
      container_id = container_id,
      root = root.display(),
      canonical_root = canonical_root.display(),
      container_workspace = CONTAINER_WORKSPACE,
      log_path = log_path.display(),
    ),
  )
  .with_context(|| format!("write PowerShell proxy {}", ps1_path.display()))?;
  fs::write(
    &cmd_path,
    r#"@echo off
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0caddy-proxy.ps1" %*
exit /b %ERRORLEVEL%
"#,
  )
  .with_context(|| format!("write cmd proxy {}", cmd_path.display()))?;
  Ok(cmd_path)
}

#[cfg(not(windows))]
fn write_unix_proxy(
  proxy_dir: &Path,
  root: &Path,
  canonical_root: &Path,
  container_id: &str,
  log_path: &Path,
) -> Result<PathBuf> {
  use std::os::unix::fs::PermissionsExt;

  let proxy_path = proxy_dir.join("caddy-proxy");
  fs::write(
    &proxy_path,
    format!(
      r#"#!/usr/bin/env bash
set -euo pipefail
container_id='{container_id}'
host_root='{root}'
host_canonical_root='{canonical_root}'
container_root='{container_workspace}'
log_path='{log_path}'
translated=()
for arg in "$@"; do
  mapped="$arg"
  if [[ "$mapped" == "$host_root" ]]; then
    mapped="$container_root"
  elif [[ "$mapped" == "$host_root"/* ]]; then
    mapped="$container_root${{mapped#"$host_root"}}"
  elif [[ "$mapped" == "$host_canonical_root" ]]; then
    mapped="$container_root"
  elif [[ "$mapped" == "$host_canonical_root"/* ]]; then
    mapped="$container_root${{mapped#"$host_canonical_root"}}"
  fi
  translated+=("$mapped")
done
printf '%s\n' "${{translated[*]}}" >> "$log_path"
exec docker exec "$container_id" caddy "${{translated[@]}}"
"#,
      container_id = shell_escape(container_id),
      root = shell_escape(&root.display().to_string()),
      canonical_root = shell_escape(&canonical_root.display().to_string()),
      container_workspace = CONTAINER_WORKSPACE,
      log_path = shell_escape(&log_path.display().to_string()),
    ),
  )
  .with_context(|| format!("write Unix proxy {}", proxy_path.display()))?;
  let mut permissions = fs::metadata(&proxy_path)?.permissions();
  permissions.set_mode(0o755);
  fs::set_permissions(&proxy_path, permissions)?;
  Ok(proxy_path)
}

#[cfg(not(windows))]
fn shell_escape(value: &str) -> String {
  value.replace('\'', r#"'\''"#)
}
