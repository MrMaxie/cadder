use cadder_api::{domains_view, entrypoints_view};
use cadder_ipc::{GuiStateSnapshot, LogStreamIdentity};
use std::{collections::HashMap, path::Path};

#[derive(Debug, Clone, Default)]
pub struct DataModel {
  snapshot: Option<GuiStateSnapshot>,
  project_name_emphasis: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct DomainTableRow {
  entity: EntityId,
  kind: DomainRowKind,
  enabled: bool,
  visually_enabled: bool,
  name: String,
  name_emphasis_start: Option<usize>,
  endpoint: String,
  spaced_before: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityId {
  Entrypoint(String),
  Domain {
    registration_id: String,
    canonical_domain: String,
  },
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
    let previous_emphasis = std::mem::take(&mut self.project_name_emphasis);
    self.project_name_emphasis = entrypoints_view(&snapshot)
      .entrypoints
      .into_iter()
      .map(|entrypoint| {
        let working_directory = entrypoint.working_directory;
        let emphasis_start = previous_emphasis
          .get(&working_directory)
          .copied()
          .unwrap_or_else(|| project_name_emphasis_start(&working_directory));
        (working_directory, emphasis_start)
      })
      .collect();
    self.snapshot = Some(snapshot);
  }

  pub fn clear_snapshot(&mut self) {
    self.snapshot = None;
    self.project_name_emphasis.clear();
  }

  pub fn domain_rows(&self) -> Vec<DomainTableRow> {
    let Some(snapshot) = &self.snapshot else {
      return Vec::new();
    };
    let entrypoints = entrypoints_view(snapshot);
    let domains = domains_view(snapshot, None);
    let project_name_emphasis = &self.project_name_emphasis;

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
          name_emphasis_start: project_name_emphasis
            .get(&entrypoint.working_directory)
            .copied(),
          endpoint: String::new(),
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
            name_emphasis_start: None,
            endpoint: domain.upstream.clone().unwrap_or_default(),
            spaced_before: false,
          });

