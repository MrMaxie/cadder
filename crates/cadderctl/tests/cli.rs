mod support;

use cadder_protocol::{
  ActivationState, LogSeverity, LogStreamIdentity, LogStreamStatus, QueryLogsRequest,
  StateChangeKind, message_types, new_request_id,
};
use cadderctl::{app, cli::CliArgs};
use clap::Parser;
use serde_json::Value;
use std::{process::Command as ProcessCommand, time::Duration};
use support::Harness;

async fn run_cli(args: &[&str]) -> (u8, String, String) {
  let cli = CliArgs::parse_from(std::iter::once("cadderctl").chain(args.iter().copied()));
  let mut stdout = Vec::new();
  let mut stderr = Vec::new();
  let code = app::run(cli, &mut stdout, &mut stderr).await.unwrap();
  (
    code,
    String::from_utf8(stdout).unwrap(),
    String::from_utf8(stderr).unwrap(),
  )
}

fn parse_json(text: &str) -> Value {
  serde_json::from_str(text).unwrap()
}

fn run_binary(args: &[&str]) -> std::process::Output {
  ProcessCommand::new(env!("CARGO_BIN_EXE_cadderctl"))
    .args(args)
    .output()
    .unwrap()
}

#[tokio::test]
async fn daemon_status_json_reports_not_running_runtime_without_failing() {
  let runtime_dir = tempfile::tempdir().unwrap();
  let runtime_dir_arg = runtime_dir.path().to_string_lossy().to_string();

  let (code, stdout, _stderr) = run_cli(&[
    "--runtime-dir",
    &runtime_dir_arg,
    "--output",
    "json",
    "daemon",
    "status",
  ])
  .await;

  assert_eq!(code, 0);
  let json = parse_json(&stdout);
  assert_eq!(json["ok"], true);
  assert_eq!(json["data"]["connectionState"], "notRunning");
}

#[tokio::test]
async fn domains_list_json_returns_typed_error_when_daemon_is_unavailable() {
  let runtime_dir = tempfile::tempdir().unwrap();
  let runtime_dir_arg = runtime_dir.path().to_string_lossy().to_string();

  let (code, stdout, _stderr) = run_cli(&[
    "--runtime-dir",
    &runtime_dir_arg,
    "--output",
    "json",
    "domains",
    "list",
  ])
  .await;

  assert_eq!(code, 3);
  let json = parse_json(&stdout);
  assert_eq!(json["ok"], false);
  assert_eq!(json["error"]["kind"], "daemonUnavailable");
  assert_eq!(json["error"]["exitCode"], 3);
}

#[tokio::test]
async fn entrypoints_and_domains_list_json_report_registered_state() {
  let harness = Harness::start().await;
  harness.register_entrypoint("shim-1").await;
  let runtime_dir = harness.paths.runtime_dir().display().to_string();

  let (entrypoints_code, entrypoints_stdout, _entrypoints_stderr) = run_cli(&[
    "--runtime-dir",
    &runtime_dir,
    "--output",
    "json",
    "entrypoints",
    "list",
  ])
  .await;
  assert_eq!(entrypoints_code, 0);
  let entrypoints_json = parse_json(&entrypoints_stdout);
  assert_eq!(
    entrypoints_json["data"]["entrypoints"]
      .as_array()
      .unwrap()
      .len(),
    1
  );

  let (domains_code, domains_stdout, _domains_stderr) = run_cli(&[
    "--runtime-dir",
    &runtime_dir,
    "--output",
    "json",
    "domains",
    "list",
  ])
  .await;
  assert_eq!(domains_code, 0);
  let domains_json = parse_json(&domains_stdout);
  assert_eq!(domains_json["data"]["domains"].as_array().unwrap().len(), 4);

  harness.shutdown().await;
}

#[tokio::test]
async fn domains_disable_updates_registered_state() {
  let harness = Harness::start().await;
  harness.register_entrypoint("shim-1").await;
  let runtime_dir = harness.paths.runtime_dir().display().to_string();

  let (code, _stdout, _stderr) = run_cli(&[
    "--runtime-dir",
    &runtime_dir,
    "--output",
    "json",
    "domains",
    "disable",
    "app.smarketing.localhost",
    "--registration",
    "shim-1",
  ])
  .await;

  assert_eq!(code, 0);
  let snapshot = harness.snapshot().await;
  let domain = snapshot.registrations[0]
    .registered_domains
    .iter()
    .find(|domain| domain.name.canonical == "app.smarketing.localhost")
    .unwrap();
  assert_eq!(domain.activation_state, ActivationState::Inactive);

  harness.shutdown().await;
}

