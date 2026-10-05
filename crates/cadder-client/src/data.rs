use cadder_api::{domains_view, entrypoints_view};
use cadder_ipc::GuiStateSnapshot;
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
  last_in_project: bool,
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
    let mut entrypoints = entrypoints_view(snapshot);
    entrypoints
      .entrypoints
      .sort_by(|left, right| left.working_directory.cmp(&right.working_directory));
    let mut domains = domains_view(snapshot, None);
    domains.domains.sort_by(|left, right| {
      (&left.working_directory, &left.canonical_domain)
        .cmp(&(&right.working_directory, &right.canonical_domain))
    });
    let project_name_emphasis = &self.project_name_emphasis;

    entrypoints
      .entrypoints
      .iter()
      .flat_map(|entrypoint| {
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
          last_in_project: false,
        };
        let domain_count = domains
          .domains
          .iter()
          .filter(|domain| domain.registration_id == registration_id)
          .count();
        let domain_rows = domains
          .domains
          .iter()
          .filter(move |domain| domain.registration_id == registration_id)
          .enumerate()
          .map(move |(index, domain)| DomainTableRow {
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
            last_in_project: index + 1 == domain_count,
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

  pub const fn is_last_in_project(&self) -> bool {
    self.last_in_project
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
    LogStreamIdentity, OwnerProcessIdentity, RegisteredDomain, RuntimeState, SourcePath,
    StorageState,
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
  fn snapshot_drives_rows_and_mutations() {
    let mut model = DataModel::default();
    model.replace_snapshot(populated_snapshot());

    let rows = model.domain_rows();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].kind(), DomainRowKind::Entrypoint);
    assert!(rows[0].enabled());
    assert!(rows[0].visually_enabled());
    assert_eq!(rows[0].name(), "workspace/project-1");
    assert_eq!(rows[0].endpoint(), "");
    assert!(!rows[0].is_last_in_project());
    assert_eq!(
      &rows[0].name()[rows[0].name_emphasis_start().unwrap()..],
      "project-1"
    );
    let app = rows
      .iter()
      .find(|row| row.name() == "app.localhost")
      .unwrap();
    assert_eq!(app.kind(), DomainRowKind::Domain);
    assert!(app.enabled());
    assert!(app.visually_enabled());
    assert_eq!(app.endpoint(), "127.0.0.1:3000");
    assert!(app.is_last_in_project());

    let api = rows
      .iter()
      .find(|row| row.name() == "api.localhost")
      .unwrap();
    assert!(!api.enabled());
    assert!(!api.visually_enabled());
    assert!(!api.is_last_in_project());

    let disabled = rows
      .iter()
      .find(|row| row.name() == "disabled.localhost")
      .unwrap();
    assert!(!disabled.visually_enabled());
    assert!(disabled.is_last_in_project());

    for row in &rows {
      let entity = row.entity();
      let target = model.mutation_target(&entity).unwrap();
      assert_eq!(target.entity, entity);
      assert_eq!(target.enabled, !row.enabled());
    }

    model.clear_snapshot();
    assert!(model.domain_rows().is_empty());
    let missing = EntityId::Entrypoint("missing".to_string());
    assert!(model.mutation_target(&missing).is_none());
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
