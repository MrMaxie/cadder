use cadder_ipc::{EntrypointRegistration, GuiStateSnapshot, RegisteredDomain, canonicalize_domain};
use netstat2::{AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState, get_sockets_info};
use std::{
  collections::BTreeSet,
  net::IpAddr,
  path::{Path, PathBuf},
};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System, UpdateKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SocketProtocol {
  Tcp,
  Udp,
}

impl SocketProtocol {
  pub(crate) const fn label(self) -> &'static str {
    match self {
      Self::Tcp => "tcp",
      Self::Udp => "udp",
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PortOwner {
  pub(crate) protocol: SocketProtocol,
  pub(crate) local_address: IpAddr,
  pub(crate) port: u16,
  pub(crate) pid: Option<u32>,
  pub(crate) process_name: Option<String>,
  pub(crate) executable_path: Option<String>,
}

pub(crate) trait ProcessInspector {
  fn owners_for_ports(&self, ports: &BTreeSet<u16>) -> Result<Vec<PortOwner>, String>;

  fn owners(&self, port: u16) -> Result<Vec<PortOwner>, String> {
    self.owners_for_ports(&BTreeSet::from([port]))
  }

  fn kill(&self, pid: u32) -> Result<(), ProcessSignalError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessSignalError {
  NotFound,
  Rejected,
}

pub(crate) struct HostProcessInspector;

impl ProcessInspector for HostProcessInspector {
  fn owners_for_ports(&self, ports: &BTreeSet<u16>) -> Result<Vec<PortOwner>, String> {
    let sockets = get_sockets_info(
      AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
      ProtocolFlags::TCP | ProtocolFlags::UDP,
    )
    .map_err(|error| error.to_string())?;
    let system = System::new_with_specifics(
      RefreshKind::nothing()
        .with_processes(ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet)),
    );
    let mut owners = Vec::new();

    for socket in sockets {
      let (protocol, local_address, port) = match socket.protocol_socket_info {
        ProtocolSocketInfo::Tcp(tcp) if tcp.state == TcpState::Listen => {
          (SocketProtocol::Tcp, tcp.local_addr, tcp.local_port)
        }
        ProtocolSocketInfo::Udp(udp) => (SocketProtocol::Udp, udp.local_addr, udp.local_port),
        ProtocolSocketInfo::Tcp(_) => continue,
      };
      if !ports.contains(&port) {
        continue;
      }

      if socket.associated_pids.is_empty() {
        owners.push(PortOwner {
          protocol,
          local_address,
          port,
          pid: None,
          process_name: None,
          executable_path: None,
        });
        continue;
      }

      for pid in socket.associated_pids {
        let process = system.process(Pid::from_u32(pid));
        owners.push(PortOwner {
          protocol,
          local_address,
          port,
          pid: Some(pid),
          process_name: process.map(|process| process.name().to_string_lossy().into_owned()),
          executable_path: process
            .and_then(|process| process.exe())
            .map(|path| path.display().to_string()),
        });
      }
    }

    owners.sort_by(|left, right| {
      (
        left.protocol,
        left.local_address,
        left.pid,
        &left.process_name,
      )
        .cmp(&(
          right.protocol,
          right.local_address,
          right.pid,
          &right.process_name,
        ))
    });
    owners.dedup_by(|left, right| {
      left.protocol == right.protocol
        && left.local_address == right.local_address
        && left.port == right.port
        && left.pid == right.pid
    });
    Ok(owners)
  }

  fn kill(&self, pid: u32) -> Result<(), ProcessSignalError> {
    let system = System::new_with_specifics(
      RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );
    let process = system
      .process(Pid::from_u32(pid))
      .ok_or(ProcessSignalError::NotFound)?;
    process
      .kill()
      .then_some(())
      .ok_or(ProcessSignalError::Rejected)
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalUpstream {
  pub(crate) port: u16,
}

pub(crate) struct RouteMatch<'a> {
  pub(crate) project: &'a EntrypointRegistration,
  pub(crate) domain: &'a RegisteredDomain,
  pub(crate) upstream: &'a str,
}

pub(crate) struct RegistrationIndex<'a> {
  snapshot: &'a GuiStateSnapshot,
}

impl<'a> RegistrationIndex<'a> {
  pub(crate) const fn new(snapshot: &'a GuiStateSnapshot) -> Self {
    Self { snapshot }
  }

  pub(crate) fn routes_for_port(&self, port: u16) -> Vec<RouteMatch<'a>> {
    let mut routes = self
      .snapshot
      .registrations
      .iter()
      .flat_map(|project| {
        project.registered_domains.iter().filter_map(move |domain| {
          let upstream = domain.upstream.as_deref()?;
          (local_upstream(upstream).is_some_and(|candidate| candidate.port == port)).then_some(
            RouteMatch {
              project,
              domain,
              upstream,
            },
          )
        })
      })
      .collect::<Vec<_>>();
    sort_routes(&mut routes);
    routes
  }

  pub(crate) fn projects_for_caddyfile(&self, caddyfile: &Path) -> Vec<&'a EntrypointRegistration> {
    let requested = comparable_path(caddyfile, None);
    let mut projects = self
      .snapshot
      .registrations
      .iter()
      .filter(|project| project_matches_path(project, &requested))
      .collect::<Vec<_>>();
    projects.sort_by_key(|project| &project.source_working_directory.raw);
    projects
  }

