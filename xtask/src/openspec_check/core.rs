use anyhow::{Result, bail};
use std::{
  collections::{BTreeMap, BTreeSet},
  fs,
  path::{Path, PathBuf},
};

const CAPABILITY_PREFIXES: [(&str, &str); 11] = [
  ("product-topology", "TOP-"),
  ("daemon-lifecycle", "RUN-"),
  ("project-registration", "REG-"),
  ("local-control-plane", "IPC-"),
  ("caddy-runtime", "CAD-"),
  ("runtime-storage", "STO-"),
  ("operator-cli", "CLI-"),
  ("operator-tui", "TUI-"),
  ("observability", "OBS-"),
  ("distribution-and-upgrades", "DST-"),
  ("documentation-experience", "DOC-"),
];
const VERIFICATION_GATES: [&str; 6] = [
  "Every task in `tasks.md` is complete.",
  "Every in-scope requirement ID has passing evidence.",
  "Focused tests pass.",
  "Repository checks required by the design pass.",
  "Documentation describes only verified behavior.",
  "`cargo xtask openspec-check` passes.",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangeSchema {
  SpecDriven,
  Implementation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequirementOperation {
  Main,
  Added,
  Modified,
  Removed,
  Renamed,
  Unknown,
}

impl RequirementOperation {
  fn from_heading(line: &str) -> Option<Self> {
    match line {
      "## ADDED Requirements" => Some(Self::Added),
      "## MODIFIED Requirements" => Some(Self::Modified),
      "## REMOVED Requirements" => Some(Self::Removed),
      "## RENAMED Requirements" => Some(Self::Renamed),
      _ => None,
    }
  }

  fn needs_normative_text(self) -> bool {
    matches!(self, Self::Main | Self::Added | Self::Modified)
  }

  fn needs_scenario(self) -> bool {
    matches!(self, Self::Main | Self::Added | Self::Modified)
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedRequirement {
  id: String,
  capability: String,
  operation: RequirementOperation,
  path: PathBuf,
  line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Diagnostic {
  path: PathBuf,
  line: usize,
  message: String,
}

impl Diagnostic {
  fn new(path: &Path, line: usize, message: impl Into<String>) -> Self {
    Self {
      path: path.to_path_buf(),
      line,
      message: message.into(),
    }
  }
}

#[derive(Debug)]
struct PendingRequirement {
  parsed: ParsedRequirement,
  has_normative_text: bool,
  scenario_count: usize,
}

pub(crate) fn validate_repository(root: &Path) -> Result<()> {
  let diagnostics = collect_repository_diagnostics(root);
  if diagnostics.is_empty() {
    return Ok(());
  }

  let mut message = format!(
    "OpenSpec repository validation failed with {} error(s):",
    diagnostics.len()
  );
  for diagnostic in diagnostics {
    message.push_str(&format!(
      "\n{}:{}: {}",
      display_path(root, &diagnostic.path),
      diagnostic.line,
      diagnostic.message
    ));
  }
  bail!(message)
}

pub(crate) fn spec_driven_change_names(root: &Path) -> Result<Vec<String>> {
  let changes_root = root.join("openspec").join("changes");
  let mut diagnostics = Vec::new();
  let mut names = Vec::new();
  for change_dir in sorted_directories(&changes_root, &mut diagnostics) {
    if change_dir.file_name().is_some_and(|name| name == "archive") {
      continue;
    }
    if classify_change(&change_dir.join(".openspec.yaml"), &mut diagnostics)
      == Some(ChangeSchema::SpecDriven)
      && let Some(name) = file_name(&change_dir, &mut diagnostics)
    {
      names.push(name);
    }
  }
  if diagnostics.is_empty() {
    Ok(names)
  } else {
    diagnostics.sort();
    let details = diagnostics
      .into_iter()
      .map(|diagnostic| {
        format!(
          "{}:{}: {}",
          display_path(root, &diagnostic.path),
          diagnostic.line,
          diagnostic.message
        )
      })
      .collect::<Vec<_>>()
      .join("\n");
    bail!("cannot classify active OpenSpec changes:\n{details}")
  }
}

fn collect_repository_diagnostics(root: &Path) -> Vec<Diagnostic> {
  let openspec_root = root.join("openspec");
  let mut diagnostics = Vec::new();
  validate_shared_artifact_rules(&openspec_root, &mut diagnostics);
  let main_requirements = load_main_requirements(&openspec_root, &mut diagnostics);
  let mut provided_capabilities = main_requirements
    .values()
    .map(|requirement| requirement.capability.clone())
    .collect::<BTreeSet<_>>();
  let mut added_requirements = BTreeMap::<String, PathBuf>::new();
  let changes_root = openspec_root.join("changes");

  for change_dir in sorted_directories(&changes_root, &mut diagnostics) {
    if change_dir.file_name().is_some_and(|name| name == "archive") {
      continue;
    }
    let schema_path = change_dir.join(".openspec.yaml");
    match classify_change(&schema_path, &mut diagnostics) {
      Some(ChangeSchema::SpecDriven) => validate_contract_change(
        &change_dir,
        &main_requirements,
        &mut added_requirements,
        &mut provided_capabilities,
        &mut diagnostics,
      ),
      Some(ChangeSchema::Implementation) => {
        validate_implementation_change(&change_dir, &main_requirements, &mut diagnostics)
      }
      None => {}
    }
  }

  for (capability, _) in CAPABILITY_PREFIXES {
    if !provided_capabilities.contains(capability) {
      diagnostics.push(Diagnostic::new(
        &openspec_root.join("specs").join(capability),
        1,
        format!("accepted Cadder contract is missing capability `{capability}`"),
      ));
    }
  }

  validate_content_boundaries(&openspec_root, root, &mut diagnostics);

  diagnostics.sort();
  diagnostics
}
