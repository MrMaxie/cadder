use crate::inspection::{PortOwner, RouteMatch, caddyfile_path, display_socket, display_upstream};
use cadder_ipc::{ActivationState, EntrypointRegistration, GuiStateSnapshot};
use comfy_table::{Cell, Table, presets::NOTHING};
use std::{collections::BTreeMap, path::Path};

use cadder_api::{DiagnosticsView, LogsView};

pub(crate) fn status(snapshot: &GuiStateSnapshot) {
  println!("cadderd  running");
  println!("Caddy    {}", debug_label(snapshot.runtime.status));
  println!("Config   {}", debug_label(snapshot.config.status));
}

pub(crate) fn projects(snapshot: &GuiStateSnapshot) {
  if snapshot.registrations.is_empty() {
    println!("No projects registered.");
    return;
  }

  let mut table = table(["STATE", "PROJECT", "CADDYFILE"]);
  let mut projects = snapshot.registrations.iter().collect::<Vec<_>>();
  projects.sort_by_key(|project| &project.source_working_directory.raw);
  for project in projects {
    table.add_row([
      activation_label(project.activation_state).to_string(),
      project.source_working_directory.raw.clone(),
      caddyfile_path(project).display().to_string(),
    ]);
  }
  println!("{table}");
}

pub(crate) fn domains(snapshot: &GuiStateSnapshot) {
  let mut table = table(["STATE", "DOMAIN", "UPSTREAM", "PROJECT"]);
  let mut domains = snapshot
    .registrations
    .iter()
    .flat_map(|project| {
      project
        .registered_domains
        .iter()
        .map(move |domain| (project, domain))
    })
    .collect::<Vec<_>>();
  domains.sort_by(
    |(left_project, left_domain), (right_project, right_domain)| {
      (
        &left_project.source_working_directory.raw,
        &left_domain.name.canonical,
      )
        .cmp(&(
          &right_project.source_working_directory.raw,
          &right_domain.name.canonical,
        ))
    },
  );

  if domains.is_empty() {
    println!("No domains registered.");
    return;
  }

  for (project, domain) in domains {
    table.add_row([
      effective_state(project.activation_state, domain.activation_state).to_string(),
      domain.name.raw.clone(),
      domain
        .upstream
        .as_deref()
        .map(display_upstream)
        .unwrap_or_else(|| "-".to_string()),
      project.source_working_directory.raw.clone(),
    ]);
  }
  println!("{table}");
}

