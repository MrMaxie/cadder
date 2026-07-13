use cadder_daemon::{CadderClient, RuntimePaths};
use cadder_protocol::{BasicResponse, QueryStateRequest, QueryStateResponse, message_types};
use std::{
  path::PathBuf,
  process::{Child, Command, Stdio},
  time::{Duration, SystemTime, UNIX_EPOCH},
};

fn unique_runtime_dir(name: &str) -> PathBuf {
  std::env::temp_dir().join(format!(
    "cadderd-{name}-{}-{}",
    std::process::id(),
    SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_nanos()
  ))
}

fn spawn_cadderd(runtime_dir: &PathBuf) -> Child {
  Command::new(env!("CARGO_BIN_EXE_cadderd"))
    .arg("--runtime-dir")
    .arg(runtime_dir)
    .arg("--caddy-backend")
    .arg("mock")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .unwrap()
}

async fn wait_for_state(client: &CadderClient) -> QueryStateResponse {
  for _ in 0..100 {
    let response = client
      .request::<_>(
        message_types::QUERY_STATE_REQUEST,
        message_types::QUERY_STATE_RESPONSE,
        &QueryStateRequest {
          request_id: cadder_protocol::new_request_id("test-query"),
        },
      )
      .await;
    if let Ok(response) = response {
      return response;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
  panic!("cadderd did not become ready");
}

#[tokio::test]
async fn cadderd_binary_serves_ipc_and_shuts_down_cleanly() {
  let runtime_dir = unique_runtime_dir("ipc");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let client = CadderClient::new(paths);
  let mut child = spawn_cadderd(&runtime_dir);

  let state = wait_for_state(&client).await;
  let shutdown: BasicResponse = client
    .request(
      message_types::SHUTDOWN_DAEMON_REQUEST,
      message_types::SHUTDOWN_DAEMON_RESPONSE,
      &cadder_protocol::ShutdownDaemonRequest {
        request_id: cadder_protocol::new_request_id("test-shutdown"),
      },
    )
    .await
    .unwrap();
  let status = child.wait().unwrap();

  assert!(state.accepted);
  assert!(shutdown.accepted, "{}", shutdown.message);
  assert!(status.success(), "{status}");
  let _ = std::fs::remove_dir_all(runtime_dir);
}

#[tokio::test]
async fn cadderd_binary_repeated_start_succeeds_when_runtime_is_already_running() {
  let runtime_dir = unique_runtime_dir("repeated-start");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();
  let client = CadderClient::new(paths);
  let mut child = spawn_cadderd(&runtime_dir);

  let state = wait_for_state(&client).await;
  let repeated = Command::new(env!("CARGO_BIN_EXE_cadderd"))
    .arg("--runtime-dir")
    .arg(&runtime_dir)
    .arg("--caddy-backend")
    .arg("mock")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .unwrap();
  let shutdown: BasicResponse = client
    .request(
      message_types::SHUTDOWN_DAEMON_REQUEST,
      message_types::SHUTDOWN_DAEMON_RESPONSE,
      &cadder_protocol::ShutdownDaemonRequest {
        request_id: cadder_protocol::new_request_id("test-shutdown"),
      },
    )
    .await
    .unwrap();
  let status = child.wait().unwrap();

  assert!(state.accepted);
  assert!(repeated.success(), "{repeated}");
  assert!(shutdown.accepted, "{}", shutdown.message);
  assert!(status.success(), "{status}");
  let _ = std::fs::remove_dir_all(runtime_dir);
}
