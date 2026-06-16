use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputMode {
  Human,
  Json,
  Jsonl,
}

impl OutputMode {
  pub fn is_streaming(self) -> bool {
    matches!(self, Self::Jsonl)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SeverityArg {
  Trace,
  Debug,
  Info,
  Warn,
  Error,
  Fatal,
}

#[derive(Debug, Clone, Parser)]
#[command(
  name = "cadderctl",
  version,
  about = "Cadder non-TUI CLI for daemon state, domains, logs, and automation",
  long_about = "Attaches to an existing Cadder daemon to inspect state, toggle managed routes, query diagnostics and logs, and explicitly start or stop cadderd for scripting and operator workflows."
)]
pub struct CliArgs {
  #[arg(
    long,
    global = true,
    help = "Override the Cadder runtime directory used to find daemon IPC and state"
  )]
  pub runtime_dir: Option<PathBuf>,

  #[arg(
    long,
    global = true,
    help = "Path to a cadderd executable for the explicit daemon start command"
  )]
  pub daemon_path: Option<PathBuf>,

  #[arg(
    long,
    global = true,
    help = "Command or path passed to cadderd when explicitly starting the real Caddy binary"
  )]
  pub real_caddy_command: Option<String>,

  #[arg(
    long,
    global = true,
    value_enum,
    default_value_t = OutputMode::Human,
    help = "Output mode: human for operators, json for one-shot scripting, jsonl for streaming commands"
  )]
  pub output: OutputMode,

  #[command(subcommand)]
  pub command: Command,
}

impl CliArgs {
  pub fn command_label(&self) -> &'static str {
    self.command.label()
  }
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
  Daemon {
    #[command(subcommand)]
    command: DaemonCommand,
  },
  Entrypoints {
    #[command(subcommand)]
    command: EntrypointsCommand,
  },
  Domains {
    #[command(subcommand)]
    command: DomainsCommand,
  },
  Diagnostics {
    #[command(subcommand)]
    command: DiagnosticsCommand,
  },
  Logs {
    #[command(subcommand)]
    command: LogsCommand,
  },
  Watch {
    #[command(subcommand)]
    command: WatchCommand,
  },
}

impl Command {
  pub fn label(&self) -> &'static str {
    match self {
      Self::Daemon { command } => command.label(),
      Self::Entrypoints { command } => command.label(),
      Self::Domains { command } => command.label(),
      Self::Diagnostics { command } => command.label(),
      Self::Logs { command } => command.label(),
      Self::Watch { command } => command.label(),
    }
  }
}

#[derive(Debug, Clone, Subcommand)]
pub enum DaemonCommand {
  Status,
  Start,
  Shutdown,
}

impl DaemonCommand {
  pub fn label(&self) -> &'static str {
    match self {
      Self::Status => "daemon status",
      Self::Start => "daemon start",
      Self::Shutdown => "daemon shutdown",
    }
  }
}

#[derive(Debug, Clone, Subcommand)]
pub enum EntrypointsCommand {
  List,
  Enable { registration_id: String },
  Disable { registration_id: String },
}

impl EntrypointsCommand {
  pub fn label(&self) -> &'static str {
    match self {
      Self::List => "entrypoints list",
      Self::Enable { .. } => "entrypoints enable",
      Self::Disable { .. } => "entrypoints disable",
    }
  }
}

#[derive(Debug, Clone, Args)]
pub struct DomainSelectorArgs {
  #[arg(help = "Domain name to select")]
  pub domain: String,

  #[arg(
    long,
    help = "Entrypoint registration ID used to disambiguate repeated domain names"
  )]
  pub registration: Option<String>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum DomainsCommand {
  List {
    #[arg(
      long,
      help = "Only list domains from the selected entrypoint registration"
    )]
    registration: Option<String>,
  },
  Enable {
    #[command(flatten)]
    selector: DomainSelectorArgs,
  },
  Disable {
    #[command(flatten)]
    selector: DomainSelectorArgs,
  },
}

impl DomainsCommand {
  pub fn label(&self) -> &'static str {
    match self {
      Self::List { .. } => "domains list",
      Self::Enable { .. } => "domains enable",
      Self::Disable { .. } => "domains disable",
    }
  }
}

#[derive(Debug, Clone, Subcommand)]
pub enum DiagnosticsCommand {
  Show,
}

impl DiagnosticsCommand {
  pub fn label(&self) -> &'static str {
    "diagnostics show"
  }
}

#[derive(Debug, Clone, Args)]
pub struct LogReadArgs {
  #[arg(
    long,
    default_value_t = 50,
    help = "Maximum number of retained log entries to read per request"
  )]
  pub limit: usize,

  #[arg(
    long,
    value_enum,
    help = "Only include log entries at or above this severity"
  )]
  pub minimum_severity: Option<SeverityArg>,
}

#[derive(Debug, Clone, Args)]
pub struct TailArgs {
  #[command(flatten)]
  pub read: LogReadArgs,