pub(crate) fn port(
  port: u16,
  owners: &[PortOwner],
  socket_note: Option<&str>,
  routes: Option<&[RouteMatch<'_>]>,
  cadder_note: Option<&str>,
) {
  println!("Port {port}");
  println!();
  println!("Sockets");
  print_socket_report(owners, socket_note);

  println!();
  println!("Cadder routes");
  if let Some(routes) = routes {
    print_routes(routes);
  } else {
    println!("Registration data unavailable.");
  }

  if let Some(note) = cadder_note {
    println!();
    println!("Cadder: {note}");
  }
}

pub(crate) fn caddyfile(
  requested: &Path,
  snapshot: &GuiStateSnapshot,
  projects: &[&EntrypointRegistration],
  owners_by_port: &BTreeMap<u16, Vec<PortOwner>>,
  socket_note: Option<&str>,
) {
  println!("Caddyfile {}", requested.display());
  println!("cadderd  running");
  println!("Caddy    {}", debug_label(snapshot.runtime.status));
  println!("Config   {}", debug_label(snapshot.config.status));

  if projects.is_empty() {
    println!("Registered no");
    return;
  }

  println!("Registered yes");
  println!();
  let mut project_table = table(["PROJECT STATE", "PROJECT", "CADDYFILE"]);
  for project in projects {
    project_table.add_row([
      activation_label(project.activation_state).to_string(),
      project.source_working_directory.raw.clone(),
      caddyfile_path(project).display().to_string(),
    ]);
  }
  println!("{project_table}");

  println!();
  println!("Domains");
  let mut domain_table = table(["PROJECT STATE", "DOMAIN STATE", "DOMAIN", "UPSTREAM"]);
  for project in projects {
    for domain in &project.registered_domains {
      domain_table.add_row([
        activation_label(project.activation_state).to_string(),
        activation_label(domain.activation_state).to_string(),
        domain.name.raw.clone(),
        domain
          .upstream
          .as_deref()
          .map(display_upstream)
          .unwrap_or_else(|| "-".to_string()),
      ]);
    }
  }
  println!("{domain_table}");

  if !owners_by_port.is_empty() {
    println!();
    println!("Local upstream sockets");
    let owners = owners_by_port
      .values()
      .flat_map(|owners| owners.iter())
      .cloned()
      .collect::<Vec<_>>();
    print_socket_report(&owners, socket_note);
  } else if let Some(note) = socket_note {
    println!();
    println!("Local upstream sockets");
    print_socket_report(&[], Some(note));
  }
}

pub(crate) fn domain(
  requested: &str,
  snapshot: &GuiStateSnapshot,
  routes: &[RouteMatch<'_>],
  owners_by_port: &BTreeMap<u16, Vec<PortOwner>>,
  socket_note: Option<&str>,
) {
  println!("Domain {requested}");
  println!("cadderd  running");
  println!("Caddy    {}", debug_label(snapshot.runtime.status));
  println!("Config   {}", debug_label(snapshot.config.status));
  println!();
  print_routes(routes);

  if !owners_by_port.is_empty() {
    println!();
    println!("Local upstream sockets");
    let owners = owners_by_port
      .values()
      .flat_map(|owners| owners.iter())
      .cloned()
      .collect::<Vec<_>>();
    print_socket_report(&owners, socket_note);
  } else if let Some(note) = socket_note {
    println!();
    println!("Local upstream sockets");
    print_socket_report(&[], Some(note));
  }
}

pub(crate) fn diagnostics(diagnostics: &DiagnosticsView) {
  println!(
    "Caddy    {}",
    diagnostics.runtime.status.to_ascii_lowercase()
  );
  println!(
    "Config   {}",
    diagnostics.config.status.to_ascii_lowercase()
  );
  if let Some(pid) = diagnostics.runtime.process_id {
    println!("PID      {pid}");
  }
  if let Some(version) = diagnostics.runtime.version.as_deref() {
    println!("Version  {version}");
  }
  if let Some(binary) = diagnostics.runtime.binary_path.as_deref() {
    println!("Binary   {binary}");
  }
  if let Some(endpoint) = diagnostics.runtime.admin_endpoint.as_deref() {
    println!("Admin    {endpoint}");
  }

  println!();
  println!("Runtime diagnostics");
  if diagnostics.runtime_diagnostics.is_empty() {
    println!("None.");
  } else {
    let mut table = table(["CODE", "OPERATION", "MESSAGE"]);
    for diagnostic in &diagnostics.runtime_diagnostics {
      table.add_row([
        diagnostic.code.clone(),
        diagnostic
          .operation
          .clone()
          .unwrap_or_else(|| "-".to_string()),
        diagnostic.message.clone(),
      ]);
    }
    println!("{table}");
  }

  println!();
  println!("Configuration diagnostics");
  if diagnostics.config_diagnostics.is_empty() {
    println!("None.");
  } else {
    let mut table = table(["CODE", "DOMAIN", "CADDYFILES", "MESSAGE"]);
    for diagnostic in &diagnostics.config_diagnostics {
      table.add_row([
        diagnostic.code.clone(),
        diagnostic
          .domain_key
          .clone()
          .unwrap_or_else(|| "-".to_string()),
        if diagnostic.source_config_paths.is_empty() {
          "-".to_string()
        } else {
          diagnostic.source_config_paths.join(", ")
        },
        diagnostic.message.clone(),
      ]);
    }
    println!("{table}");
  }
}

pub(crate) fn logs(logs: &LogsView) {
  println!("Status   {}", debug_label(logs.stream_status));
  println!("Channel  {}", logs.stream.channel);
  if let Some(domain) = logs.stream.domain_key.as_deref() {
    println!("Domain   {domain}");
  }

  if logs.entries.is_empty() {
    println!();
    println!("No log entries.");
    return;
  }

  println!();
  let mut table = table(["SEQ", "TIME (UTC)", "LEVEL", "MESSAGE"]);
  for entry in &logs.entries {
    table.add_row([
      entry.sequence_number.to_string(),
      entry.timestamp_utc.to_rfc3339(),
      debug_label(entry.severity),
      entry.raw_message.clone(),
    ]);
  }
  println!("{table}");
}

fn print_socket_report(owners: &[PortOwner], note: Option<&str>) {
  if let Some(note) = note {
    println!("Unavailable: {note}");
  } else {
    print_owners(owners);
  }
}

fn print_routes(routes: &[RouteMatch<'_>]) {
  if routes.is_empty() {
    println!("No matching routes.");
    return;
  }

  let mut table = table([
    "PROJECT STATE",
    "DOMAIN STATE",
    "DOMAIN",
    "UPSTREAM",
    "PROJECT",
    "CADDYFILE",
  ]);
  for route in routes {
    table.add_row([
      activation_label(route.project.activation_state).to_string(),
      activation_label(route.domain.activation_state).to_string(),
      route.domain.name.raw.clone(),
      display_upstream(route.upstream),
      route.project.source_working_directory.raw.clone(),
      caddyfile_path(route.project).display().to_string(),
    ]);
  }
  println!("{table}");
}

fn print_owners(owners: &[PortOwner]) {
  if owners.is_empty() {
    println!("No listening socket found.");
    return;
  }

  let mut table = table(["PROTOCOL", "ADDRESS", "PID", "PROCESS", "EXECUTABLE"]);
  for owner in owners {
    table.add_row([
      owner.protocol.label().to_string(),
      display_socket(owner),
      owner
        .pid
        .map(|pid| pid.to_string())
        .unwrap_or_else(|| "unknown".to_string()),
      owner
        .process_name
        .clone()
        .unwrap_or_else(|| "unknown".to_string()),
      owner
        .executable_path
        .clone()
        .unwrap_or_else(|| "-".to_string()),
    ]);
  }
  println!("{table}");
}

fn table<const N: usize>(header: [&str; N]) -> Table {
  let mut table = Table::new();
  table.load_style(NOTHING);
  table.set_header(header.map(Cell::new));
  table
}

fn activation_label(state: ActivationState) -> &'static str {
  match state {
    ActivationState::Unknown => "unknown",
    ActivationState::Registered => "registered",
    ActivationState::Activating => "activating",
    ActivationState::Active => "active",
    ActivationState::Inactive => "inactive",
    ActivationState::Faulted => "faulted",
  }
}

fn effective_state(project: ActivationState, domain: ActivationState) -> &'static str {
  match (project, domain) {
    (ActivationState::Faulted, _) | (_, ActivationState::Faulted) => "faulted",
    (ActivationState::Inactive, _) | (_, ActivationState::Inactive) => "inactive",
    (ActivationState::Active, ActivationState::Active) => "active",
    _ => "pending",
  }
}

fn debug_label(value: impl std::fmt::Debug) -> String {
  format!("{value:?}").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::inspection::{RegistrationIndex, SocketProtocol};
  use cadder_api::{LogsView, diagnostics_view};
  use cadder_ipc::{
    ConfigDiagnostic, ConfigState, DomainName, EntrypointInstanceIdentity, LogAttributionKind,
    LogEntry, LogEntryKind, LogSeverity, LogStreamIdentity, LogStreamStatus, OwnerProcessIdentity,
    RegisteredDomain, RuntimeDiagnostic, RuntimeState, SourcePath,
  };
  use chrono::Utc;
  use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

  fn registration(
    id: &str,
    project: &str,
    state: ActivationState,
    domains: &[(&str, ActivationState, Option<&str>)],
  ) -> EntrypointRegistration {
    let now = Utc::now();
    let nonce = format!("{id}-nonce");
    EntrypointRegistration {
      registration_id: id.to_string(),
      entrypoint_instance: EntrypointInstanceIdentity {
        instance_id: id.to_string(),
        started_at_utc: now,
        shim_session_nonce: nonce.clone(),
      },
      source_working_directory: SourcePath::new(project, None),
      source_config_path: SourcePath::new("Caddyfile", None),
      registered_domains: domains
        .iter()
        .map(|(name, state, upstream)| RegisteredDomain {
          name: DomainName::parse(*name),
          activation_state: *state,
          upstream: upstream.map(ToString::to_string),
          log_stream: LogStreamIdentity::domain(name),
        })
        .collect(),
      activation_state: state,
      owner_process: OwnerProcessIdentity {
        process_id: 42,
        process_start_time_utc: now,
        shim_session_nonce: nonce,
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint(id),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
    }
  }

  fn populated_snapshot() -> GuiStateSnapshot {
    let now = Utc::now();
    GuiStateSnapshot {
      captured_at_utc: now,
      registrations: vec![
        registration(
          "entry-b",
          "/workspace/project-b",
          ActivationState::Active,
          &[
            (
              "active.localhost",
              ActivationState::Active,
              Some("127.0.0.1:3000"),
            ),
            (
              "inactive.localhost",
              ActivationState::Inactive,
              Some("localhost:4000"),
            ),
            ("pending.localhost", ActivationState::Registered, None),
          ],
        ),
        registration(
          "entry-a",
          "/workspace/project-a",
          ActivationState::Faulted,
          &[(
            "faulted.localhost",
            ActivationState::Active,
            Some("api.example.com:443"),
          )],
        ),
      ],
      runtime: RuntimeState {
        status: cadder_ipc::RuntimeStatus::Running,
        binary_path: Some("/usr/bin/caddy".to_string()),
        version: Some("v2.11.3".to_string()),
        process_id: Some(42),
        admin_endpoint: Some("localhost:2019".to_string()),
        diagnostics: vec![RuntimeDiagnostic {
          code: "runtime-warning".to_string(),
          message: "runtime message".to_string(),
          operation: Some("reload".to_string()),
        }],
      },
      config: ConfigState {
        diagnostics: vec![ConfigDiagnostic {
          code: "config-warning".to_string(),
          message: "config message".to_string(),
          domain_key: Some("active.localhost".to_string()),
          source_config_paths: vec!["/workspace/project-b/Caddyfile".to_string()],
        }],
        ..ConfigState::idle()
      },
      storage: None,
    }
  }

  fn owner(protocol: SocketProtocol, address: IpAddr, port: u16, pid: Option<u32>) -> PortOwner {
    PortOwner {
      protocol,
      local_address: address,
      port,
      pid,
      process_name: pid.map(|_| "server".to_string()),
      executable_path: pid.map(|_| "/usr/bin/server".to_string()),
    }
  }

  #[test]
  fn list_and_status_outputs_cover_empty_and_populated_snapshots() {
    let empty = GuiStateSnapshot {
      captured_at_utc: Utc::now(),
      registrations: Vec::new(),
      runtime: RuntimeState::idle(),
      config: ConfigState::idle(),
      storage: None,
    };
    status(&empty);
    projects(&empty);
    domains(&empty);

    let snapshot = populated_snapshot();
    status(&snapshot);
    projects(&snapshot);
    domains(&snapshot);
  }

  #[test]
  fn port_output_covers_socket_and_registration_availability() {
    let owners = vec![
      owner(
        SocketProtocol::Tcp,
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        3000,
        Some(42),
      ),
      owner(
        SocketProtocol::Udp,
        IpAddr::V6(Ipv6Addr::LOCALHOST),
        3000,
        None,
      ),
      owner(
        SocketProtocol::Tcp,
        "192.0.2.1".parse().unwrap(),
        3000,
        Some(84),
      ),
      owner(
        SocketProtocol::Tcp,
        "2001:db8::1".parse().unwrap(),
        3000,
        Some(126),
      ),
    ];
    let snapshot = populated_snapshot();
    let routes = RegistrationIndex::new(&snapshot).routes_for_port(3000);

    port(3000, &owners, None, Some(&routes), None);
    port(
      3000,
      &[],
      Some("socket inspection unavailable"),
      None,
      Some("daemon unavailable"),
    );
    port(3000, &[], None, Some(&[]), None);
  }

  #[test]
  fn route_outputs_cover_registered_unregistered_and_socket_notes() {
    let snapshot = populated_snapshot();
    let project = &snapshot.registrations[0];
    let requested = caddyfile_path(project);
    let projects = [&snapshot.registrations[0]];
    let routes = RegistrationIndex::new(&snapshot).routes_for_domain("active.localhost", None);
    let owners = BTreeMap::from([(
      3000,
      vec![owner(
        SocketProtocol::Tcp,
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        3000,
        Some(42),
      )],
    )]);

    caddyfile(&requested, &snapshot, &projects, &owners, None);
    caddyfile(
      Path::new("missing/Caddyfile"),
      &snapshot,
      &[],
      &BTreeMap::new(),
      None,
    );
    caddyfile(
      &requested,
      &snapshot,
      &projects,
      &BTreeMap::new(),
      Some("socket inspection unavailable"),
    );
    domain("active.localhost", &snapshot, &routes, &owners, None);
    domain(
      "active.localhost",
      &snapshot,
      &routes,
      &BTreeMap::new(),
      Some("socket inspection unavailable"),
    );
  }

  #[test]
  fn diagnostics_and_logs_cover_empty_and_detailed_reports() {
    let empty = GuiStateSnapshot {
      captured_at_utc: Utc::now(),
      registrations: Vec::new(),
      runtime: RuntimeState::idle(),
      config: ConfigState::idle(),
      storage: None,
    };
    diagnostics(&diagnostics_view(&empty));
    diagnostics(&diagnostics_view(&populated_snapshot()));

    let stream = LogStreamIdentity::domain("active.localhost");
    logs(&LogsView {
      stream: stream.clone(),
      stream_status: LogStreamStatus::Empty,
      entries: Vec::new(),
    });
    logs(&LogsView {
      stream: stream.clone(),
      stream_status: LogStreamStatus::Active,
      entries: vec![LogEntry {
        sequence_number: 7,
        timestamp_utc: Utc::now(),
        severity: LogSeverity::Warn,
        stream,
        attribution_kind: LogAttributionKind::Domain,
        entry_kind: LogEntryKind::Normal,
        raw_message: "redacted log message".to_string(),
        domain_key: Some("active.localhost".to_string()),
        source_registration_id: Some("entry-b".to_string()),
        source_instance_id: Some("entry-b".to_string()),
        operation: None,
      }],
    });
  }

  #[test]
  fn state_labels_cover_every_activation_state() {
    assert_eq!(activation_label(ActivationState::Unknown), "unknown");
    assert_eq!(activation_label(ActivationState::Registered), "registered");
    assert_eq!(activation_label(ActivationState::Activating), "activating");
    assert_eq!(activation_label(ActivationState::Active), "active");
    assert_eq!(activation_label(ActivationState::Inactive), "inactive");
    assert_eq!(activation_label(ActivationState::Faulted), "faulted");
    assert_eq!(
      effective_state(ActivationState::Active, ActivationState::Active),
      "active"
    );
    assert_eq!(
      effective_state(ActivationState::Inactive, ActivationState::Active),
      "inactive"
    );
    assert_eq!(
      effective_state(ActivationState::Active, ActivationState::Faulted),
      "faulted"
    );
    assert_eq!(
      effective_state(ActivationState::Registered, ActivationState::Registered),
      "pending"
    );
  }
}