#[tokio::test]
async fn logs_show_json_returns_entries_and_cursor() {
  let harness = Harness::start().await;
  harness.register_entrypoint("shim-1").await;
  harness.append_domain_log("app.smarketing.localhost", LogSeverity::Info, "first log");
  harness.append_domain_log("app.smarketing.localhost", LogSeverity::Error, "second log");
  let runtime_dir = harness.paths.runtime_dir().display().to_string();

  let (code, stdout, _stderr) = run_cli(&[
    "--runtime-dir",
    &runtime_dir,
    "--output",
    "json",
    "logs",
    "show",
    "--limit",
    "1",
    "domain",
    "app.smarketing.localhost",
    "--registration",
    "shim-1",
  ])
  .await;

  assert_eq!(code, 0);
  let json = parse_json(&stdout);
  let entries = json["data"]["entries"].as_array().unwrap();
  assert_eq!(entries.len(), 1);
  assert_eq!(entries[0]["rawMessage"], "second log");
  let sequence_number = entries[0]["sequenceNumber"].as_u64().unwrap();
  assert_eq!(json["data"]["nextCursor"], format!("seq:{sequence_number}"));

  harness.shutdown().await;
}

#[tokio::test]
async fn state_subscription_yields_initial_snapshot_for_watch_transport() {
  let harness = Harness::start().await;
  harness.register_entrypoint("shim-1").await;

  let mut subscription = harness
    .client
    .subscribe_state(new_request_id("watch-test"))
    .await
    .unwrap();
  let event = tokio::time::timeout(Duration::from_secs(2), subscription.next_event())
    .await
    .unwrap()
    .unwrap();

  assert_eq!(event.change_kind, StateChangeKind::Snapshot);
  assert_eq!(event.snapshot.registrations.len(), 1);

  harness.shutdown().await;
}

#[tokio::test]
async fn log_cursor_progression_supports_tail_transport() {
  let harness = Harness::start().await;
  harness.register_entrypoint("shim-1").await;
  harness.append_domain_log("app.smarketing.localhost", LogSeverity::Info, "first log");
  harness.append_domain_log("app.smarketing.localhost", LogSeverity::Warn, "second log");

  let first_page: cadder_protocol::QueryLogsResponse = harness
    .client
    .request(
      message_types::QUERY_LOGS_REQUEST,
      message_types::QUERY_LOGS_RESPONSE,
      &QueryLogsRequest {
        request_id: new_request_id("tail-page-1"),
        stream: LogStreamIdentity::domain("app.smarketing.localhost"),
        limit: Some(1),
        cursor: None,
        minimum_severity: None,
      },
    )
    .await
    .unwrap();
  assert_eq!(first_page.stream_status, LogStreamStatus::Active);
  assert_eq!(first_page.entries.len(), 1);
  assert_eq!(first_page.entries[0].raw_message, "second log");

  let next_page: cadder_protocol::QueryLogsResponse = harness
    .client
    .request(
      message_types::QUERY_LOGS_REQUEST,
      message_types::QUERY_LOGS_RESPONSE,
      &QueryLogsRequest {
        request_id: new_request_id("tail-page-2"),
        stream: LogStreamIdentity::domain("app.smarketing.localhost"),
        limit: Some(10),
        cursor: first_page.next_cursor.clone(),
        minimum_severity: None,
      },
    )
    .await
    .unwrap();
  assert_eq!(next_page.stream_status, LogStreamStatus::Active);
  assert!(next_page.entries.is_empty());

  harness.shutdown().await;
}