  #[arg(
    long,
    default_value_t = 750,
    help = "Polling interval in milliseconds for tailing current log streams"
  )]
  pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, Subcommand)]
pub enum LogsTargetCommand {
  Runtime,
  Entrypoint {
    registration_id: String,
  },
  Domain {
    domain: String,
    #[arg(
      long,
      help = "Entrypoint registration ID used to disambiguate repeated domain names"
    )]
    registration: Option<String>,
  },
}

#[derive(Debug, Clone, Subcommand)]
pub enum LogsCommand {
  Show {
    #[command(subcommand)]
    target: LogsTargetCommand,
    #[command(flatten)]
    options: LogReadArgs,
  },
  Tail {
    #[command(subcommand)]
    target: LogsTargetCommand,
    #[command(flatten)]
    options: TailArgs,
  },
}

impl LogsCommand {
  pub fn label(&self) -> &'static str {
    match self {
      Self::Show { .. } => "logs show",
      Self::Tail { .. } => "logs tail",
    }
  }
}

#[derive(Debug, Clone, Subcommand)]
pub enum WatchCommand {
  Status,
  Entrypoints,
  Domains,
  Diagnostics,
}

impl WatchCommand {
  pub fn label(&self) -> &'static str {
    match self {
      Self::Status => "watch status",
      Self::Entrypoints => "watch entrypoints",
      Self::Domains => "watch domains",
      Self::Diagnostics => "watch diagnostics",
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use clap::CommandFactory;

  #[test]
  fn command_metadata_matches_release_identity() {
    let command = CliArgs::command();

    assert_eq!(command.get_name(), "cadderctl");
    assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(
      command.get_about().map(ToString::to_string),
      Some(env!("CARGO_PKG_DESCRIPTION").to_string())
    );
  }

  #[test]
  fn long_help_describes_attach_and_start_behavior() {
    let help = CliArgs::command().render_long_help().to_string();

    assert!(
      help.contains("Attaches to an existing Cadder daemon"),
      "long help should describe attach-only behavior: {help}"
    );
    assert!(
      help.contains("explicitly start or stop cadderd"),
      "long help should mention explicit daemon control: {help}"
    );
  }

  #[test]
  fn parses_domain_selector_with_registration() {
    let args = CliArgs::parse_from([
      "cadderctl",
      "domains",
      "disable",
      "app.localhost",
      "--registration",
      "shim-1",
    ]);

    match args.command {
      Command::Domains {
        command:
          DomainsCommand::Disable {
            selector:
              DomainSelectorArgs {
                domain,
                registration,
              },
          },
      } => {
        assert_eq!(domain, "app.localhost");
        assert_eq!(registration.as_deref(), Some("shim-1"));
      }
      other => panic!("unexpected command parsed: {other:?}"),
    }
  }

  #[test]
  fn parses_logs_tail_target_and_options() {
    let args = CliArgs::parse_from([
      "cadderctl",
      "--output",
      "jsonl",
      "logs",
      "tail",
      "--limit",
      "10",
      "--minimum-severity",
      "warn",
      "--poll-interval-ms",
      "1000",
      "domain",
      "app.localhost",
      "--registration",
      "shim-1",
    ]);

    assert_eq!(args.output, OutputMode::Jsonl);
    match args.command {
      Command::Logs {
        command:
          LogsCommand::Tail {
            target:
              LogsTargetCommand::Domain {
                domain,
                registration,
              },
            options,
          },
      } => {
        assert_eq!(domain, "app.localhost");
        assert_eq!(registration.as_deref(), Some("shim-1"));
        assert_eq!(options.read.limit, 10);
        assert_eq!(options.read.minimum_severity, Some(SeverityArg::Warn));
        assert_eq!(options.poll_interval_ms, 1000);
      }
      other => panic!("unexpected command parsed: {other:?}"),
    }
  }

  #[test]
  fn parses_global_flags_after_subcommand_path() {
    let args = CliArgs::parse_from([
      "cadderctl",
      "logs",
      "show",
      "--limit",
      "20",
      "domain",
      "app.localhost",
      "--registration",
      "shim-1",
      "--output",
      "json",
      "--runtime-dir",
      "C:/temp/cadder-runtime",
    ]);

    assert_eq!(args.output, OutputMode::Json);
    assert_eq!(
      args.runtime_dir.as_deref(),
      Some(std::path::Path::new("C:/temp/cadder-runtime"))
    );

    match args.command {
      Command::Logs {
        command:
          LogsCommand::Show {
            target:
              LogsTargetCommand::Domain {
                domain,
                registration,
              },
            options,
          },
      } => {
        assert_eq!(options.limit, 20);
        assert_eq!(domain, "app.localhost");
        assert_eq!(registration.as_deref(), Some("shim-1"));
      }
      other => panic!("unexpected command parsed: {other:?}"),
    }
  }
}
