mod app;
mod data;
mod logs;
mod tui;
mod widgets;

use cadder_api::{AppExit, DaemonLaunchOptions, OperatorContext};
use clap::{Parser, Subcommand, error::ErrorKind};
use color_eyre::Result;

#[derive(Debug, Parser)]
#[command(
  name = "cadder",
  version,
  about = "Cadder operator",
  arg_required_else_help = true
)]
struct Cli {
  #[command(subcommand)]
  command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
  /// Open the full-screen operator.
  Tui,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> AppExit {
  if let Err(error) = color_eyre::install() {
    eprintln!("Could not initialize Cadder error reporting: {error}");
    return AppExit::IpcFailure;
  }

  match Cli::try_parse() {
    Ok(Cli {
      command: Command::Tui,
    }) => match run_tui().await {
      Ok(()) => AppExit::Success,
      Err(error) => {
        eprintln!("Could not run the Cadder TUI: {error}");
        AppExit::IpcFailure
      }
    },
    Err(error) => {
      let exit = cli_exit(&error);
      let _ = error.print();
      exit
    }
  }
}

fn cli_exit(error: &clap::Error) -> AppExit {
  if error.exit_code() == 0 || error.kind() == ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand {
    AppExit::Success
  } else {
    AppExit::InvalidUsage
  }
}

async fn run_tui() -> Result<()> {
  let context = OperatorContext::new("tui", None, DaemonLaunchOptions::default())?;
  tui::run(context).await
}

#[cfg(test)]
mod tests;
