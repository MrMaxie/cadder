use cadder_daemon::{
  CadderClient, CadderSession, CaddyConfigAdapter, CaddyConfigCoordinator, DaemonServer,
  DaemonState, ProcessRuntime, RealCaddyResolver, RuntimePaths,
};
use cadder_protocol::{
  ActivationState, EntrypointInstanceIdentity, EntrypointRegistration, LogAttributionKind,
  LogSeverity, LogStreamIdentity, OwnerProcessIdentity, QueryStateRequest, QueryStateResponse,
  RegisterEntrypointRequest, RegisterEntrypointResponse, ShimRunMetadata, SourcePath,
  message_types, new_request_id,
};
use chrono::Utc;
use std::{
  fs,
  path::{Path, PathBuf},
  sync::Mutex,
  time::Duration,
};
use tokio::{sync::watch, time::sleep};

const FIXTURE: &str = include_str!(concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/../cadder-daemon/tests/fixtures/SmarketingReverseProxy.Caddyfile"
));

pub struct Harness {
  pub client: CadderClient,
  pub state: DaemonState,
  pub paths: RuntimePaths,
  pub config_path: PathBuf,
  sessions: Mutex<Vec<CadderSession>>,
  shutdown_tx: watch::Sender<bool>,
  _temp: tempfile::TempDir,
}

impl Harness {
  pub async fn start() -> Self {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("runtime");
    let paths = RuntimePaths::resolve(Some(runtime_dir)).unwrap();
    paths.ensure_dirs().unwrap();
    let config_path = temp.path().join("Caddyfile");
    fs::write(&config_path, FIXTURE).unwrap();
    let command_log_path = temp.path().join("fake-caddy-commands.log");
    let fake_caddy_path = write_fake_caddy(temp.path(), &command_log_path);

    let resolver = RealCaddyResolver::new(Some(fake_caddy_path.display().to_string()));
    let adapter = CaddyConfigAdapter::new(resolver.clone());
    let runtime = ProcessRuntime::new(resolver, paths.clone());
    let state = DaemonState::new(CaddyConfigCoordinator::new(adapter, runtime));
    let server = DaemonServer::new(paths.clone(), state.clone());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
      let _ = server.run_until(shutdown_rx).await;
    });

    let client = CadderClient::new(paths.clone());
    for _ in 0..50 {
      if client
        .request::<_, QueryStateResponse>(
          message_types::QUERY_STATE_REQUEST,
          message_types::QUERY_STATE_RESPONSE,
          &QueryStateRequest {
            request_id: new_request_id("wait"),
          },
        )
        .await
        .is_ok()
      {
        return Self {
          client,
          state,
          paths,
          config_path,
          sessions: Mutex::new(Vec::new()),
          shutdown_tx,
          _temp: temp,
        };
      }
      sleep(Duration::from_millis(20)).await;
    }

    panic!("daemon server did not become ready");
  }

  pub async fn register_entrypoint(&self, id: &str) {
    let mut session = CadderSession::connect(&self.paths).await.unwrap();
    let response: RegisterEntrypointResponse = session
      .request(
        message_types::REGISTER_ENTRYPOINT_REQUEST,
        message_types::REGISTER_ENTRYPOINT_RESPONSE,
        &RegisterEntrypointRequest {
          request_id: new_request_id("test-register"),
          registration: registration(id, &format!("{id}-nonce"), &self.config_path),
        },
      )
      .await
      .unwrap();
    assert!(response.accepted, "{}", response.message);
    self.sessions.lock().unwrap().push(session);
  }

  pub async fn snapshot(&self) -> cadder_protocol::GuiStateSnapshot {
    let response: QueryStateResponse = self
      .client
      .request(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: new_request_id("test-query"),
        },
      )
      .await
      .unwrap();
    response.snapshot.unwrap()
  }

  pub fn append_domain_log(&self, domain_key: &str, severity: LogSeverity, message: &str) {
    self.state.logs().append(
      LogStreamIdentity::domain(domain_key),
      severity,
      message,
      LogAttributionKind::Domain,
      None,
    );
  }

  pub async fn shutdown(self) {
    let _ = self.shutdown_tx.send(true);
  }
}

fn registration(id: &str, nonce: &str, config_path: &Path) -> EntrypointRegistration {
  let now = Utc::now();
  let identity = EntrypointInstanceIdentity {
    instance_id: id.to_string(),
    started_at_utc: now,
    shim_session_nonce: nonce.to_string(),
  };
  EntrypointRegistration {
    registration_id: id.to_string(),
    entrypoint_instance: identity.clone(),
    source_working_directory: SourcePath::new(".", None),
    source_config_path: SourcePath::new(
      config_path.display().to_string(),
      Some(config_path.display().to_string()),
    ),
    registered_domains: Vec::new(),
    activation_state: ActivationState::Active,
    owner_process: OwnerProcessIdentity {
      process_id: 1,
      process_start_time_utc: now,
      shim_session_nonce: nonce.to_string(),
      executable_path: None,
    },
    log_stream: LogStreamIdentity::entrypoint(id),
    shim_run: Some(ShimRunMetadata {
      adapter: Some("caddyfile".to_string()),
      raw_arguments: vec!["run".to_string()],
      command_line: "run".to_string(),
    }),
    created_at_utc: now,
    last_heartbeat_utc: now,
  }
}

fn write_fake_caddy(dir: &Path, command_log_path: &Path) -> PathBuf {
  const ADAPTED_JSON: &str = r#"{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["api.smarketing.localhost","app.smarketing.localhost","mailbox.smarketing.localhost","storage.smarketing.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}"#;

  #[cfg(windows)]
  {
    let path = dir.join("fake-caddy.cmd");
    fs::write(
      &path,
      format!(
        r#"@echo off
echo %*>> "{command_log}"
if "%1"=="adapt" (
  echo {adapted_json}
  exit 0
)
if "%1"=="reload" (
  exit 0
)
if "%1"=="run" (
  echo fake runtime started
  exit 0
)
exit 0
"#,
        command_log = command_log_path.display(),
        adapted_json = ADAPTED_JSON,
      ),
    )
    .unwrap();
    path
  }

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-caddy");
    fs::write(
      &path,
      format!(
        r#"#!/usr/bin/env sh
printf '%s\n' "$*" >> '{command_log}'
if [ "$1" = "adapt" ]; then
  printf '%s\n' '{adapted_json}'
  exit 0
fi
if [ "$1" = "reload" ]; then
  exit 0
fi
if [ "$1" = "run" ]; then echo fake runtime started; exit 0; fi
exit 0
"#,
        command_log = command_log_path.display(),
        adapted_json = ADAPTED_JSON,
      ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
  }
}
