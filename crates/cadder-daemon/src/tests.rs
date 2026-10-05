use super::*;
use crate::database::Database;
use cadder_ipc::{QueryStatePayload, QueryStateResponse, new_request_id};
use tokio::time::{Duration, sleep, timeout};

#[tokio::test]
async fn run_daemon_starts_ipc_and_stops_on_shutdown_signal() {
  let temp = tempfile::tempdir().unwrap();
  let runtime_dir = temp.path().join("runtime");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let client = CadderClient::new(paths);
  let (shutdown_tx, shutdown_rx) = watch::channel(false);
  let daemon = tokio::spawn(run_daemon(
    DaemonOptions {
      runtime_dir: Some(runtime_dir),
      real_caddy_override: None,
      caddy_backend: Some(CaddyBackendMode::Mock),
    },
    shutdown_rx,
  ));

  let response = wait_for_query_state(&client).await;
  shutdown_tx.send(true).unwrap();
  let daemon_result = timeout(Duration::from_secs(2), daemon)
    .await
    .unwrap()
    .unwrap();

  assert!(response.accepted);
  assert!(response.snapshot.is_some());
  daemon_result.unwrap();
}

#[tokio::test]
async fn run_daemon_shutdown_closes_sqlite_before_runtime_release() {
  let temp = tempfile::tempdir().unwrap();
  let runtime_dir = temp.path().join("runtime");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let client = CadderClient::new(paths.clone());
  let (shutdown_tx, shutdown_rx) = watch::channel(false);
  let daemon = tokio::spawn(run_daemon(
    DaemonOptions {
      runtime_dir: Some(runtime_dir),
      real_caddy_override: None,
      caddy_backend: Some(CaddyBackendMode::Mock),
    },
    shutdown_rx,
  ));

  wait_for_query_state(&client).await;
  shutdown_tx.send(true).unwrap();
  timeout(Duration::from_secs(2), daemon)
    .await
    .unwrap()
    .unwrap()
    .unwrap();

  assert!(!paths.runtime_dir().join("cadder-ipc.json").exists());
  let database = Database::open(paths.storage_paths()).await.unwrap();
  assert_eq!(database.state().backend, "sqlite");
  database.close().await.unwrap();
}

#[tokio::test]
async fn run_daemon_ignores_and_removes_legacy_runtime_artifacts_after_claiming_endpoint() {
  let temp = tempfile::tempdir().unwrap();
  let runtime_dir = temp.path().join("runtime");
  std::fs::create_dir_all(&runtime_dir).unwrap();
  for name in [
    "cadder.lock",
    "cadder.lock.json",
    "cadder-containment.lock",
    "cadder-containment.json",
    "cadder-launch.lock",
    "cadder-ipc.lock",
    "cadder-ipc.json",
  ] {
    std::fs::write(runtime_dir.join(name), "not-runtime-state").unwrap();
  }
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let client = CadderClient::new(paths.clone());
  let (shutdown_tx, shutdown_rx) = watch::channel(false);
  let daemon = tokio::spawn(run_daemon(
    DaemonOptions {
      runtime_dir: Some(runtime_dir),
      real_caddy_override: None,
      caddy_backend: Some(CaddyBackendMode::Mock),
    },
    shutdown_rx,
  ));

  assert!(wait_for_query_state(&client).await.accepted);
  for name in [
    "cadder.lock",
    "cadder.lock.json",
    "cadder-containment.lock",
    "cadder-containment.json",
    "cadder-launch.lock",
    "cadder-ipc.lock",
    "cadder-ipc.json",
  ] {
    assert!(!paths.runtime_dir().join(name).exists());
  }
  shutdown_tx.send(true).unwrap();
  daemon.await.unwrap().unwrap();
}

#[tokio::test]
async fn second_daemon_attaches_to_the_live_endpoint_without_creating_runtime_artifacts() {
  let temp = tempfile::tempdir().unwrap();
  let runtime_dir = temp.path().join("runtime");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let client = CadderClient::new(paths.clone());
  let (shutdown_tx, shutdown_rx) = watch::channel(false);
  let first = tokio::spawn(run_daemon(
    DaemonOptions {
      runtime_dir: Some(runtime_dir.clone()),
      real_caddy_override: None,
      caddy_backend: Some(CaddyBackendMode::Mock),
    },
    shutdown_rx,
  ));
  assert!(wait_for_query_state(&client).await.accepted);

  let (_second_shutdown_tx, second_shutdown_rx) = watch::channel(false);
  let second = tokio::spawn(run_daemon(
    DaemonOptions {
      runtime_dir: Some(runtime_dir),
      real_caddy_override: None,
      caddy_backend: Some(CaddyBackendMode::Mock),
    },
    second_shutdown_rx,
  ));
  timeout(Duration::from_secs(2), second)
    .await
    .unwrap()
    .unwrap()
    .unwrap();
  assert!(wait_for_query_state(&client).await.accepted);

  shutdown_tx.send(true).unwrap();
  first.await.unwrap().unwrap();
}

async fn wait_for_query_state(client: &CadderClient) -> QueryStateResponse {
  for _ in 0..50 {
    let result = client
      .request::<_>(new_request_id("wait"), &QueryStatePayload::default())
      .await;
    if let Ok(response) = result {
      return response;
    }
    sleep(Duration::from_millis(20)).await;
  }

  panic!("daemon server did not become ready");
}
