use super::*;
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
fn tui_is_the_only_operator_command() {
  let cli = Cli::try_parse_from(["cadder", "tui"]).expect("TUI should parse");

  assert!(matches!(cli.command, Command::Tui));
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

  assert!(help.contains("Cadder operator"));
  assert!(!help.contains("--runtime-dir"));
  assert!(!help.contains("--profile"));
  assert!(help.contains("tui"));
}
