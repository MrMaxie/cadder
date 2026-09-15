use std::{
  collections::{BTreeMap, BTreeSet},
  path::{Path, PathBuf},
};

use cadder_api::{
  AppExit, DaemonLaunchOptions, DomainSelector, LogsTarget, OperatorContext, OperatorError,
  diagnostics_view,
};
use cadder_ipc::{EntrypointRegistration, GuiStateSnapshot};

use super::{
  CaddyfileCommand, Command, DaemonCommand, DomainLogsArgs, DomainSelectorArgs, DomainsCommand,
  LogsCommand, PortCommand, ProjectLogsArgs, ProjectsCommand, output,
};
use crate::{
  inspection::{
    HostProcessInspector, PortOwner, ProcessInspector, ProcessSignalError, RegistrationIndex,
    RouteMatch, local_upstream,
  },
  run_tui,
};

pub(super) async fn execute(command: Command) -> Result<(), CommandFailure> {
  match command {
    Command::Tui => run_tui()
      .await
      .map_err(|error| CommandFailure::ipc(format!("Could not run the Cadder TUI: {error}"))),
    Command::Status => status().await,
    Command::Daemon { command } => daemon(command).await,
    Command::Projects { command } => projects(command).await,
    Command::Domains { command } => domains(command).await,
    Command::Port { command } => port(command).await,
    Command::Caddyfile { command } => caddyfile(command).await,
    Command::Diagnostics => diagnostics().await,
    Command::Logs { command } => logs(command).await,
  }
}

async fn status() -> Result<(), CommandFailure> {
  let context = context("status")?;
  let snapshot = context
    .query_snapshot("status", "query current state")
    .await?;
  output::status(&snapshot);
  Ok(())
}

async fn daemon(command: DaemonCommand) -> Result<(), CommandFailure> {
  let context = context("daemon")?;
  match command {
    DaemonCommand::Start => {
      context.ensure_daemon_running("daemon").await?;
      println!("cadderd is running.");
    }
    DaemonCommand::Stop => {
      context.stop_daemon("daemon").await?;
      println!("cadderd stop requested.");
    }
    DaemonCommand::Restart => {
      context.restart_daemon("daemon").await?;
      println!("cadderd restarted.");
    }
  }
  Ok(())
}

async fn projects(command: ProjectsCommand) -> Result<(), CommandFailure> {
  let context = context("projects")?;
  match command {
    ProjectsCommand::List => {
      let snapshot = context.query_snapshot("projects", "query projects").await?;
      output::projects(&snapshot);
    }
    ProjectsCommand::Enable(selector) => {
      set_project_enabled(&context, selector.caddyfile, true).await?;
    }
    ProjectsCommand::Disable(selector) => {
      set_project_enabled(&context, selector.caddyfile, false).await?;
    }
  }
  Ok(())
}

async fn set_project_enabled(
  context: &OperatorContext,
  caddyfile: PathBuf,
  enabled: bool,
) -> Result<(), CommandFailure> {
  let snapshot = context.query_snapshot("projects", "query projects").await?;
  let registration_id = unique_project(&snapshot, &caddyfile)?
    .registration_id
    .clone();
  let response = context
    .set_entrypoint_enabled("projects", registration_id, enabled)
    .await?;
  println!("{}", response.message);
  Ok(())
}

async fn domains(command: DomainsCommand) -> Result<(), CommandFailure> {
  let context = context("domains")?;
  match command {
    DomainsCommand::List => {
      let snapshot = context.query_snapshot("domains", "query domains").await?;
      output::domains(&snapshot);
    }
    DomainsCommand::Inspect(selector) => inspect_domain(&context, selector).await?,
    DomainsCommand::Enable(selector) => {
      set_domain_enabled(&context, selector, true).await?;
    }
    DomainsCommand::Disable(selector) => {
      set_domain_enabled(&context, selector, false).await?;
    }
  }
  Ok(())
}

