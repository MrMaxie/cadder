use cadder_operator::{domains_view, entrypoints_view};
use cadder_protocol::{GuiStateSnapshot, LogStreamIdentity};

#[derive(Debug, Clone, Default)]
pub struct DataModel {
  snapshot: Option<GuiStateSnapshot>,
}

#[derive(Debug, Clone)]
pub struct DomainTableRow {
  entity: EntityId,
  kind: DomainRowKind,
  enabled: bool,
  visually_enabled: bool,
  name: String,
  endpoint: String,
  count: Option<usize>,
  spaced_before: bool,
}

#[derive(Debug, Clone)]
pub struct SettingsTableRow {
  entity: EntityId,
  name: String,
  value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityId {
  Entrypoint(String),
  Domain {
    registration_id: String,
    canonical_domain: String,
  },
  Status(StatusId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusId {
  Connection,
  Runtime,
  Config,
  Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainRowKind {
  Entrypoint,
  Domain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationTarget {
  pub entity: EntityId,
  pub enabled: bool,
}

impl DataModel {
  pub fn replace_snapshot(&mut self, snapshot: GuiStateSnapshot) {
    self.snapshot = Some(snapshot);
  }

  pub fn clear_snapshot(&mut self) {
    self.snapshot = None;
  }

  pub fn domain_rows(&self) -> Vec<DomainTableRow> {
    let Some(snapshot) = &self.snapshot else {
      return Vec::new();
    };
    let entrypoints = entrypoints_view(snapshot);
    let domains = domains_view(snapshot, None);

    entrypoints
      .entrypoints
      .iter()
      .enumerate()
      .flat_map(|(entrypoint_index, entrypoint)| {
        let registration_id = entrypoint.registration_id.clone();
        let entrypoint_enabled = entrypoint.activation_state.is_enabled();
        let entrypoint_row = DomainTableRow {
          entity: EntityId::Entrypoint(registration_id.clone()),
          kind: DomainRowKind::Entrypoint,
          enabled: entrypoint_enabled,
          visually_enabled: entrypoint_enabled,
          name: entrypoint.working_directory.clone(),
          endpoint: format!("{} domains", entrypoint.domain_count),
          count: Some(entrypoint.domain_count),
          spaced_before: entrypoint_index > 0,
        };
        let domain_rows = domains
          .domains
          .iter()
          .filter(move |domain| domain.registration_id == registration_id)
          .map(move |domain| DomainTableRow {
            entity: EntityId::Domain {
              registration_id: domain.registration_id.clone(),
              canonical_domain: domain.canonical_domain.clone(),
            },
            kind: DomainRowKind::Domain,
            enabled: domain.activation_state.is_enabled(),
            visually_enabled: entrypoint_enabled && domain.activation_state.is_enabled(),
            name: domain.domain.clone(),
            endpoint: domain.upstream.clone().unwrap_or_default(),
            count: None,
            spaced_before: false,
          });

        std::iter::once(entrypoint_row).chain(domain_rows)
      })
      .collect()
  }

  pub fn status_rows(&self, connection: &str) -> Vec<SettingsTableRow> {
    let mut rows = vec![SettingsTableRow {
      entity: EntityId::Status(StatusId::Connection),
      name: "Daemon".to_string(),
      value: connection.to_string(),
    }];
    if let Some(snapshot) = &self.snapshot {
      rows.extend([
        SettingsTableRow {
          entity: EntityId::Status(StatusId::Runtime),
          name: "Caddy runtime".to_string(),
          value: format!("{:?}", snapshot.runtime.status),
        },
        SettingsTableRow {
          entity: EntityId::Status(StatusId::Config),
          name: "Configuration".to_string(),
          value: format!("{:?}", snapshot.config.status),
        },
        SettingsTableRow {
          entity: EntityId::Status(StatusId::Storage),
          name: "Storage".to_string(),
          value: snapshot.storage.as_ref().map_or_else(
            || "Unavailable".to_string(),
            |storage| storage.backend.clone(),
          ),
        },
      ]);
    }
    rows
  }

  pub fn mutation_target(&self, entity: &EntityId) -> Option<MutationTarget> {
    let snapshot = self.snapshot.as_ref()?;
    match entity {
      EntityId::Entrypoint(registration_id) => snapshot
        .registrations
        .iter()
        .find(|entrypoint| entrypoint.registration_id == *registration_id)
        .map(|entrypoint| MutationTarget {
          entity: entity.clone(),
          enabled: !entrypoint.activation_state.is_enabled(),
        }),
      EntityId::Domain {
        registration_id,
        canonical_domain,
      } => snapshot
        .registrations
        .iter()
        .find(|entrypoint| entrypoint.registration_id == *registration_id)
        .and_then(|entrypoint| {
          entrypoint
            .registered_domains
            .iter()
            .find(|domain| domain.name.canonical == *canonical_domain)
        })
        .map(|domain| MutationTarget {
          entity: entity.clone(),
          enabled: !domain.activation_state.is_enabled(),
        }),
      EntityId::Status(_) => None,
    }
  }

  pub fn log_stream(&self, entity: &EntityId) -> Option<LogStreamIdentity> {
    let snapshot = self.snapshot.as_ref()?;
    match entity {
      EntityId::Entrypoint(registration_id) => snapshot
        .registrations
        .iter()
        .find(|entrypoint| entrypoint.registration_id == *registration_id)
        .map(|entrypoint| entrypoint.log_stream.clone()),
      EntityId::Domain {
        registration_id,
        canonical_domain,
      } => snapshot
        .registrations
        .iter()
        .find(|entrypoint| entrypoint.registration_id == *registration_id)
        .and_then(|entrypoint| {
          entrypoint
            .registered_domains
            .iter()
            .find(|domain| domain.name.canonical == *canonical_domain)
        })
        .map(|domain| domain.log_stream.clone()),
      EntityId::Status(_) => None,
    }
  }

  pub fn describe(&self, entity: &EntityId) -> Vec<String> {
    let Some(snapshot) = &self.snapshot else {
      return vec!["No daemon snapshot is available.".to_string()];
    };
    match entity {
      EntityId::Entrypoint(registration_id) => entrypoints_view(snapshot)
        .entrypoints
        .into_iter()
        .find(|entrypoint| entrypoint.registration_id == *registration_id)
        .map(|entrypoint| {
          vec![
            format!("Registration: {}", entrypoint.registration_id),
            format!("State: {:?}", entrypoint.activation_state),
            format!("Working directory: {}", entrypoint.working_directory),
            format!("Config: {}", entrypoint.config_path),
            format!("Process: {}", entrypoint.process_id),
            format!("Domains: {}", entrypoint.domain_count),
          ]
        })
        .unwrap_or_else(|| vec!["Entrypoint is no longer present.".to_string()]),
      EntityId::Domain {
        registration_id,
        canonical_domain,
      } => domains_view(snapshot, Some(registration_id))
        .domains
        .into_iter()
        .find(|domain| domain.canonical_domain == *canonical_domain)
        .map(|domain| {
          vec![
            format!("Domain: {}", domain.domain),
            format!("State: {:?}", domain.activation_state),
            format!("Entrypoint: {}", domain.registration_id),
            format!("Upstream: {}", domain.upstream.as_deref().unwrap_or("none")),
            format!("Config: {}", domain.config_path),
          ]
        })
        .unwrap_or_else(|| vec!["Domain is no longer present.".to_string()]),
      EntityId::Status(status) => self.describe_status(*status),
    }
  }

  pub fn title(&self, entity: &EntityId) -> String {
    match entity {
      EntityId::Entrypoint(registration_id) => format!(" {registration_id} "),
      EntityId::Domain {
        canonical_domain, ..
      } => format!(" {canonical_domain} "),
      EntityId::Status(status) => format!(" {status:?} "),
    }
  }

  fn describe_status(&self, status: StatusId) -> Vec<String> {
    let Some(snapshot) = &self.snapshot else {
      return vec!["No daemon snapshot is available.".to_string()];
    };
    match status {
      StatusId::Connection => vec![
        "Daemon connection is active.".to_string(),
        format!("Snapshot captured: {}", snapshot.captured_at_utc),
      ],
      StatusId::Runtime => vec![
        format!("Caddy runtime: {:?}", snapshot.runtime.status),
        format!(
          "Version: {}",
          snapshot.runtime.version.as_deref().unwrap_or("unavailable")
        ),
        format!(
          "Admin endpoint: {}",
          snapshot
            .runtime
            .admin_endpoint
            .as_deref()
            .unwrap_or("unavailable")
        ),
      ],
      StatusId::Config => vec![
        format!("Configuration: {:?}", snapshot.config.status),
        format!("Diagnostics: {}", snapshot.config.diagnostics.len()),
      ],
      StatusId::Storage => snapshot.storage.as_ref().map_or_else(
        || vec!["Storage information is unavailable.".to_string()],
        |storage| {
          vec![
            format!("Backend: {}", storage.backend),
            format!("Schema version: {}", storage.schema_version),
          ]
        },
      ),
    }
  }
}

impl DomainTableRow {
  pub fn entity(&self) -> EntityId {
    self.entity.clone()
  }

  pub const fn kind(&self) -> DomainRowKind {
    self.kind
  }

  pub const fn enabled(&self) -> bool {
    self.enabled
  }

  pub const fn visually_enabled(&self) -> bool {
    self.visually_enabled
  }

  pub fn name(&self) -> &str {
    &self.name
  }

  pub fn endpoint(&self) -> &str {
    &self.endpoint
  }

  pub const fn count(&self) -> Option<usize> {
    self.count
  }

  pub const fn spaced_before(&self) -> bool {
    self.spaced_before
  }
}

impl SettingsTableRow {
  pub fn entity(&self) -> EntityId {
    self.entity.clone()
  }

  pub fn name(&self) -> &str {
    &self.name
  }

  pub fn value(&self) -> &str {
    &self.value
  }
}
