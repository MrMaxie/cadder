use super::*;
use crate::cli::Command;
use clap::CommandFactory;

#[test]
fn bare_invocation_prints_help_instead_of_starting_the_tui() {
  let error = Cli::try_parse_from(["cadder"]).expect_err("bare invocation should display help");

  assert_eq!(
    error.kind(),
    ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
  );
  assert_eq!(cli_exit(&error), AppExit::Success);
}

#[test]
fn explicit_help_returns_success() {
  let error = Cli::try_parse_from(["cadder", "--help"])
    .expect_err("help should stop parsing before starting the TUI");

  assert_eq!(cli_exit(&error), AppExit::Success);
}

#[test]
fn version_returns_success() {
  let error = Cli::try_parse_from(["cadder", "--version"])
    .expect_err("version should stop parsing before starting the TUI");

  assert_eq!(cli_exit(&error), AppExit::Success);
}

#[test]
fn tui_command_preserves_read_first_default() {
  let cli = Cli::try_parse_from(["cadder", "tui"]).expect("TUI should parse");

  assert!(matches!(
    cli.command,
    Command::Tui {
      start_daemon: false
    }
  ));
}

#[test]
fn tui_command_accepts_explicit_daemon_start() {
  let cli = Cli::try_parse_from(["cadder", "tui", "--start-daemon"])
    .expect("TUI daemon startup should parse");

  assert!(matches!(cli.command, Command::Tui { start_daemon: true }));
}

#[test]
fn tui_help_explains_daemon_start() {
  let help = Cli::command()
    .find_subcommand_mut("tui")
    .expect("TUI command should exist")
    .render_help()
    .to_string();

  assert!(help.contains("--start-daemon"));
  assert!(help.contains("Start or attach to cadderd before opening the operator"));
}

#[test]
fn inspection_and_listing_commands_parse() {
  for arguments in [
    vec!["cadder", "status"],
    vec!["cadder", "daemon", "start"],
    vec!["cadder", "projects", "list"],
    vec!["cadder", "domains", "list"],
    vec!["cadder", "domains", "inspect", "app.localhost"],
    vec!["cadder", "port", "inspect", "3000"],
    vec!["cadder", "caddyfile", "inspect", "Caddyfile"],
    vec!["cadder", "diagnostics"],
    vec!["cadder", "logs", "runtime"],
  ] {
    Cli::try_parse_from(arguments).expect("supported command should parse");
  }
}

#[test]
fn port_kill_requires_expected_pid() {
  let error = Cli::try_parse_from(["cadder", "port", "kill", "3000"])
    .expect_err("kill should require an expected PID");

  assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
}

#[test]
fn removed_runtime_selection_options_are_rejected() {
  let error = Cli::try_parse_from(["cadder", "--runtime-dir", "runtime-test", "tui"])
    .expect_err("runtime selection options should be rejected");

  assert_eq!(error.kind(), ErrorKind::UnknownArgument);
  assert_eq!(error.exit_code(), 2);
}

#[test]
fn unknown_command_is_rejected_without_starting_the_tui() {
  let error =
    Cli::try_parse_from(["cadder", "web"]).expect_err("unknown commands should be rejected");

  assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
  assert_eq!(error.exit_code(), 2);
}

#[test]
fn root_help_is_generated_from_the_cli_definition() {
  let help = Cli::command().render_help().to_string();

  assert!(help.contains("Inspect and manage Cadder routes"));
  assert!(!help.contains("--runtime-dir"));
  assert!(!help.contains("--profile"));
  assert!(help.contains("tui"));
  assert!(help.contains("projects"));
  assert!(help.contains("domains"));
  assert!(help.contains("port"));
  assert!(help.contains("caddyfile"));
  assert!(help.contains("diagnostics"));
  assert!(help.contains("logs"));
}