  pub(crate) fn routes_for_domain(
    &self,
    domain: &str,
    caddyfile: Option<&Path>,
  ) -> Vec<RouteMatch<'a>> {
    let canonical_domain = canonicalize_domain(domain);
    let requested_caddyfile = caddyfile.map(|path| comparable_path(path, None));
    let mut routes = self
      .snapshot
      .registrations
      .iter()
      .filter(|project| {
        requested_caddyfile
          .as_deref()
          .is_none_or(|requested| project_matches_path(project, requested))
      })
      .flat_map(|project| {
        let canonical_domain = canonical_domain.clone();
        project.registered_domains.iter().filter_map(move |domain| {
          (domain.name.canonical == canonical_domain).then_some(RouteMatch {
            project,
            domain,
            upstream: domain.upstream.as_deref().unwrap_or("-"),
          })
        })
      })
      .collect::<Vec<_>>();
    sort_routes(&mut routes);
    routes
  }
}

pub(crate) fn local_upstream(upstream: &str) -> Option<LocalUpstream> {
  let authority = upstream
    .trim()
    .strip_prefix("http://")
    .or_else(|| upstream.trim().strip_prefix("https://"))
    .unwrap_or(upstream.trim())
    .split('/')
    .next()?;

  if let Ok(port) = authority.parse::<u16>() {
    return Some(LocalUpstream { port });
  }

  let (host, port) = authority.rsplit_once(':')?;
  let port = port.parse::<u16>().ok()?;
  let host = host
    .strip_prefix('[')
    .and_then(|value| value.strip_suffix(']'))
    .unwrap_or(host);
  let is_local = host.is_empty()
    || host.eq_ignore_ascii_case("localhost")
    || host
      .parse::<IpAddr>()
      .is_ok_and(|address| address.is_loopback() || address.is_unspecified());
  is_local.then_some(LocalUpstream { port })
}

pub(crate) fn display_upstream(upstream: &str) -> String {
  local_upstream(upstream)
    .map(|local| format!(":{}", local.port))
    .unwrap_or_else(|| upstream.to_string())
}

pub(crate) fn display_socket(owner: &PortOwner) -> String {
  if owner.local_address.is_loopback() || owner.local_address.is_unspecified() {
    format!(":{}", owner.port)
  } else if owner.local_address.is_ipv6() {
    format!("[{}]:{}", owner.local_address, owner.port)
  } else {
    format!("{}:{}", owner.local_address, owner.port)
  }
}

pub(crate) fn caddyfile_path(project: &EntrypointRegistration) -> PathBuf {
  let source = PathBuf::from(&project.source_config_path.raw);
  if source.is_absolute() {
    source
  } else {
    Path::new(&project.source_working_directory.raw).join(source)
  }
}

fn project_matches_path(project: &EntrypointRegistration, requested: &str) -> bool {
  let working_directory = Path::new(&project.source_working_directory.raw);
  comparable_path(
    Path::new(&project.source_config_path.raw),
    Some(working_directory),
  ) == requested
    || project
      .source_config_path
      .canonical
      .as_deref()
      .is_some_and(|canonical| comparable_path(Path::new(canonical), None) == requested)
}