        std::iter::once(entrypoint_row).chain(domain_rows)
      })
      .collect()
  }

  pub fn mutation_target(&self, entity: &EntityId) -> Option<MutationTarget> {
    let snapshot = self.snapshot.as_ref()?;
    match entity {
      EntityId::Entrypoint(registration_id) => Self::find_entrypoint(snapshot, registration_id)
        .map(|entrypoint| MutationTarget {
          entity: entity.clone(),
          enabled: !entrypoint.activation_state.is_enabled(),
        }),
      EntityId::Domain {
        registration_id,
        canonical_domain,
      } => Self::find_domain(snapshot, registration_id, canonical_domain).map(|domain| {
        MutationTarget {
          entity: entity.clone(),
          enabled: !domain.activation_state.is_enabled(),
        }
      }),
    }
  }

  pub fn log_stream(&self, entity: &EntityId) -> Option<LogStreamIdentity> {
    let snapshot = self.snapshot.as_ref()?;
    match entity {
      EntityId::Entrypoint(registration_id) => Self::find_entrypoint(snapshot, registration_id)
        .map(|entrypoint| entrypoint.log_stream.clone()),
      EntityId::Domain {
        registration_id,
        canonical_domain,
      } => Self::find_domain(snapshot, registration_id, canonical_domain)
        .map(|domain| domain.log_stream.clone()),
    }
  }

  fn find_entrypoint<'a>(
    snapshot: &'a GuiStateSnapshot,
    registration_id: &str,
  ) -> Option<&'a cadder_ipc::EntrypointRegistration> {
    snapshot
      .registrations
      .iter()
      .find(|entrypoint| entrypoint.registration_id == registration_id)
  }

  fn find_domain<'a>(
    snapshot: &'a GuiStateSnapshot,
    registration_id: &str,
    canonical_domain: &str,
  ) -> Option<&'a cadder_ipc::RegisteredDomain> {
    Self::find_entrypoint(snapshot, registration_id)?
      .registered_domains
      .iter()
      .find(|domain| domain.name.canonical == canonical_domain)
  }

  pub fn log_title(&self, entity: &EntityId) -> String {
    match entity {
      EntityId::Entrypoint(registration_id) => self
        .snapshot
        .as_ref()
        .and_then(|snapshot| Self::find_entrypoint(snapshot, registration_id))
        .map_or_else(
          || "Logs".to_string(),
          |entrypoint| {
            Path::new(&entrypoint.source_working_directory.raw)
              .file_name()
              .map_or_else(
                || "Logs".to_string(),
                |name| format!("Logs - {}", name.to_string_lossy()),
              )
          },
        ),
      EntityId::Domain {
        canonical_domain, ..
      } => format!("Logs - {canonical_domain}"),
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

  pub const fn name_emphasis_start(&self) -> Option<usize> {
    self.name_emphasis_start
  }

  pub fn endpoint(&self) -> &str {
    &self.endpoint
  }

  pub const fn spaced_before(&self) -> bool {
    self.spaced_before
  }
}

fn project_name_emphasis_start(path: &str) -> usize {
  let path = Path::new(path);
  let git_root = path
    .ancestors()
    .find(|ancestor| ancestor.join(".git").exists());

  project_name_emphasis_start_with_root(&path.to_string_lossy(), git_root)
}

fn project_name_emphasis_start_with_root(path: &str, git_root: Option<&Path>) -> usize {
  if let Some(git_root) = git_root
    && let Ok(relative) = Path::new(path).strip_prefix(git_root)
    && !relative.as_os_str().is_empty()
  {
    let relative = relative.to_string_lossy();
    if path.ends_with(relative.as_ref()) {
      let relative_start = path.len() - relative.len();
      return path[..relative_start]
        .char_indices()
        .next_back()
        .filter(|(_, character)| is_path_separator(*character))
        .map_or(relative_start, |(index, _)| index);
    }
  }

  path
    .char_indices()
    .rfind(|(_, character)| is_path_separator(*character))
    .map_or(0, |(index, character)| index + character.len_utf8())
}

const fn is_path_separator(character: char) -> bool {
  matches!(character, '/' | '\\')
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_ipc::{
    ActivationState, ConfigState, DomainName, EntrypointInstanceIdentity, EntrypointRegistration,
    OwnerProcessIdentity, RegisteredDomain, RuntimeState, SourcePath, StorageState,
  };
  use chrono::Utc;

  fn registration(
    id: &str,
    project: &str,
    enabled: bool,
    domains: &[(&str, bool, &str)],
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
      source_config_path: SourcePath::new(format!("{project}/Caddyfile"), None),
      registered_domains: domains
        .iter()
        .map(|(name, enabled, upstream)| RegisteredDomain {
          name: DomainName::parse(*name),
          activation_state: ActivationState::from_enabled(*enabled),
          upstream: Some((*upstream).to_string()),
          log_stream: LogStreamIdentity::domain(name),
        })
        .collect(),
      activation_state: ActivationState::from_enabled(enabled),
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
    GuiStateSnapshot {
      captured_at_utc: Utc::now(),
      registrations: vec![
        registration(
          "entry-1",
          "workspace/project-1",
          true,
          &[
            ("app.localhost", true, "127.0.0.1:3000"),
            ("api.localhost", false, "localhost:4000"),
          ],
        ),
        registration(
          "entry-2",
          "workspace/project-2",
          false,
          &[("disabled.localhost", true, "[::1]:5000")],
        ),
      ],
      runtime: RuntimeState::idle(),
      config: ConfigState::idle(),
      storage: Some(StorageState {
        backend: "sqlite".to_string(),
        path: Some("runtime/data/cadder.sqlite3".to_string()),
        schema_version: 1,
        diagnostics: Vec::new(),
      }),
    }
  }

  #[test]
  fn snapshot_drives_rows_mutations_and_log_targets() {
    let mut model = DataModel::default();
    model.replace_snapshot(populated_snapshot());

    let rows = model.domain_rows();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].kind(), DomainRowKind::Entrypoint);
    assert!(rows[0].enabled());
    assert!(rows[0].visually_enabled());
    assert_eq!(rows[0].name(), "workspace/project-1");
    assert_eq!(rows[0].endpoint(), "");
    assert!(!rows[0].spaced_before());
    assert_eq!(
      &rows[0].name()[rows[0].name_emphasis_start().unwrap()..],
      "project-1"
    );
    assert_eq!(rows[1].kind(), DomainRowKind::Domain);
    assert!(rows[1].enabled());
    assert!(rows[1].visually_enabled());
    assert_eq!(rows[1].endpoint(), "127.0.0.1:3000");
    assert!(!rows[2].enabled());
    assert!(!rows[2].visually_enabled());
    assert!(rows[3].spaced_before());
    assert!(!rows[4].visually_enabled());

    for row in &rows {
      let entity = row.entity();
      assert!(model.log_title(&entity).starts_with("Logs"));
      assert!(model.log_stream(&entity).is_some());
      let target = model.mutation_target(&entity).unwrap();
      assert_eq!(target.entity, entity);
      assert_eq!(target.enabled, !row.enabled());
    }

    model.clear_snapshot();
    assert!(model.domain_rows().is_empty());
    let missing = EntityId::Entrypoint("missing".to_string());
    assert!(model.mutation_target(&missing).is_none());
    assert!(model.log_stream(&missing).is_none());
    assert_eq!(model.log_title(&missing), "Logs");
  }

  #[test]
  fn project_path_without_repository_emphasizes_only_last_component() {
    let path = Path::new("workspace").join("temporary").join("project-3");
    let path = path.to_string_lossy();
    let start = project_name_emphasis_start_with_root(&path, None);

    assert_eq!(&path[start..], "project-3");
  }

  #[test]
  fn project_path_inside_repository_emphasizes_the_repo_relative_path() {
    let repo = Path::new("workspace").join("smarketing");
    let path = repo.join("apps").join("reverse-proxy");
    let path = path.to_string_lossy();
    let start = project_name_emphasis_start_with_root(&path, Some(&repo));
    let expected = format!(
      "{}apps{}reverse-proxy",
      std::path::MAIN_SEPARATOR,
      std::path::MAIN_SEPARATOR
    );

    assert_eq!(&path[start..], expected);
  }
}