async fn inspect_domain(
  context: &OperatorContext,
  selector: DomainSelectorArgs,
) -> Result<(), CommandFailure> {
  let snapshot = context.query_snapshot("domains", "query domains").await?;
  let routes = RegistrationIndex::new(&snapshot)
    .routes_for_domain(&selector.domain, selector.caddyfile.as_deref());
  if routes.is_empty() {
    return Err(CommandFailure::target_not_found(format!(
      "Domain `{}` was not found{}.",
      selector.domain,
      selector
        .caddyfile
        .as_ref()
        .map(|path| format!(" in `{}`", path.display()))
        .unwrap_or_default()
    )));
  }
  let owners = host_owners_for_routes(&routes).await;
  output::domain(
    &selector.domain,
    &snapshot,
    &routes,
    &owners.by_port,
    owners.error.as_deref(),
  );
  Ok(())
}

async fn set_domain_enabled(
  context: &OperatorContext,
  selector: DomainSelectorArgs,
  enabled: bool,
) -> Result<(), CommandFailure> {
  let registration = if let Some(caddyfile) = selector.caddyfile.as_deref() {
    let snapshot = context.query_snapshot("domains", "query projects").await?;
    Some(
      unique_project(&snapshot, caddyfile)?
        .registration_id
        .clone(),
    )
  } else {
    None
  };
  let response = context
    .set_domain_enabled(
      "domains",
      &DomainSelector {
        domain: selector.domain,
        registration,
      },
      enabled,
    )
    .await?;
  println!("{}", response.message);
  Ok(())
}

async fn port(command: PortCommand) -> Result<(), CommandFailure> {
  match command {
    PortCommand::Inspect { port } => inspect_port(port).await,
    PortCommand::Kill { port, pid } => {
      tokio::task::spawn_blocking(move || kill_port_owner(&HostProcessInspector, port, pid))
        .await
        .map_err(CommandFailure::join)?
    }
  }
}

async fn inspect_port(port: u16) -> Result<(), CommandFailure> {
  let owners = tokio::task::spawn_blocking(move || HostProcessInspector.owners(port)).await;
  let (owners, socket_note) = match owners {
    Ok(Ok(owners)) => (owners, None),
    Ok(Err(error)) => (Vec::new(), Some(error)),
    Err(error) => (
      Vec::new(),
      Some(format!("inspection worker failed: {error}")),
    ),
  };
  let context = context("port");
  match context {
    Ok(context) => match context.query_snapshot("port", "query routes").await {
      Ok(snapshot) => {
        let routes = RegistrationIndex::new(&snapshot).routes_for_port(port);
        output::port(port, &owners, socket_note.as_deref(), Some(&routes), None);
      }
      Err(error) => output::port(
        port,
        &owners,
        socket_note.as_deref(),
        None,
        Some(&error.message),
      ),
    },
    Err(error) => output::port(
      port,
      &owners,
      socket_note.as_deref(),
      None,
      Some(&error.message),
    ),
  }
  Ok(())
}

fn kill_port_owner(
  inspector: &impl ProcessInspector,
  port: u16,
  pid: u32,
) -> Result<(), CommandFailure> {
  if pid == std::process::id() {
    return Err(CommandFailure::conflict(
      "Cadder refuses to terminate its own operator process.".to_string(),
      None,
    ));
  }
  let owners = inspector.owners(port).map_err(CommandFailure::inspection)?;
  if !owners.iter().any(|owner| owner.pid == Some(pid)) {
    return Err(CommandFailure::conflict(
      format!(
        "PID {pid} no longer owns a listening socket on port {port}; no process was signaled."
      ),
      Some("Run `cadder port inspect <port>` again before retrying.".to_string()),
    ));
  }

  inspector.kill(pid).map_err(|error| match error {
    ProcessSignalError::NotFound => {
      CommandFailure::target_not_found(format!("Process {pid} no longer exists."))
    }
    ProcessSignalError::Rejected => CommandFailure::permission(format!(
      "The operating system rejected the termination signal for PID {pid}."
    )),
  })?;
  println!("Sent the termination signal to PID {pid} on port {port}.");
  Ok(())
}

