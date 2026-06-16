mod support;

use rmcp::{
  ServiceExt, model::CallToolRequestParams, object, serde_json, transport::TokioChildProcess,
};
use serde_json::Value;
use std::process::Stdio;
use support::Harness;
use tokio::process::Command;

fn parse_structured(result: &rmcp::model::CallToolResult) -> Value {
  result.structured_content.clone().unwrap()
}

async fn spawn_client(runtime_dir: &str) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
  let mut command = Command::new(env!("CARGO_BIN_EXE_cadder-mcp"));
  command
    .arg("--runtime-dir")
    .arg(runtime_dir)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit());
  let transport = TokioChildProcess::new(command).unwrap();
  ().serve(transport).await.unwrap()
}

#[tokio::test]
async fn stdio_smoke_and_tool_discovery_are_stable() {
  let harness = Harness::start().await;
  harness.register_entrypoint("shim-1").await;
  let runtime_dir = harness.paths.runtime_dir().display().to_string();

  let client = spawn_client(&runtime_dir).await;
  let tools = client.peer().list_all_tools().await.unwrap();
  let names = tools
    .iter()
    .map(|tool| tool.name.as_ref())
    .collect::<Vec<_>>();

  assert_eq!(
    names,
    vec![
      "cadder_get_logs",
      "cadder_get_overview",
      "cadder_list_domains",
      "cadder_list_entrypoints",
      "cadder_set_domain_enabled",
      "cadder_set_entrypoint_enabled",
      "cadder_show_diagnostics",
      "cadder_start_daemon",
    ]
  );
  assert!(tools.iter().all(|tool| tool.description.is_some()));

  let overview = client
    .peer()
    .call_tool(CallToolRequestParams::new("cadder_get_overview"))
    .await
    .unwrap();
  assert_eq!(overview.is_error, Some(false));
  assert_eq!(parse_structured(&overview)["counts"]["entrypoints"], 1);

  let toggle = client
    .peer()
    .call_tool(
      CallToolRequestParams::new("cadder_set_domain_enabled").with_arguments(object!({
        "domain": "app.smarketing.localhost",
        "registrationId": "shim-1",
        "enabled": false
      })),
    )
    .await
    .unwrap();
  assert_eq!(toggle.is_error, Some(false));

  let snapshot = harness.snapshot().await;
  let domain = snapshot.registrations[0]
    .registered_domains
    .iter()
    .find(|domain| domain.name.canonical == "app.smarketing.localhost")
    .unwrap();
  assert_eq!(format!("{:?}", domain.activation_state), "Inactive");

  client.cancel().await.unwrap();
  harness.shutdown().await;
}

#[tokio::test]
async fn unavailable_daemon_returns_typed_tool_error() {
  let runtime_dir = tempfile::tempdir().unwrap();
  let runtime_arg = runtime_dir.path().to_string_lossy().to_string();
  let client = spawn_client(&runtime_arg).await;

  let result = client
    .peer()
    .call_tool(CallToolRequestParams::new("cadder_list_domains").with_arguments(object!({})))
    .await
    .unwrap();

  let structured = parse_structured(&result);
  assert_eq!(result.is_error, Some(true));
  assert_eq!(structured["error"]["kind"], "daemonUnavailable");
  assert!(
    structured["error"]["guidance"]
      .as_str()
      .unwrap()
      .contains("cadder_start_daemon")
  );

  client.cancel().await.unwrap();
}

#[tokio::test]
async fn log_results_are_redacted_and_bounded() {
  let harness = Harness::start().await;
  harness.register_entrypoint("shim-1").await;
  harness.append_domain_log(
    "app.smarketing.localhost",
    cadder_protocol::LogSeverity::Error,
    "Authorization: bearer token=abc D:/Projects/Personal/Cadder/Caddyfile exploded",
  );
  let runtime_dir = harness.paths.runtime_dir().display().to_string();
  let client = spawn_client(&runtime_dir).await;

  let result = client
    .peer()
    .call_tool(
      CallToolRequestParams::new("cadder_get_logs").with_arguments(object!({
        "target": "domain",
        "domain": "app.smarketing.localhost",
        "registrationId": "shim-1",
        "limit": 10
      })),
    )
    .await
    .unwrap();

  let structured = parse_structured(&result);
  let message = structured["entries"][0]["message"].as_str().unwrap();
  assert_eq!(result.is_error, Some(false));
  assert!(!message.contains("token=abc"));
  assert!(!message.contains("D:/Projects/Personal/Cadder"));

  client.cancel().await.unwrap();
  harness.shutdown().await;
}

#[tokio::test]
async fn invalid_log_limit_returns_typed_error() {
  let harness = Harness::start().await;
  let runtime_dir = harness.paths.runtime_dir().display().to_string();
  let client = spawn_client(&runtime_dir).await;

  let result = client
    .peer()
    .call_tool(
      CallToolRequestParams::new("cadder_get_logs").with_arguments(object!({
        "target": "runtime",
        "limit": 0
      })),
    )
    .await
    .unwrap();

  let structured = parse_structured(&result);
  assert_eq!(result.is_error, Some(true));
  assert_eq!(structured["error"]["kind"], "invalidUsage");

  client.cancel().await.unwrap();
  harness.shutdown().await;
}