fn sort_routes(routes: &mut [RouteMatch<'_>]) {
  routes.sort_by(|left, right| {
    (
      &left.project.source_working_directory.raw,
      &left.domain.name.canonical,
    )
      .cmp(&(
        &right.project.source_working_directory.raw,
        &right.domain.name.canonical,
      ))
  });
}

fn comparable_path(path: &Path, base: Option<&Path>) -> String {
  let path = if path.is_absolute() {
    path.to_path_buf()
  } else if let Some(base) = base {
    base.join(path)
  } else {
    std::path::absolute(path).unwrap_or_else(|_| PathBuf::from(path))
  };
  let path = path.canonicalize().unwrap_or(path);
  let value = path.to_string_lossy().into_owned();
  if cfg!(windows) {
    value.replace('/', "\\").to_ascii_lowercase()
  } else {
    value
  }
}

#[cfg(test)]
mod tests {
  use cadder_ipc::{
    ActivationState, ConfigState, DomainName, EntrypointInstanceIdentity, LogStreamIdentity,
    OwnerProcessIdentity, RuntimeState, SourcePath,
  };
  use chrono::Utc;

  use super::*;

  fn snapshot(caddyfile: &Path) -> GuiStateSnapshot {
    let now = Utc::now();
    let registration_id = "entry-1".to_string();
    let nonce = "nonce-1".to_string();
    let domains = [
      ("local.example", "localhost:3000"),
      ("remote.example", "api.example.com:3000"),
    ]
    .into_iter()
    .map(|(domain, upstream)| RegisteredDomain {
      name: DomainName::parse(domain),
      activation_state: ActivationState::Active,
      upstream: Some(upstream.to_string()),
      log_stream: LogStreamIdentity::domain(domain),
    })
    .collect();
    let project = EntrypointRegistration {
      registration_id: registration_id.clone(),
      entrypoint_instance: EntrypointInstanceIdentity {
        instance_id: registration_id.clone(),
        started_at_utc: now,
        shim_session_nonce: nonce.clone(),
      },
      source_working_directory: SourcePath::new(
        caddyfile.parent().unwrap().display().to_string(),
        None,
      ),
      source_config_path: SourcePath::new(caddyfile.display().to_string(), None),
      registered_domains: domains,
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 42,
        process_start_time_utc: now,
        shim_session_nonce: nonce,
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint(&registration_id),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
    };
    GuiStateSnapshot {
      captured_at_utc: now,
      registrations: vec![project],
      runtime: RuntimeState::idle(),
      config: ConfigState::idle(),
      storage: None,
    }
  }

  #[test]
  fn recognizes_local_upstream_aliases() {
    for upstream in [
      "localhost:3000",
      "127.0.0.1:3000",
      "[::1]:3000",
      "0.0.0.0:3000",
      ":3000",
      "3000",
      "http://localhost:3000/path",
    ] {
      assert_eq!(local_upstream(upstream), Some(LocalUpstream { port: 3000 }));
      assert_eq!(display_upstream(upstream), ":3000");
    }
  }

  #[test]
  fn excludes_remote_and_dynamic_upstreams() {
    assert_eq!(local_upstream("api.example.com:3000"), None);
    assert_eq!(local_upstream("{env.UPSTREAM}"), None);
  }

  #[test]
  fn port_correlation_includes_only_local_upstreams() {
    let directory = tempfile::tempdir().unwrap();
    let caddyfile = directory.path().join("Caddyfile");
    std::fs::write(&caddyfile, "").unwrap();
    let snapshot = snapshot(&caddyfile);

    let routes = RegistrationIndex::new(&snapshot).routes_for_port(3000);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].domain.name.canonical, "local.example");
  }

  #[test]
  fn caddyfile_correlation_matches_normalized_path() {
    let directory = tempfile::tempdir().unwrap();
    let caddyfile = directory.path().join("Caddyfile");
    std::fs::write(&caddyfile, "").unwrap();
    let snapshot = snapshot(&caddyfile);

    let projects = RegistrationIndex::new(&snapshot).projects_for_caddyfile(&caddyfile);

    assert_eq!(projects.len(), 1);
  }
}