async fn caddyfile(command: CaddyfileCommand) -> Result<(), CommandFailure> {
  let CaddyfileCommand::Inspect(selector) = command;
  let context = context("caddyfile")?;
  let snapshot = context
    .query_snapshot("caddyfile", "query projects")
    .await?;
  let projects = RegistrationIndex::new(&snapshot).projects_for_caddyfile(&selector.caddyfile);
  let routes = projects
    .iter()
    .flat_map(|project| {
      project.registered_domains.iter().map(|domain| RouteMatch {
        project,
        domain,
        upstream: domain.upstream.as_deref().unwrap_or("-"),
      })
    })
    .collect::<Vec<_>>();
  let owners = host_owners_for_routes(&routes).await;
  output::caddyfile(
    &selector.caddyfile,
    &snapshot,
    &projects,
    &owners.by_port,
    owners.error.as_deref(),
  );
  Ok(())
}

async fn diagnostics() -> Result<(), CommandFailure> {
  let context = context("diagnostics")?;
  let snapshot = context
    .query_snapshot("diagnostics", "query diagnostics")
    .await?;
  output::diagnostics(&diagnostics_view(&snapshot));
  Ok(())
}

async fn logs(command: LogsCommand) -> Result<(), CommandFailure> {
  let context = context("logs")?;
  let (target, limit) = match command {
    LogsCommand::Runtime(options) => (LogsTarget::Runtime, options.limit),
    LogsCommand::Project(ProjectLogsArgs { caddyfile, options }) => {
      let snapshot = context.query_snapshot("logs", "query projects").await?;
      let project = unique_project(&snapshot, &caddyfile)?;
      (
        LogsTarget::Entrypoint {
          registration_id: project.registration_id.clone(),
        },
        options.limit,
      )
    }
    LogsCommand::Domain(DomainLogsArgs { selector, options }) => {
      let registration = if let Some(caddyfile) = selector.caddyfile.as_deref() {
        let snapshot = context.query_snapshot("logs", "query projects").await?;
        Some(
          unique_project(&snapshot, caddyfile)?
            .registration_id
            .clone(),
        )
      } else {
        None
      };
      (
        LogsTarget::Domain(DomainSelector {
          domain: selector.domain,
          registration,
        }),
        options.limit,
      )
    }
  };
  let stream = context.resolve_logs_target("logs", target).await?;
  let logs = context
    .query_logs("logs", "query logs", stream, limit)
    .await?;
  output::logs(&logs);
  Ok(())
}

async fn host_owners_for_routes(routes: &[RouteMatch<'_>]) -> OwnerReport {
  let ports = routes
    .iter()
    .filter_map(|route| local_upstream(route.upstream).map(|upstream| upstream.port))
    .collect::<BTreeSet<_>>();
  if ports.is_empty() {
    return OwnerReport::default();
  }

  let requested_ports = ports.clone();
  let mut owners_by_port = requested_ports
    .into_iter()
    .map(|port| (port, Vec::new()))
    .collect::<BTreeMap<_, _>>();
  let owners =
    tokio::task::spawn_blocking(move || HostProcessInspector.owners_for_ports(&ports)).await;
  let owners = match owners {
    Ok(Ok(owners)) => owners,
    Ok(Err(error)) => {
      return OwnerReport {
        by_port: owners_by_port,
        error: Some(error),
      };
    }
    Err(error) => {
      return OwnerReport {
        by_port: owners_by_port,
        error: Some(format!("inspection worker failed: {error}")),
      };
    }
  };
  for owner in owners {
    owners_by_port
      .entry(owner.port)
      .or_insert_with(Vec::new)
      .push(owner);
  }
  OwnerReport {
    by_port: owners_by_port,
    error: None,
  }
}

#[derive(Default)]
struct OwnerReport {
  by_port: BTreeMap<u16, Vec<PortOwner>>,
  error: Option<String>,
}

fn unique_project<'a>(
  snapshot: &'a GuiStateSnapshot,
  caddyfile: &Path,
) -> Result<&'a EntrypointRegistration, CommandFailure> {
  let matches = RegistrationIndex::new(snapshot).projects_for_caddyfile(caddyfile);
  match matches.as_slice() {
    [] => Err(CommandFailure::target_not_found(format!(
      "Caddyfile `{}` is not registered.",
      caddyfile.display()
    ))),
    [project] => Ok(project),
    _ => Err(CommandFailure::conflict(
      format!(
        "Caddyfile `{}` is registered by multiple running projects; stop the duplicate `caddy run` process before changing activation.",
        caddyfile.display()
      ),
      None,
    )),
  }
}

