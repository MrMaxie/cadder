#[cfg(test)]
use crate::view::{SelectedDomain, resolve_domain};
use crate::{
  cli::{
    CliArgs, Command, DaemonCommand, DiagnosticsCommand, DomainsCommand, LogsCommand,
    LogsTargetCommand, OutputMode, WatchCommand,
  },
  error::{
    CliError, CliErrorKind, daemon_error_indicates_unavailable, format_error_chain, start_guidance,
  },
  render,
  view::{
    ActionResultView, ConnectionStateView, DaemonStartView, LogsView, daemon_status_connected,
    daemon_status_unavailable, diagnostics_view, domains_view, entrypoints_view, map_severity,
  },
};
use anyhow::Result as AnyResult;
use cadder_daemon::{DaemonLaunchOptions, RuntimePaths, StateSubscription};
use cadder_operator::{DomainSelector, OperatorContext};
use cadder_protocol::{
  BasicResponse, GuiStateSnapshot, LogStreamIdentity, QueryStateResponse, ShutdownDaemonRequest,
  StateChangedEvent, message_types, new_request_id,
};
use chrono::Utc;
use serde::Serialize;
use serde_json::json;
use std::{
  io::{self, Write},
  time::Duration,
};

pub async fn run(args: CliArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> io::Result<u8> {
  let output = args.output;
  match execute(args, stdout).await {
    Ok(()) => Ok(0),
    Err(ExecuteError::Cli(error)) => {
      write_error(output, &error, stdout, stderr)?;
      Ok(error.exit_code().code())
    }
    Err(ExecuteError::Io(error)) => Err(error),
  }
}

#[derive(Debug)]
enum ExecuteError {
  Cli(CliError),
  Io(io::Error),
}

impl From<CliError> for ExecuteError {
  fn from(value: CliError) -> Self {
    Self::Cli(value)
  }
}

impl From<io::Error> for ExecuteError {
  fn from(value: io::Error) -> Self {
    Self::Io(value)
  }
}

struct AppContext {
  output: OutputMode,
  operator: OperatorContext,
}

impl AppContext {
  fn new(args: &CliArgs) -> Result<Self, CliError> {
    let command = args.command_label();
    Ok(Self {
      output: args.output,
      operator: OperatorContext::new(
        command,
        args.runtime_dir.clone(),
        DaemonLaunchOptions {
          explicit_daemon: args.daemon_path.clone(),
          real_caddy_command: args.real_caddy_command.clone(),
          shim_path: None,
        },
      )?,
    })
  }

  fn paths(&self) -> &RuntimePaths {
    self.operator.paths()
  }

  async fn query_state_response(&self) -> AnyResult<QueryStateResponse> {
    self.operator.query_state_response().await
  }

  async fn query_snapshot(
    &self,
    command: &'static str,
    action: &str,
  ) -> Result<GuiStateSnapshot, CliError> {
    self.operator.query_snapshot(command, action).await
  }

  async fn request_basic(
    &self,
    command: &'static str,
    action: &str,
    message_type: &str,
    response_type: &str,
    request: &impl Serialize,
  ) -> Result<BasicResponse, CliError> {
    self
      .operator
      .request_basic(command, action, message_type, response_type, request)
      .await
  }

  async fn query_logs(
    &self,
    command: &'static str,
    action: &str,
    stream: LogStreamIdentity,
    limit: usize,
    cursor: Option<String>,
    minimum_severity: Option<cadder_protocol::LogSeverity>,
  ) -> Result<LogsView, CliError> {
    self
      .operator
      .query_logs(command, action, stream, limit, cursor, minimum_severity)
      .await
  }

  async fn subscribe_state(
    &self,
    command: &'static str,
    action: &str,
  ) -> Result<StateSubscription, CliError> {
    self.operator.subscribe_state(command, action).await
  }

  async fn ensure_daemon_running(&self, command: &'static str) -> Result<(), CliError> {
    self.operator.ensure_daemon_running(command).await
  }

  async fn set_entrypoint_enabled(
    &self,
    command: &'static str,
    registration_id: String,
    enabled: bool,
  ) -> Result<BasicResponse, CliError> {
    self
      .operator
      .set_entrypoint_enabled(command, registration_id, enabled)
      .await
  }

  async fn set_domain_enabled(
    &self,
    command: &'static str,
    selector: &DomainSelector,
    enabled: bool,
  ) -> Result<BasicResponse, CliError> {
    self
      .operator
      .set_domain_enabled(command, selector, enabled)
      .await
  }

  async fn resolve_logs_target(
    &self,
    command: &'static str,
    target: cadder_operator::LogsTarget,
  ) -> Result<LogStreamIdentity, CliError> {
    self.operator.resolve_logs_target(command, target).await
  }
}

async fn execute(args: CliArgs, stdout: &mut dyn Write) -> Result<(), ExecuteError> {
  let command = args.command_label();
  validate_output_mode(args.output, &args.command, command)?;
  let context = AppContext::new(&args)?;

  match args.command {
    Command::Daemon { command } => handle_daemon(&context, command, stdout).await,
    Command::Entrypoints { command } => handle_entrypoints(&context, command, stdout).await,
    Command::Domains { command } => handle_domains(&context, command, stdout).await,
    Command::Diagnostics { command } => handle_diagnostics(&context, command, stdout).await,
    Command::Logs { command } => handle_logs(&context, command, stdout).await,
    Command::Watch { command } => handle_watch(&context, command, stdout).await,
  }
}

fn validate_output_mode(
  output: OutputMode,
  command: &Command,
  label: &'static str,
) -> Result<(), CliError> {
  let streaming = matches!(
    command,
    Command::Logs {
      command: LogsCommand::Tail { .. }
    } | Command::Watch { .. }
  );

  if streaming && output == OutputMode::Json {
    return Err(CliError::invalid_usage(
      label,
      "Streaming commands support `human` or `jsonl` output.".to_string(),
      Some("Use `--output jsonl` for streaming automation or omit `--output` for human-readable tail/watch output.".to_string()),
    ));
  }

  if !streaming && output == OutputMode::Jsonl {
    return Err(CliError::invalid_usage(
      label,
      "`jsonl` output is only available for `watch` and `logs tail`.".to_string(),
      Some("Use `--output json` for one-shot commands.".to_string()),
    ));
  }

  match command {
    Command::Logs {
      command: LogsCommand::Show { options, .. },
    } if options.limit == 0 || options.limit > 500 => {
      return Err(CliError::invalid_usage(
        label,
        "`--limit` must be between 1 and 500.".to_string(),
        None,
      ));
    }
    Command::Logs {
      command: LogsCommand::Tail { options, .. },
    } if options.read.limit == 0 || options.read.limit > 500 => {
      return Err(CliError::invalid_usage(
        label,
        "`--limit` must be between 1 and 500.".to_string(),
        None,
      ));
    }
    Command::Logs {
      command: LogsCommand::Tail { options, .. },
    } if options.poll_interval_ms < 50 || options.poll_interval_ms > 60_000 => {
      return Err(CliError::invalid_usage(
        label,
        "`--poll-interval-ms` must be between 50 and 60000.".to_string(),
        None,
      ));
    }
    _ => {}
  }

  Ok(())
}

async fn handle_daemon(
  context: &AppContext,
  command: DaemonCommand,
  stdout: &mut dyn Write,
) -> Result<(), ExecuteError> {
  match command {
    DaemonCommand::Status => {
      let view = match context.query_state_response().await {
        Ok(response) => {
          let Some(snapshot) = response.snapshot else {
            return Err(CliError::new(
              command.label(),
              CliErrorKind::IpcFailure,
              "Cadder daemon returned no state snapshot.".to_string(),
              Some("Retry `cadderctl daemon status` after the daemon finishes its current state transition.".to_string()),
            )
            .into());
          };
          daemon_status_connected(context.paths().runtime_dir(), &snapshot)
        }
        Err(error) if daemon_error_indicates_unavailable(&error) => daemon_status_unavailable(
          context.paths().runtime_dir(),
          connection_state_from_error(&error),
          unavailable_message(context.paths().runtime_dir(), &error),
          Some(start_guidance(context.paths().runtime_dir())),
        ),
        Err(error) => {
          return Err(
            CliError::daemon_request(
              command.label(),
              context.paths().runtime_dir(),
              "query daemon status",
              &error,
            )
            .into(),
          );
        }
      };
      write_one_shot(
        context.output,
        command.label(),
        &view,
        stdout,
        render::write_human_daemon_status,
      )?;
    }
    DaemonCommand::Start => {
      let already_running = match context.query_state_response().await {
        Ok(_) => true,
        Err(error) if daemon_error_indicates_unavailable(&error) => false,
        Err(error) => {
          return Err(
            CliError::daemon_request(
              command.label(),
              context.paths().runtime_dir(),
              "check daemon status before start",
              &error,
            )
            .into(),
          );
        }
      };

      context.ensure_daemon_running(command.label()).await?;
      let snapshot = context
        .query_snapshot(command.label(), "query daemon status after start")
        .await?;
      let view = DaemonStartView {
        started: !already_running,
        message: if already_running {
          "cadderd is already running.".to_string()
        } else {
          "cadderd started.".to_string()
        },
        status: daemon_status_connected(context.paths().runtime_dir(), &snapshot),
      };
      write_one_shot(
        context.output,
        command.label(),
        &view,
        stdout,
        render::write_human_daemon_start,
      )?;
    }
    DaemonCommand::Shutdown => {
      let response = context
        .request_basic(
          command.label(),
          "request daemon shutdown",
          message_types::SHUTDOWN_DAEMON_REQUEST,
          message_types::SHUTDOWN_DAEMON_RESPONSE,
          &ShutdownDaemonRequest {
            request_id: new_request_id("ctl-shutdown"),
          },
        )
        .await?;
      if !response.accepted {
        return Err(
          CliError::conflict_or_rejected(
            command.label(),
            response.message,
            Some("Retry once Cadder has finished the current runtime operation.".to_string()),
          )
          .into(),
        );
      }
      write_one_shot(
        context.output,
        command.label(),
        &ActionResultView {
          message: response.message,
        },
        stdout,
        render::write_human_action,
      )?;
    }
  }

  Ok(())
}

async fn handle_entrypoints(
  context: &AppContext,
  command: crate::cli::EntrypointsCommand,
  stdout: &mut dyn Write,
) -> Result<(), ExecuteError> {
  match command {
    crate::cli::EntrypointsCommand::List => {
      let snapshot = context
        .query_snapshot("entrypoints list", "query entrypoints")
        .await?;
      let view = entrypoints_view(&snapshot);
      write_one_shot(
        context.output,
        "entrypoints list",
        &view,
        stdout,
        render::write_human_entrypoints,
      )?;
    }
    crate::cli::EntrypointsCommand::Enable { registration_id } => {
      toggle_entrypoint(context, "entrypoints enable", registration_id, true, stdout).await?;
    }
    crate::cli::EntrypointsCommand::Disable { registration_id } => {
      toggle_entrypoint(
        context,
        "entrypoints disable",
        registration_id,
        false,
        stdout,
      )
      .await?;
    }
  }
  Ok(())
}

async fn toggle_entrypoint(
  context: &AppContext,
  command: &'static str,
  registration_id: String,
  enabled: bool,
  stdout: &mut dyn Write,
) -> Result<(), ExecuteError> {
  let response = context
    .set_entrypoint_enabled(command, registration_id, enabled)
    .await?;

  write_one_shot(
    context.output,
    command,
    &ActionResultView {
      message: response.message,
    },
    stdout,
    render::write_human_action,
  )?;
  Ok(())
}

async fn handle_domains(
  context: &AppContext,
  command: DomainsCommand,
  stdout: &mut dyn Write,
) -> Result<(), ExecuteError> {
  match command {
    DomainsCommand::List { registration } => {
      let snapshot = context
        .query_snapshot("domains list", "query domains")
        .await?;
      let view = domains_view(&snapshot, registration.as_deref());
      write_one_shot(
        context.output,
        "domains list",
        &view,
        stdout,
        render::write_human_domains,
      )?;
    }
    DomainsCommand::Enable { selector } => {
      toggle_domain(context, "domains enable", selector, true, stdout).await?;
    }
    DomainsCommand::Disable { selector } => {
      toggle_domain(context, "domains disable", selector, false, stdout).await?;
    }
  }
  Ok(())
}

async fn toggle_domain(
  context: &AppContext,
  command: &'static str,
  selector: crate::cli::DomainSelectorArgs,
  enabled: bool,
  stdout: &mut dyn Write,
) -> Result<(), ExecuteError> {
  let response = context
    .set_domain_enabled(
      command,
      &DomainSelector {
        domain: selector.domain,
        registration: selector.registration,
      },
      enabled,
    )
    .await?;
  write_one_shot(
    context.output,
    command,
    &ActionResultView {
      message: response.message,
    },
    stdout,
    render::write_human_action,
  )?;
  Ok(())
}

async fn handle_diagnostics(
  context: &AppContext,
  command: DiagnosticsCommand,
  stdout: &mut dyn Write,
) -> Result<(), ExecuteError> {
  match command {
    DiagnosticsCommand::Show => {
      let snapshot = context
        .query_snapshot(command.label(), "query diagnostics")
        .await?;
      let view = diagnostics_view(&snapshot);
      write_one_shot(
        context.output,
        command.label(),
        &view,
        stdout,
        render::write_human_diagnostics,
      )?;
    }
  }
  Ok(())
}

async fn handle_logs(
  context: &AppContext,
  command: LogsCommand,
  stdout: &mut dyn Write,
) -> Result<(), ExecuteError> {
  match command {
    LogsCommand::Show { target, options } => {
      let target = resolve_logs_target(context, "logs show", target).await?;
      let view = context
        .query_logs(
          "logs show",
          "query retained logs",
          target,
          options.limit,
          None,
          map_severity(options.minimum_severity),
        )
        .await?;
      write_one_shot(
        context.output,
        "logs show",
        &view,
        stdout,
        render::write_human_logs,
      )?;
    }
    LogsCommand::Tail { target, options } => {
      let target = resolve_logs_target(context, "logs tail", target).await?;
      let mut cursor = None;
      let mut last_status = None;
      let minimum_severity = map_severity(options.read.minimum_severity);
      let mut first_page = true;

      loop {
        let view = context
          .query_logs(
            "logs tail",
            "continue log tail",
            target.clone(),
            options.read.limit,
            cursor.clone(),
            minimum_severity,
          )
          .await?;
        if let Some(next_cursor) = &view.next_cursor {
          cursor = Some(next_cursor.clone());
        }
        let should_emit = first_page
          || !view.entries.is_empty()
          || view.has_gap
          || view.truncated_by_retention
          || last_status.is_some_and(|status| status != view.stream_status);
        if should_emit {
          if context.output == OutputMode::Human && !first_page {
            writeln!(stdout)?;
          }
          write_logs_stream_event(context.output, "logsPage", "logs tail", &view, stdout)?;
        }
        last_status = Some(view.stream_status);
        first_page = false;

        tokio::select! {
          _ = tokio::signal::ctrl_c() => return Ok(()),
          _ = tokio::time::sleep(Duration::from_millis(options.poll_interval_ms)) => {}
        }
      }
    }
  }

  Ok(())
}

async fn handle_watch(
  context: &AppContext,
  command: WatchCommand,
  stdout: &mut dyn Write,
) -> Result<(), ExecuteError> {
  let mut subscription = context
    .subscribe_state(command.label(), "subscribe to daemon state")
    .await?;
  let mut first = true;
  loop {
    let event = tokio::select! {
      _ = tokio::signal::ctrl_c() => return Ok(()),
      result = subscription.next_event() => {
        result.map_err(|error| CliError::daemon_request(command.label(), context.paths().runtime_dir(), "stream daemon state", &error))?
      }
    };

    if context.output == OutputMode::Human && !first {
      writeln!(stdout)?;
    }
    write_watch_event(context, &command, &event, stdout)?;
    first = false;
  }
}

#[cfg(test)]
fn select_domain(
  command: &'static str,
  snapshot: &GuiStateSnapshot,
  selector: &crate::cli::DomainSelectorArgs,
) -> Result<SelectedDomain, CliError> {
  resolve_domain(snapshot, &selector.domain, selector.registration.as_deref()).map_err(|error| {
    match error {
      crate::view::DomainResolveError::NotFound {
        canonical_domain,
        requested_registration,
      } => CliError::target_not_found(
        command,
        match requested_registration {
          Some(registration_id) => format!(
            "Domain `{canonical_domain}` was not found under entrypoint `{registration_id}`."
          ),
          None => format!("Domain `{canonical_domain}` was not found."),
        },
        Some(
          "Run `cadderctl domains list` to inspect the current domain registrations.".to_string(),
        ),
      ),
      crate::view::DomainResolveError::Ambiguous {
        canonical_domain,
        matches,
      } => CliError::conflict_or_rejected(
        command,
        format!(
          "Domain `{canonical_domain}` matches multiple entrypoints: {}.",
          matches.join(", ")
        ),
        Some(
          "Retry the command with `--registration <id>` to choose the exact entrypoint."
            .to_string(),
        ),
      ),
    }
  })
}

async fn resolve_logs_target(
  context: &AppContext,
  command: &'static str,
  target: LogsTargetCommand,
) -> Result<LogStreamIdentity, CliError> {
  match target {
    LogsTargetCommand::Runtime => {
      context
        .resolve_logs_target(command, cadder_operator::LogsTarget::Runtime)
        .await
    }
    LogsTargetCommand::Entrypoint { registration_id } => {
      context
        .resolve_logs_target(
          command,
          cadder_operator::LogsTarget::Entrypoint { registration_id },
        )
        .await
    }
    LogsTargetCommand::Domain {
      domain,
      registration,
    } => {
      context
        .resolve_logs_target(
          command,
          cadder_operator::LogsTarget::Domain(DomainSelector {
            domain,
            registration,
          }),
        )
        .await
    }
  }
}

fn write_one_shot<T: Serialize>(
  output: OutputMode,
  command: &'static str,
  value: &T,
  stdout: &mut dyn Write,
  human: fn(&T, &mut dyn Write) -> io::Result<()>,
) -> io::Result<()> {
  match output {
    OutputMode::Human => human(value, stdout),
    OutputMode::Json => render::write_json_success(command, value, stdout),
    OutputMode::Jsonl => unreachable!("jsonl is validated before execution"),
  }?;
  stdout.flush()
}

fn write_logs_stream_event(
  output: OutputMode,
  event: &'static str,
  command: &'static str,
  view: &LogsView,
  stdout: &mut dyn Write,
) -> io::Result<()> {
  match output {
    OutputMode::Human => render::write_human_logs(view, stdout),
    OutputMode::Json => unreachable!("json output is invalid for streaming commands"),
    OutputMode::Jsonl => render::write_jsonl(
      &json!({
        "event": event,
        "command": command,
        "generatedAtUtc": Utc::now(),
        "data": view,
      }),
      stdout,
    ),
  }?;
  stdout.flush()
}

fn write_watch_event(
  context: &AppContext,
  command: &WatchCommand,
  event: &StateChangedEvent,
  stdout: &mut dyn Write,
) -> io::Result<()> {
  match command {
    WatchCommand::Status => {
      let status = daemon_status_connected(context.paths().runtime_dir(), &event.snapshot);
      match context.output {
        OutputMode::Human => {
          writeln!(
            stdout,
            "Event: {:?} (sequence {})",
            event.change_kind, event.sequence_number
          )?;
          if let Some(registration_id) = &event.registration_id {
            writeln!(stdout, "Registration: {registration_id}")?;
          }
          render::write_human_daemon_status(&status, stdout)
        }
        OutputMode::Jsonl => render::write_jsonl(
          &render::WatchEnvelope {
            event: "stateChanged",
            command: command.label(),
            generated_at_utc: Utc::now(),
            sequence_number: Some(event.sequence_number),
            change_kind: Some(format!("{:?}", event.change_kind)),
            registration_id: event.registration_id.as_deref(),
            data: &status,
          },
          stdout,
        ),
        OutputMode::Json => unreachable!("json output is invalid for watch commands"),
      }
    }
    WatchCommand::Entrypoints => {
      let entrypoints = entrypoints_view(&event.snapshot);
      match context.output {
        OutputMode::Human => {
          writeln!(
            stdout,
            "Event: {:?} (sequence {})",
            event.change_kind, event.sequence_number
          )?;
          render::write_human_entrypoints(&entrypoints, stdout)
        }
        OutputMode::Jsonl => render::write_jsonl(
          &render::WatchEnvelope {
            event: "stateChanged",
            command: command.label(),
            generated_at_utc: Utc::now(),
            sequence_number: Some(event.sequence_number),
            change_kind: Some(format!("{:?}", event.change_kind)),
            registration_id: event.registration_id.as_deref(),
            data: &entrypoints,
          },
          stdout,
        ),
        OutputMode::Json => unreachable!("json output is invalid for watch commands"),
      }
    }
    WatchCommand::Domains => {
      let domains = domains_view(&event.snapshot, None);
      match context.output {
        OutputMode::Human => {
          writeln!(
            stdout,
            "Event: {:?} (sequence {})",
            event.change_kind, event.sequence_number
          )?;
          render::write_human_domains(&domains, stdout)
        }
        OutputMode::Jsonl => render::write_jsonl(
          &render::WatchEnvelope {
            event: "stateChanged",
            command: command.label(),
            generated_at_utc: Utc::now(),
            sequence_number: Some(event.sequence_number),
            change_kind: Some(format!("{:?}", event.change_kind)),
            registration_id: event.registration_id.as_deref(),
            data: &domains,
          },
          stdout,
        ),
        OutputMode::Json => unreachable!("json output is invalid for watch commands"),
      }
    }
    WatchCommand::Diagnostics => {
      let diagnostics = diagnostics_view(&event.snapshot);
      match context.output {
        OutputMode::Human => {
          writeln!(
            stdout,
            "Event: {:?} (sequence {})",
            event.change_kind, event.sequence_number
          )?;
          render::write_human_diagnostics(&diagnostics, stdout)
        }
        OutputMode::Jsonl => render::write_jsonl(
          &render::WatchEnvelope {
            event: "stateChanged",
            command: command.label(),
            generated_at_utc: Utc::now(),
            sequence_number: Some(event.sequence_number),
            change_kind: Some(format!("{:?}", event.change_kind)),
            registration_id: event.registration_id.as_deref(),
            data: &diagnostics,
          },
          stdout,
        ),
        OutputMode::Json => unreachable!("json output is invalid for watch commands"),
      }
    }
  }?;
  stdout.flush()
}

fn write_error(
  output: OutputMode,
  error: &CliError,
  stdout: &mut dyn Write,
  stderr: &mut dyn Write,
) -> io::Result<()> {
  match output {
    OutputMode::Human => render::write_human_error(error, stderr),
    OutputMode::Json => render::write_json_error(error, stdout),
    OutputMode::Jsonl => render::write_jsonl(
      &json!({
        "event": "error",
        "command": error.command,
        "generatedAtUtc": Utc::now(),
        "error": {
          "kind": error.kind,
          "exitCode": error.exit_code().code(),
          "message": error.message,
          "guidance": error.guidance,
        }
      }),
      stdout,
    ),
  }?;

  match output {
    OutputMode::Human => stderr.flush(),
    OutputMode::Json | OutputMode::Jsonl => stdout.flush(),
  }
}

fn unavailable_message(runtime_dir: &std::path::Path, error: &anyhow::Error) -> String {
  match connection_state_from_error(error) {
    ConnectionStateView::NotRunning => format!(
      "Cadder daemon is not running for runtime `{}`.",
      runtime_dir.display()
    ),
    ConnectionStateView::ConnectionFailed => format!(
      "Could not attach to Cadder daemon for runtime `{}`: {}.",
      runtime_dir.display(),
      format_error_chain(error)
    ),
    ConnectionStateView::Connected => "Attached to cadderd.".to_string(),
  }
}

fn connection_state_from_error(error: &anyhow::Error) -> ConnectionStateView {
  if error.chain().any(|cause| {
    cause.downcast_ref::<std::io::Error>().is_some_and(|error| {
      matches!(
        error.kind(),
        std::io::ErrorKind::NotFound
          | std::io::ErrorKind::ConnectionRefused
          | std::io::ErrorKind::ConnectionAborted
      )
    })
  }) {
    ConnectionStateView::NotRunning
  } else {
    ConnectionStateView::ConnectionFailed
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cli::{DomainSelectorArgs, EntrypointsCommand, LogReadArgs, TailArgs};
  use cadder_protocol::{
    ActivationState, ConfigState, DomainName, EntrypointInstanceIdentity, EntrypointRegistration,
    LogAttributionKind, LogEntry, LogEntryKind, LogSeverity, OwnerProcessIdentity,
    RegisteredDomain, RuntimeState, SourcePath, StateChangeKind,
  };
  use chrono::{TimeZone, Utc};
  use serde_json::Value;

  fn timestamp() -> chrono::DateTime<Utc> {
    Utc
      .with_ymd_and_hms(2026, 6, 16, 19, 0, 0)
      .single()
      .unwrap()
  }

  fn registration(id: &str, domains: &[&str]) -> EntrypointRegistration {
    let now = timestamp();
    let identity = EntrypointInstanceIdentity {
      instance_id: id.to_string(),
      started_at_utc: now,
      shim_session_nonce: format!("{id}-nonce"),
    };
    EntrypointRegistration {
      registration_id: id.to_string(),
      entrypoint_instance: identity.clone(),
      source_working_directory: SourcePath::new("D:/Projects/App", None),
      source_config_path: SourcePath::new("D:/Projects/App/Caddyfile", None),
      registered_domains: domains
        .iter()
        .map(|domain| RegisteredDomain {
          name: DomainName::parse(*domain),
          activation_state: ActivationState::Active,
          log_stream: LogStreamIdentity::domain(domain),
        })
        .collect(),
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 17,
        process_start_time_utc: now,
        shim_session_nonce: identity.shim_session_nonce.clone(),
        executable_path: Some("caddy.exe".to_string()),
      },
      log_stream: LogStreamIdentity::entrypoint(id),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
    }
  }

  fn snapshot(registrations: Vec<EntrypointRegistration>) -> GuiStateSnapshot {
    GuiStateSnapshot {
      captured_at_utc: timestamp(),
      registrations,
      runtime: RuntimeState::idle(),
      config: ConfigState::idle(),
    }
  }

  fn test_context(output: OutputMode) -> (tempfile::TempDir, AppContext) {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let context = AppContext {
      output,
      operator: OperatorContext::from_paths(paths, DaemonLaunchOptions::default()),
    };
    (temp, context)
  }

  fn sample_logs_view() -> LogsView {
    LogsView {
      stream: LogStreamIdentity::domain("app.localhost"),
      stream_status: cadder_protocol::LogStreamStatus::Active,
      entries: vec![LogEntry {
        sequence_number: 3,
        timestamp_utc: timestamp(),
        severity: LogSeverity::Warn,
        stream: LogStreamIdentity::domain("app.localhost"),
        attribution_kind: LogAttributionKind::Domain,
        entry_kind: LogEntryKind::Normal,
        raw_message: "tail event".to_string(),
        domain_key: Some("app.localhost".to_string()),
        source_registration_id: Some("shim-1".to_string()),
        source_instance_id: Some("shim-1-instance".to_string()),
        operation: None,
      }],
      next_cursor: Some("seq:3".to_string()),
      has_gap: false,
      has_more_before: false,
      truncated_by_retention: false,
    }
  }

  fn sample_event() -> StateChangedEvent {
    StateChangedEvent {
      request_id: "watch".to_string(),
      sequence_number: 7,
      change_kind: StateChangeKind::Snapshot,
      snapshot: snapshot(vec![registration("shim-1", &["app.localhost"])]),
      registration_id: Some("shim-1".to_string()),
    }
  }

  fn parse_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
  }

  #[test]
  fn validate_output_mode_rejects_invalid_combinations_and_ranges() {
    let watch = Command::Watch {
      command: WatchCommand::Status,
    };
    let err = validate_output_mode(OutputMode::Json, &watch, "watch status").unwrap_err();
    assert_eq!(err.kind, CliErrorKind::InvalidUsage);

    let domains = Command::Domains {
      command: DomainsCommand::List { registration: None },
    };
    let err = validate_output_mode(OutputMode::Jsonl, &domains, "domains list").unwrap_err();
    assert_eq!(err.kind, CliErrorKind::InvalidUsage);

    let logs_show = Command::Logs {
      command: LogsCommand::Show {
        target: LogsTargetCommand::Runtime,
        options: LogReadArgs {
          limit: 0,
          minimum_severity: None,
        },
      },
    };
    let err = validate_output_mode(OutputMode::Human, &logs_show, "logs show").unwrap_err();
    assert!(err.message.contains("between 1 and 500"));

    let logs_tail = Command::Logs {
      command: LogsCommand::Tail {
        target: LogsTargetCommand::Runtime,
        options: TailArgs {
          read: LogReadArgs {
            limit: 20,
            minimum_severity: None,
          },
          poll_interval_ms: 10,
        },
      },
    };
    let err = validate_output_mode(OutputMode::Jsonl, &logs_tail, "logs tail").unwrap_err();
    assert!(err.message.contains("poll-interval-ms"));

    let entrypoints = Command::Entrypoints {
      command: EntrypointsCommand::List,
    };
    validate_output_mode(OutputMode::Json, &entrypoints, "entrypoints list").unwrap();
  }

  #[test]
  fn select_domain_maps_not_found_and_ambiguity_into_cli_errors() {
    let missing = select_domain(
      "domains disable",
      &snapshot(vec![registration("shim-1", &["app.localhost"])]),
      &DomainSelectorArgs {
        domain: "missing.localhost".to_string(),
        registration: None,
      },
    )
    .unwrap_err();
    assert_eq!(missing.kind, CliErrorKind::TargetNotFound);
    assert!(missing.message.contains("missing.localhost"));

    let ambiguous = select_domain(
      "domains disable",
      &snapshot(vec![
        registration("shim-1", &["app.localhost"]),
        registration("shim-2", &["app.localhost"]),
      ]),
      &DomainSelectorArgs {
        domain: "app.localhost".to_string(),
        registration: None,
      },
    )
    .unwrap_err();
    assert_eq!(ambiguous.kind, CliErrorKind::ConflictOrRejected);
    assert!(ambiguous.message.contains("shim-1, shim-2"));
  }

  #[test]
  fn unavailable_message_distinguishes_not_running_from_connection_failure() {
    let not_running = anyhow::Error::from(std::io::Error::new(
      std::io::ErrorKind::ConnectionRefused,
      "connection refused",
    ));
    assert_eq!(
      connection_state_from_error(&not_running),
      ConnectionStateView::NotRunning
    );
    assert!(
      unavailable_message(std::path::Path::new("runtime"), &not_running).contains("is not running")
    );

    let connection_failed = anyhow::Error::msg("protocol mismatch");
    assert_eq!(
      connection_state_from_error(&connection_failed),
      ConnectionStateView::ConnectionFailed
    );
    assert!(
      unavailable_message(std::path::Path::new("runtime"), &connection_failed)
        .contains("Could not attach")
    );
  }

  #[test]
  fn write_helpers_emit_expected_human_and_machine_output() {
    let action = ActionResultView {
      message: "updated".to_string(),
    };

    let mut human = Vec::new();
    write_one_shot(
      OutputMode::Human,
      "domains enable",
      &action,
      &mut human,
      render::write_human_action,
    )
    .unwrap();
    assert_eq!(String::from_utf8(human).unwrap(), "updated\n");

    let mut json = Vec::new();
    write_one_shot(
      OutputMode::Json,
      "domains enable",
      &action,
      &mut json,
      render::write_human_action,
    )
    .unwrap();
    let json = parse_json(&json);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "domains enable");

    let logs = sample_logs_view();
    let mut stream = Vec::new();
    write_logs_stream_event(
      OutputMode::Human,
      "logsPage",
      "logs tail",
      &logs,
      &mut stream,
    )
    .unwrap();
    assert!(String::from_utf8(stream).unwrap().contains("tail event"));

    let mut stream = Vec::new();
    write_logs_stream_event(
      OutputMode::Jsonl,
      "logsPage",
      "logs tail",
      &logs,
      &mut stream,
    )
    .unwrap();
    let json = parse_json(&stream);
    assert_eq!(json["event"], "logsPage");
    assert_eq!(json["command"], "logs tail");

    let error = CliError::invalid_usage(
      "logs tail",
      "Streaming commands support `human` or `jsonl` output.",
      Some("Use `--output jsonl`.".to_string()),
    );
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    write_error(OutputMode::Human, &error, &mut stdout, &mut stderr).unwrap();
    assert!(String::from_utf8(stderr).unwrap().contains("Guidance:"));

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    write_error(OutputMode::Json, &error, &mut stdout, &mut stderr).unwrap();
    let json = parse_json(&stdout);
    assert_eq!(json["error"]["kind"], "invalidUsage");

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    write_error(OutputMode::Jsonl, &error, &mut stdout, &mut stderr).unwrap();
    let json = parse_json(&stdout);
    assert_eq!(json["event"], "error");
    assert_eq!(json["error"]["exitCode"], 2);
  }

  #[test]
  fn write_watch_event_supports_all_views_in_human_and_jsonl() {
    let event = sample_event();

    let (_temp, human_context) = test_context(OutputMode::Human);
    let expectations = [
      (WatchCommand::Status, "Runtime directory:"),
      (WatchCommand::Entrypoints, "Registration: shim-1"),
      (WatchCommand::Domains, "Domain: app.localhost"),
      (WatchCommand::Diagnostics, "No diagnostics."),
    ];
    for (command, expected) in expectations {
      let mut output = Vec::new();
      write_watch_event(&human_context, &command, &event, &mut output).unwrap();
      let text = String::from_utf8(output).unwrap();
      assert!(text.contains("Event: Snapshot (sequence 7)"));
      assert!(text.contains(expected), "missing `{expected}` in `{text}`");
    }

    let (_temp, jsonl_context) = test_context(OutputMode::Jsonl);
    let expectations = [
      (WatchCommand::Status, "watch status"),
      (WatchCommand::Entrypoints, "watch entrypoints"),
      (WatchCommand::Domains, "watch domains"),
      (WatchCommand::Diagnostics, "watch diagnostics"),
    ];
    for (command, label) in expectations {
      let mut output = Vec::new();
      write_watch_event(&jsonl_context, &command, &event, &mut output).unwrap();
      let json = parse_json(&output);
      assert_eq!(json["event"], "stateChanged");
      assert_eq!(json["command"], label);
      assert_eq!(json["sequenceNumber"], 7);
    }
  }
}
