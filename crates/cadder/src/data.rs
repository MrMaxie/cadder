use color_eyre::eyre::{Context, Result};
use serde::Deserialize;

const MOCK_DATA: &str = include_str!("mock_data.yaml");
const MOCK_SETTINGS: &str = include_str!("mock_settings.yaml");

#[derive(Debug, Clone)]
pub struct DataModel {
  projects: Vec<Project>,
  settings: Settings,
}

#[derive(Debug, Clone)]
pub struct Project {
  name: String,
  enabled: bool,
  is_iis: bool,
  domains: Vec<Domain>,
}

#[derive(Debug, Clone)]
pub struct Domain {
  name: String,
  port: u16,
  enabled: bool,
}

#[derive(Debug, Clone)]
pub struct Settings {
  deamon_autorun: bool,
  rewire_iis: bool,
}

#[derive(Debug, Clone)]
pub struct DomainTableRow {
  entity: EntityId,
  kind: DomainRowKind,
  enabled: bool,
  visually_enabled: bool,
  name: String,
  port: Option<u16>,
  count: Option<usize>,
  spaced_before: bool,
  is_iis: bool,
}

#[derive(Debug, Clone)]
pub struct SettingsTableRow {
  entity: EntityId,
  enabled: bool,
  name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityId {
  Project(usize),
  Domain { project: usize, domain: usize },
  Setting(SettingId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingId {
  DeamonAutorun,
  RewireIis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainRowKind {
  Project,
  Domain,
}

impl DataModel {
  pub fn load() -> Result<Self> {
    let data: MockDataFile = serde_yaml::from_str(MOCK_DATA).wrap_err("parsing mock_data.yaml")?;
    let settings: SettingsFile =
      serde_yaml::from_str(MOCK_SETTINGS).wrap_err("parsing mock_settings.yaml")?;

    Ok(Self {
      projects: data.into_projects(),
      settings: Settings {
        deamon_autorun: settings.deamon_autorun,
        rewire_iis: settings.rewire_iis,
      },
    })
  }

  pub fn domain_rows(&self) -> Vec<DomainTableRow> {
    self
      .projects
      .iter()
      .enumerate()
      .flat_map(|(project_index, project)| {
        let project_row = DomainTableRow {
          entity: EntityId::Project(project_index),
          kind: DomainRowKind::Project,
          enabled: project.enabled,
          visually_enabled: project.enabled,
          name: project.name.clone(),
          port: None,
          count: Some(project.domains.len()),
          spaced_before: project_index > 0,
          is_iis: project.is_iis,
        };

        std::iter::once(project_row).chain(project.domains.iter().enumerate().map(
          move |(domain_index, domain)| DomainTableRow {
            entity: EntityId::Domain {
              project: project_index,
              domain: domain_index,
            },
            kind: DomainRowKind::Domain,
            enabled: domain.enabled,
            visually_enabled: project.enabled && domain.enabled,
            name: domain.name.clone(),
            port: Some(domain.port),
            count: None,
            spaced_before: false,
            is_iis: false,
          },
        ))
      })
      .collect()
  }

  pub fn settings_rows(&self) -> Vec<SettingsTableRow> {
    vec![
      SettingsTableRow {
        entity: EntityId::Setting(SettingId::DeamonAutorun),
        enabled: self.settings.deamon_autorun,
        name: "deamon autorun".to_string(),
      },
      SettingsTableRow {
        entity: EntityId::Setting(SettingId::RewireIis),
        enabled: self.settings.rewire_iis,
        name: "rewire IIS".to_string(),
      },
    ]
  }

  pub fn toggle(&mut self, entity: EntityId) {
    match entity {
      EntityId::Project(index) => {
        if let Some(project) = self.projects.get_mut(index) {
          project.enabled = !project.enabled;
        }
      }
      EntityId::Domain { project, domain } => {
        if let Some(domain) = self
          .projects
          .get_mut(project)
          .and_then(|project| project.domains.get_mut(domain))
        {
          domain.enabled = !domain.enabled;
        }
      }
      EntityId::Setting(SettingId::DeamonAutorun) => {
        self.settings.deamon_autorun = !self.settings.deamon_autorun;
      }
      EntityId::Setting(SettingId::RewireIis) => {
        self.settings.rewire_iis = !self.settings.rewire_iis;
      }
    }
  }

  pub fn describe(&self, entity: EntityId) -> Vec<String> {
    match entity {
      EntityId::Project(index) => self
        .projects
        .get(index)
        .map(|project| {
          vec![
            format!("Selected project: {}", project.name),
            format!("Enabled: {}", yes_no(project.enabled)),
            format!("Domains: {}", project.domains.len()),
          ]
        })
        .unwrap_or_else(|| vec!["Selected project: unavailable".to_string()]),
      EntityId::Domain { project, domain } => self
        .projects
        .get(project)
        .and_then(|project_data| {
          project_data
            .domains
            .get(domain)
            .map(|domain_data| (project_data, domain_data))
        })
        .map(|(project_data, domain_data)| {
          vec![
            format!("Selected domain: {}", domain_data.name),
            format!("Project: {}", project_data.name),
            format!("Port: :{}", domain_data.port),
            format!("Enabled: {}", yes_no(domain_data.enabled)),
          ]
        })
        .unwrap_or_else(|| vec!["Selected domain: unavailable".to_string()]),
      EntityId::Setting(SettingId::DeamonAutorun) => vec![
        "Selected setting: deamon autorun".to_string(),
        format!("Enabled: {}", yes_no(self.settings.deamon_autorun)),
      ],
      EntityId::Setting(SettingId::RewireIis) => vec![
        "Selected setting: rewire IIS".to_string(),
        format!("Enabled: {}", yes_no(self.settings.rewire_iis)),
      ],
    }
  }

  pub fn title(&self, entity: EntityId) -> String {
    match entity {
      EntityId::Project(index) => self
        .projects
        .get(index)
        .map(|project| format!(" {} ", project.name))
        .unwrap_or_else(|| " Selected ".to_string()),
      EntityId::Domain { project, domain } => self
        .projects
        .get(project)
        .and_then(|project| project.domains.get(domain))
        .map(|domain| format!(" {} ", domain.name))
        .unwrap_or_else(|| " Selected ".to_string()),
      EntityId::Setting(SettingId::DeamonAutorun) => " deamon autorun ".to_string(),
      EntityId::Setting(SettingId::RewireIis) => " rewire IIS ".to_string(),
    }
  }
}

impl DomainTableRow {
  pub const fn entity(&self) -> EntityId {
    self.entity
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

  pub const fn port(&self) -> Option<u16> {
    self.port
  }

  pub const fn count(&self) -> Option<usize> {
    self.count
  }

  pub const fn spaced_before(&self) -> bool {
    self.spaced_before
  }

  pub const fn is_iis(&self) -> bool {
    self.is_iis
  }
}

impl SettingsTableRow {
  pub const fn entity(&self) -> EntityId {
    self.entity
  }

  pub const fn enabled(&self) -> bool {
    self.enabled
  }

  pub fn name(&self) -> &str {
    &self.name
  }
}

#[derive(Debug, Deserialize)]
struct MockDataFile {
  #[serde(default)]
  projects: Vec<ProjectFile>,
  iis: Option<IisFile>,
}

#[derive(Debug, Deserialize)]
struct ProjectFile {
  name: String,
  #[serde(default)]
  enabled: bool,
  #[serde(default)]
  domains: Vec<DomainFile>,
}

#[derive(Debug, Deserialize)]
struct IisFile {
  #[serde(default)]
  enabled: bool,
  #[serde(default)]
  domains: Vec<DomainFile>,
}

#[derive(Debug, Deserialize)]
struct DomainFile {
  domain: String,
  port: u16,
  #[serde(default)]
  enabled: bool,
}

#[derive(Debug, Deserialize)]
struct SettingsFile {
  deamon_autorun: bool,
  #[serde(default)]
  rewire_iis: bool,
}

impl MockDataFile {
  fn into_projects(self) -> Vec<Project> {
    let mut projects = self
      .projects
      .into_iter()
      .map(|project| Project {
        name: project.name,
        enabled: project.enabled,
        is_iis: false,
        domains: project.domains.into_iter().map(Domain::from).collect(),
      })
      .collect::<Vec<_>>();

    if let Some(iis) = self.iis {
      projects.push(Project {
        name: "IIS".to_string(),
        enabled: iis.enabled,
        is_iis: true,
        domains: iis.domains.into_iter().map(Domain::from).collect(),
      });
    }

    projects
  }
}

impl From<DomainFile> for Domain {
  fn from(value: DomainFile) -> Self {
    Self {
      name: value.domain,
      port: value.port,
      enabled: value.enabled,
    }
  }
}

const fn yes_no(value: bool) -> &'static str {
  if value { "yes" } else { "no" }
}
