use cadderctl::{app, cli::CliArgs};
use clap::Parser;
use std::{io, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
  let args = match CliArgs::try_parse() {
    Ok(args) => args,
    Err(error) => {
      let exit_code = error.exit_code();
      let _ = error.print();
      return ExitCode::from(u8::try_from(exit_code).unwrap_or(2));
    }
  };

  let mut stdout = io::stdout().lock();
  let mut stderr = io::stderr().lock();
  match app::run(args, &mut stdout, &mut stderr).await {
    Ok(code) => ExitCode::from(code),
    Err(error) => {
      eprintln!("I/O error: {error}");
      ExitCode::FAILURE
    }
  }
}