fn context(command: &'static str) -> Result<OperatorContext, CommandFailure> {
  OperatorContext::new(command, None, DaemonLaunchOptions::default()).map_err(Into::into)
}

#[derive(Debug)]
pub(super) struct CommandFailure {
  kind: AppExit,
  message: String,
  guidance: Option<String>,
}

impl CommandFailure {
  fn inspection(message: String) -> Self {
    Self {
      kind: AppExit::IpcFailure,
      message: format!("Could not inspect local sockets: {message}."),
      guidance: None,
    }
  }

  fn ipc(message: String) -> Self {
    Self {
      kind: AppExit::IpcFailure,
      message,
      guidance: None,
    }
  }

  fn join(error: tokio::task::JoinError) -> Self {
    Self::ipc(format!("The local inspection worker failed: {error}"))
  }

  fn target_not_found(message: String) -> Self {
    Self {
      kind: AppExit::TargetNotFound,
      message,
      guidance: None,
    }
  }

  fn conflict(message: String, guidance: Option<String>) -> Self {
    Self {
      kind: AppExit::ConflictOrRejected,
      message,
      guidance,
    }
  }

  fn permission(message: String) -> Self {
    Self {
      kind: AppExit::PermissionOrElevation,
      message,
      guidance: Some(
        "Use an account allowed to terminate that process, then inspect the port again."
          .to_string(),
      ),
    }
  }

  pub(super) fn report(self) -> AppExit {
    eprintln!("{}", self.message);
    if let Some(guidance) = self.guidance {
      eprintln!("{guidance}");
    }
    self.kind
  }
}

impl From<OperatorError> for CommandFailure {
  fn from(error: OperatorError) -> Self {
    Self {
      kind: error.kind,
      message: error.message,
      guidance: error.guidance,
    }
  }
}

#[cfg(test)]
mod tests {
  use std::{cell::RefCell, net::Ipv4Addr};

  use super::*;
  use crate::inspection::SocketProtocol;

  struct FakeInspector {
    owners: Vec<PortOwner>,
    killed: RefCell<Vec<u32>>,
  }

  impl ProcessInspector for FakeInspector {
    fn owners_for_ports(&self, ports: &BTreeSet<u16>) -> Result<Vec<PortOwner>, String> {
      Ok(
        self
          .owners
          .iter()
          .filter(|owner| ports.contains(&owner.port))
          .cloned()
          .collect(),
      )
    }

    fn kill(&self, pid: u32) -> Result<(), ProcessSignalError> {
      self.killed.borrow_mut().push(pid);
      Ok(())
    }
  }

  fn owner(port: u16, pid: Option<u32>) -> PortOwner {
    PortOwner {
      protocol: SocketProtocol::Tcp,
      local_address: Ipv4Addr::LOCALHOST.into(),
      port,
      pid,
      process_name: None,
      executable_path: None,
    }
  }

  #[test]
  fn kill_refuses_when_expected_pid_no_longer_owns_port() {
    let inspector = FakeInspector {
      owners: vec![owner(3000, Some(20))],
      killed: RefCell::new(Vec::new()),
    };

    let error = kill_port_owner(&inspector, 3000, 10).expect_err("owner changed");

    assert_eq!(error.kind, AppExit::ConflictOrRejected);
    assert!(inspector.killed.borrow().is_empty());
  }

  #[test]
  fn kill_signals_only_the_expected_owner_when_port_has_multiple_owners() {
    let inspector = FakeInspector {
      owners: vec![
        owner(3000, None),
        owner(3000, Some(10)),
        owner(3000, Some(20)),
      ],
      killed: RefCell::new(Vec::new()),
    };

    kill_port_owner(&inspector, 3000, 20).expect("expected owner should be signaled");

    assert_eq!(*inspector.killed.borrow(), [20]);
  }
}
