mod commands;
mod output;

use std::path::PathBuf;

use cadder_api::AppExit;
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
  name = "cadder",
  bin_name = "cadder",
  version,
  about = "Inspect and manage Cadder routes",
  arg_required_else_help = true
)]
pub(crate) struct Cli {
  #[command(subcommand)]
  pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
  /// Show cadderd, Caddy, and configuration state.
  Status,
  /// Start, stop, or restart cadderd.
  Daemon {
    #[command(subcommand)]
    command: DaemonCommand,
  },
  /// Inspect and manage registered projects.
  Projects {
    #[command(subcommand)]
    command: ProjectsCommand,
  },
  /// Inspect and manage registered domains.
  Domains {
    #[command(subcommand)]
    command: DomainsCommand,
  },
  /// Inspect a local port or terminate its expected owner.
  Port {
    #[command(subcommand)]
    command: PortCommand,
  },
  /// Inspect a Caddyfile registration and its local upstreams.
  Caddyfile {
    #[command(subcommand)]
    command: CaddyfileCommand,
  },
  /// Show explicit runtime and configuration diagnostics.
  Diagnostics,
  /// Read bounded, redacted daemon logs.
  Logs {
    #[command(subcommand)]
    command: LogsCommand,
  },
  /// Open the full-screen operator.
  Tui,
}

#[derive(Debug, Subcommand)]
pub(crate) enum DaemonCommand {
  /// Start cadderd or attach when it is already running.
  Start,
  /// Stop cadderd and its owned Caddy process.
  Stop,
  /// Stop and start cadderd in order.
  Restart,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProjectsCommand {
  /// List registered projects.
  List,
  /// Enable the project registered from this Caddyfile.
  Enable(CaddyfileSelector),
  /// Disable the project registered from this Caddyfile.
  Disable(CaddyfileSelector),
}

#[derive(Debug, Subcommand)]
pub(crate) enum DomainsCommand {
  /// List registered domains.
  List,
  /// Inspect a domain and its local upstream owner.
  Inspect(DomainSelectorArgs),
  /// Enable a domain.
  Enable(DomainSelectorArgs),
  /// Disable a domain.
  Disable(DomainSelectorArgs),
}

#[derive(Debug, Subcommand)]
pub(crate) enum PortCommand {
  /// Show listening processes and matching Cadder routes.
  Inspect { port: u16 },
  /// Terminate the expected process only if it still owns the port.
  Kill {
    port: u16,
    /// Process ID observed by a prior inspect command.
    #[arg(long)]
    pid: u32,
  },
}

#[derive(Debug, Subcommand)]
pub(crate) enum CaddyfileCommand {
  /// Show registration, route, runtime, and local upstream state.
  Inspect(CaddyfileSelector),
}

#[derive(Debug, Subcommand)]
pub(crate) enum LogsCommand {
  /// Read daemon lifecycle and control logs.
  Runtime(LogOptions),
  /// Read logs attributed to a registered project.
  Project(ProjectLogsArgs),
  /// Read logs attributed to a domain.
  Domain(DomainLogsArgs),
}

#[derive(Debug, Args)]
pub(crate) struct CaddyfileSelector {
  /// Path to the source Caddyfile.
  caddyfile: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct DomainSelectorArgs {
  /// Domain name, matched canonically.
  domain: String,
  /// Restrict the match to the project registered from this Caddyfile.
  #[arg(long)]
  caddyfile: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct ProjectLogsArgs {
  /// Path to the source Caddyfile.
  caddyfile: PathBuf,
  #[command(flatten)]
  options: LogOptions,
}

#[derive(Debug, Args)]
pub(crate) struct DomainLogsArgs {
  #[command(flatten)]
  selector: DomainSelectorArgs,
  #[command(flatten)]
  options: LogOptions,
}

#[derive(Debug, Args)]
pub(crate) struct LogOptions {
  /// Number of newest entries to read (1-200).
  #[arg(long, default_value_t = 50, value_parser = parse_log_limit)]
  limit: usize,
}

fn parse_log_limit(raw: &str) -> Result<usize, String> {
  let limit = raw
    .parse::<usize>()
    .map_err(|_| "log limit must be an integer from 1 to 200".to_string())?;
  (1..=200)
    .contains(&limit)
    .then_some(limit)
    .ok_or_else(|| "log limit must be from 1 to 200".to_string())
}

pub(crate) async fn run(cli: Cli) -> AppExit {
  match commands::execute(cli.command).await {
    Ok(()) => AppExit::Success,
    Err(error) => error.report(),
  }
}
