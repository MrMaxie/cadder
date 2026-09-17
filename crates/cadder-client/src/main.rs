mod app;
mod cli;
mod data;
mod inspection;
mod tui;
mod widgets;

use cadder_api::AppExit;
use clap::{Parser, error::ErrorKind};
use cli::Cli;

#[tokio::main(flavor = "current_thread")]
async fn main() -> AppExit {
  if let Err(error) = color_eyre::install() {
    eprintln!("Could not initialize Cadder error reporting: {error}");
    return AppExit::IpcFailure;
  }

  match Cli::try_parse() {
    Ok(cli) => cli::run(cli).await,
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

#[cfg(test)]
mod tests;