#[tokio::test]
async fn human_commands_cover_status_start_shutdown_lists_and_logs() {
  let harness = Harness::start().await;
  harness.register_entrypoint("shim-1").await;
  harness.append_domain_log(
    "app.smarketing.localhost",
    LogSeverity::Error,
    "domain log entry",
  );
  let runtime_dir = harness.paths.runtime_dir().display().to_string();

  let (code, stdout, stderr) = run_cli(&["--runtime-dir", &runtime_dir, "daemon", "status"]).await;
  assert_eq!(code, 0);
  assert!(stderr.is_empty());
  assert!(stdout.contains("Attached to cadderd."));

  let (code, stdout, stderr) = run_cli(&["daemon", "start", "--runtime-dir", &runtime_dir]).await;
  assert_eq!(code, 0);
  assert!(stderr.is_empty());
  assert!(stdout.contains("cadderd is already running."));

  let (code, stdout, stderr) =
    run_cli(&["entrypoints", "list", "--runtime-dir", &runtime_dir]).await;
  assert_eq!(code, 0);
  assert!(stderr.is_empty());
  assert!(stdout.contains("Registration: shim-1"));

  let (code, stdout, stderr) = run_cli(&[
    "domains",
    "list",
    "--registration",
    "shim-1",
    "--runtime-dir",
    &runtime_dir,
  ])
  .await;
  assert_eq!(code, 0);
  assert!(stderr.is_empty());
  assert!(stdout.contains("Domain: app.smarketing.localhost"));

  let (code, stdout, stderr) =
    run_cli(&["diagnostics", "show", "--runtime-dir", &runtime_dir]).await;
  assert_eq!(code, 0);
  assert!(stderr.is_empty());
  assert!(stdout.contains("No diagnostics."));

  let (code, stdout, stderr) = run_cli(&[
    "logs",
    "show",
    "--limit",
    "5",
    "domain",
    "app.smarketing.localhost",
    "--registration",
    "shim-1",
    "--runtime-dir",
    &runtime_dir,
  ])
  .await;
  assert_eq!(code, 0);
  assert!(stderr.is_empty());
  assert!(stdout.contains("domain log entry"));

  let (code, stdout, stderr) =
    run_cli(&["daemon", "shutdown", "--runtime-dir", &runtime_dir]).await;
  assert_eq!(code, 0);
  assert!(stderr.is_empty());
  assert!(stdout.contains("Daemon shutdown requested."));
}

#[tokio::test]
async fn toggles_and_invalid_selectors_return_typed_results() {
  let harness = Harness::start().await;
  harness.register_entrypoint("shim-1").await;
  harness.register_entrypoint("shim-2").await;
  let runtime_dir = harness.paths.runtime_dir().display().to_string();

  let (code, stdout, _stderr) = run_cli(&[
    "--runtime-dir",
    &runtime_dir,
    "--output",
    "json",
    "entrypoints",
    "disable",
    "shim-1",
  ])
  .await;
  assert_eq!(code, 0);
  assert_eq!(parse_json(&stdout)["ok"], true);

  let (code, stdout, _stderr) = run_cli(&[
    "--runtime-dir",
    &runtime_dir,
    "--output",
    "json",
    "entrypoints",
    "enable",
    "missing",
  ])
  .await;
  assert_eq!(code, 5);
  assert_eq!(parse_json(&stdout)["error"]["kind"], "targetNotFound");

  let (code, stdout, _stderr) = run_cli(&[
    "--runtime-dir",
    &runtime_dir,
    "--output",
    "json",
    "domains",
    "disable",
    "app.smarketing.localhost",
  ])
  .await;
  assert_eq!(code, 6);
  assert_eq!(parse_json(&stdout)["error"]["kind"], "conflictOrRejected");

  let (code, stdout, _stderr) = run_cli(&[
    "--runtime-dir",
    &runtime_dir,
    "--output",
    "json",
    "domains",
    "enable",
    "missing.localhost",
    "--registration",
    "shim-1",
  ])
  .await;
  assert_eq!(code, 5);
  assert_eq!(parse_json(&stdout)["error"]["kind"], "targetNotFound");

  harness.shutdown().await;
}

#[tokio::test]
async fn invalid_output_modes_and_ranges_return_stable_errors() {
  let (code, stdout, _stderr) = run_cli(&["--output", "json", "watch", "status"]).await;
  assert_eq!(code, 2);
  assert_eq!(parse_json(&stdout)["error"]["kind"], "invalidUsage");

  let (code, stdout, _stderr) = run_cli(&["domains", "list", "--output", "jsonl"]).await;
  assert_eq!(code, 2);
  let json = parse_json(&stdout);
  assert_eq!(json["event"], "error");
  assert_eq!(json["error"]["kind"], "invalidUsage");

  let (code, stdout, _stderr) = run_cli(&[
    "--output", "json", "logs", "show", "--limit", "0", "runtime",
  ])
  .await;
  assert_eq!(code, 2);
  assert_eq!(parse_json(&stdout)["error"]["kind"], "invalidUsage");

  let (code, stdout, _stderr) = run_cli(&[
    "--output",
    "jsonl",
    "logs",
    "tail",
    "--poll-interval-ms",
    "10",
    "runtime",
  ])
  .await;
  assert_eq!(code, 2);
  let json = parse_json(&stdout);
  assert_eq!(json["event"], "error");
  assert_eq!(json["error"]["exitCode"], 2);
}

#[test]
fn binary_help_version_and_invalid_usage_exit_codes_are_stable() {
  let help = run_binary(&["--help"]);
  assert_eq!(help.status.code(), Some(0));

  let version = run_binary(&["--version"]);
  assert_eq!(version.status.code(), Some(0));

  let invalid = run_binary(&["logs"]);
  assert_eq!(invalid.status.code(), Some(2));
}
