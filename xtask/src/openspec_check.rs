use anyhow::{Result, bail};
use std::{
  collections::{BTreeMap, BTreeSet},
  fs,
  path::{Path, PathBuf},
};

const CAPABILITY_PREFIXES: [(&str, &str); 12] = [
  ("product-topology", "TOP-"),
  ("daemon-lifecycle", "RUN-"),
  ("project-registration", "REG-"),
  ("local-control-plane", "IPC-"),
  ("caddy-runtime", "CAD-"),
  ("runtime-storage", "STO-"),
  ("operator-cli", "CLI-"),
  ("operator-tui", "TUI-"),
  ("observability", "OBS-"),
  ("windows-iis-handoff", "IIS-"),
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

fn load_main_requirements(
  openspec_root: &Path,
  diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, ParsedRequirement> {
  let specs_root = openspec_root.join("specs");
  let mut requirements = BTreeMap::new();
  for capability_dir in sorted_directories(&specs_root, diagnostics) {
    let Some(capability) = file_name(&capability_dir, diagnostics) else {
      continue;
    };
    let Some(prefix) = capability_prefix(&capability) else {
      diagnostics.push(Diagnostic::new(
        &capability_dir,
        1,
        format!("unknown main-spec capability `{capability}`"),
      ));
      continue;
    };
    let spec_path = capability_dir.join("spec.md");
    let Some(contents) = read_required(&spec_path, diagnostics) else {
      continue;
    };
    validate_main_spec_structure(&spec_path, &contents, diagnostics);
    let (parsed, mut parse_diagnostics) =
      parse_spec_text(&spec_path, &capability, prefix, true, &contents);
    diagnostics.append(&mut parse_diagnostics);
    for requirement in parsed {
      if let Some(previous) = requirements.insert(requirement.id.clone(), requirement.clone()) {
        diagnostics.push(Diagnostic::new(
          &requirement.path,
          requirement.line,
          format!(
            "requirement ID `{}` duplicates {}:{}",
            requirement.id,
            previous.path.display(),
            previous.line
          ),
        ));
      }
    }
  }
  requirements
}

fn validate_main_spec_structure(path: &Path, contents: &str, diagnostics: &mut Vec<Diagnostic>) {
  let lines = contents.lines().collect::<Vec<_>>();
  let purpose_heading = lines.iter().position(|line| *line == "## Purpose");
  let requirements_heading = lines.iter().position(|line| *line == "## Requirements");
  if purpose_heading.is_none() {
    diagnostics.push(Diagnostic::new(
      path,
      1,
      "main spec is missing `## Purpose`",
    ));
  }
  if requirements_heading.is_none() {
    diagnostics.push(Diagnostic::new(
      path,
      1,
      "main spec is missing `## Requirements`",
    ));
  }
  if let Some(purpose_heading) = purpose_heading {
    let purpose = lines
      .iter()
      .skip(purpose_heading + 1)
      .take_while(|line| !line.starts_with("## "))
      .map(|line| line.trim())
      .filter(|line| !line.is_empty())
      .collect::<Vec<_>>()
      .join(" ");
    if purpose.len() < 50
      || contains_placeholder(&purpose)
      || purpose.contains("created by archiving")
    {
      diagnostics.push(Diagnostic::new(
        path,
        purpose_heading + 1,
        "main spec purpose must be a concrete product-level description of at least 50 characters",
      ));
    }
  }
}

fn classify_change(path: &Path, diagnostics: &mut Vec<Diagnostic>) -> Option<ChangeSchema> {
  let contents = read_required(path, diagnostics)?;
  let schema_lines = contents
    .lines()
    .enumerate()
    .filter_map(|(index, line)| {
      line
        .strip_prefix("schema:")
        .map(|value| (index + 1, value.trim()))
    })
    .collect::<Vec<_>>();
  if schema_lines.len() != 1 {
    diagnostics.push(Diagnostic::new(
      path,
      schema_lines.first().map_or(1, |(line, _)| *line),
      "change metadata must contain exactly one top-level `schema:` field",
    ));
    return None;
  }

  let (line, value) = schema_lines[0];
  match value {
    "spec-driven" => Some(ChangeSchema::SpecDriven),
    "implementation" => Some(ChangeSchema::Implementation),
    _ => {
      diagnostics.push(Diagnostic::new(
        path,
        line,
        format!("unsupported OpenSpec change schema `{value}`"),
      ));
      None
    }
  }
}

fn validate_contract_change(
  change_dir: &Path,
  main_requirements: &BTreeMap<String, ParsedRequirement>,
  added_requirements: &mut BTreeMap<String, PathBuf>,
  provided_capabilities: &mut BTreeSet<String>,
  diagnostics: &mut Vec<Diagnostic>,
) {
  let proposal_path = change_dir.join("proposal.md");
  let proposal_capabilities = read_required(&proposal_path, diagnostics)
    .map(|contents| parse_proposal_capabilities(&proposal_path, &contents, diagnostics))
    .unwrap_or_default();
  let specs_root = change_dir.join("specs");
  let spec_dirs = sorted_directories(&specs_root, diagnostics);
  let actual_capabilities = spec_dirs
    .iter()
    .filter_map(|path| file_name(path, diagnostics))
    .collect::<BTreeSet<_>>();

  let declared_capabilities = proposal_capabilities.all();
  for capability in declared_capabilities.difference(&actual_capabilities) {
    diagnostics.push(Diagnostic::new(
      &proposal_path,
      1,
      format!("proposal capability `{capability}` has no delta spec"),
    ));
  }
  for capability in actual_capabilities.difference(&declared_capabilities) {
    diagnostics.push(Diagnostic::new(
      &specs_root.join(capability),
      1,
      format!("delta capability `{capability}` is not declared in proposal.md"),
    ));
  }

  for capability_dir in spec_dirs {
    let Some(capability) = file_name(&capability_dir, diagnostics) else {
      continue;
    };
    let Some(prefix) = capability_prefix(&capability) else {
      diagnostics.push(Diagnostic::new(
        &capability_dir,
        1,
        format!("unknown delta capability `{capability}`"),
      ));
      continue;
    };
    let spec_path = capability_dir.join("spec.md");
    let Some(contents) = read_required(&spec_path, diagnostics) else {
      continue;
    };
    let (parsed, mut parse_diagnostics) =
      parse_spec_text(&spec_path, &capability, prefix, false, &contents);
    diagnostics.append(&mut parse_diagnostics);
    if !parsed.is_empty() {
      provided_capabilities.insert(capability.clone());
    }
    if proposal_capabilities.new.contains(&capability) {
      for requirement in parsed
        .iter()
        .filter(|requirement| requirement.operation != RequirementOperation::Added)
      {
        diagnostics.push(Diagnostic::new(
          &requirement.path,
          requirement.line,
          format!("new capability `{capability}` may contain only ADDED requirements"),
        ));
      }
    }
    if proposal_capabilities.modified.contains(&capability)
      && !main_requirements
        .values()
        .any(|requirement| requirement.capability == capability)
    {
      diagnostics.push(Diagnostic::new(
        &proposal_path,
        1,
        format!("modified capability `{capability}` does not exist in main specs"),
      ));
    }
    for requirement in parsed {
      match requirement.operation {
        RequirementOperation::Added => {
          if main_requirements.contains_key(&requirement.id) {
            diagnostics.push(Diagnostic::new(
              &requirement.path,
              requirement.line,
              format!(
                "ADDED requirement `{}` already exists in main specs",
                requirement.id
              ),
            ));
          }
          if let Some(previous) =
            added_requirements.insert(requirement.id.clone(), requirement.path.clone())
          {
            diagnostics.push(Diagnostic::new(
              &requirement.path,
              requirement.line,
              format!(
                "ADDED requirement `{}` is also introduced by {}",
                requirement.id,
                previous.display()
              ),
            ));
          }
        }
        RequirementOperation::Modified
        | RequirementOperation::Removed
        | RequirementOperation::Renamed => match main_requirements.get(&requirement.id) {
          Some(main) if main.capability == requirement.capability => {}
          Some(main) => diagnostics.push(Diagnostic::new(
            &requirement.path,
            requirement.line,
            format!(
              "{} requirement `{}` belongs to main capability `{}`",
              operation_name(requirement.operation),
              requirement.id,
              main.capability
            ),
          )),
          None => diagnostics.push(Diagnostic::new(
            &requirement.path,
            requirement.line,
            format!(
              "{} requirement `{}` does not exist in main specs",
              operation_name(requirement.operation),
              requirement.id
            ),
          )),
        },
        RequirementOperation::Unknown => {}
        RequirementOperation::Main => unreachable!("delta parser cannot emit main requirements"),
      }
    }
  }
}

#[derive(Debug, Default)]
struct ProposalCapabilities {
  new: BTreeSet<String>,
  modified: BTreeSet<String>,
}

impl ProposalCapabilities {
  fn all(&self) -> BTreeSet<String> {
    self.new.union(&self.modified).cloned().collect()
  }
}

#[derive(Debug, Clone, Copy)]
enum CapabilitySection {
  New,
  Modified,
}

fn parse_proposal_capabilities(
  path: &Path,
  contents: &str,
  diagnostics: &mut Vec<Diagnostic>,
) -> ProposalCapabilities {
  let mut capabilities = ProposalCapabilities::default();
  let mut section = None;
  for (index, line) in contents.lines().enumerate() {
    if line.eq_ignore_ascii_case("### New capabilities") {
      section = Some(CapabilitySection::New);
      continue;
    }
    if line.eq_ignore_ascii_case("### Modified capabilities") {
      section = Some(CapabilitySection::Modified);
      continue;
    }
    if line.starts_with("### ") || line.starts_with("## ") {
      section = None;
    }
    let Some(section) = section else {
      continue;
    };
    if !line.starts_with("- `") {
      continue;
    }
    let Some(rest) = line.strip_prefix("- `") else {
      continue;
    };
    let Some((capability, _)) = rest.split_once('`') else {
      diagnostics.push(Diagnostic::new(
        path,
        index + 1,
        "capability declaration must wrap its name in backticks",
      ));
      continue;
    };
    if capability.is_empty() || capability.contains(char::is_whitespace) {
      diagnostics.push(Diagnostic::new(
        path,
        index + 1,
        format!("invalid capability name `{capability}`"),
      ));
      continue;
    }
    let target = match section {
      CapabilitySection::New => &mut capabilities.new,
      CapabilitySection::Modified => &mut capabilities.modified,
    };
    if !target.insert(capability.to_string()) {
      diagnostics.push(Diagnostic::new(
        path,
        index + 1,
        format!("capability `{capability}` is declared more than once"),
      ));
    }
    if capabilities.new.contains(capability) && capabilities.modified.contains(capability) {
      diagnostics.push(Diagnostic::new(
        path,
        index + 1,
        format!("capability `{capability}` cannot be both new and modified"),
      ));
    }
  }
  capabilities
}

fn validate_implementation_change(
  change_dir: &Path,
  main_requirements: &BTreeMap<String, ParsedRequirement>,
  diagnostics: &mut Vec<Diagnostic>,
) {
  let specs_path = change_dir.join("specs");
  if specs_path.exists() {
    diagnostics.push(Diagnostic::new(
      &specs_path,
      1,
      "implementation changes must not contain delta specs",
    ));
  }

  let proposal_path = change_dir.join("proposal.md");
  let design_path = change_dir.join("design.md");
  let tasks_path = change_dir.join("tasks.md");
  let Some(proposal) = read_required(&proposal_path, diagnostics) else {
    return;
  };
  let design = read_required(&design_path, diagnostics);
  let tasks = read_required(&tasks_path, diagnostics);
  let requirement_ids = parse_requirement_id_section(&proposal_path, &proposal, diagnostics);

  for id in &requirement_ids {
    if !main_requirements.contains_key(id) {
      diagnostics.push(Diagnostic::new(
        &proposal_path,
        1,
        format!("implementation requirement `{id}` does not exist in main specs"),
      ));
    }
    if design
      .as_ref()
      .is_some_and(|contents| !contents.contains(id))
    {
      diagnostics.push(Diagnostic::new(
        &design_path,
        1,
        format!("design.md does not address requirement `{id}`"),
      ));
    }
  }

  let Some(tasks) = tasks else {
    return;
  };
  let parsed_tasks = parse_tasks(&tasks_path, &tasks, &requirement_ids, diagnostics);
  let covered_ids = parsed_tasks
    .iter()
    .flat_map(|task| task.requirement_ids.iter().cloned())
    .collect::<BTreeSet<_>>();
  for id in requirement_ids.difference(&covered_ids) {
    diagnostics.push(Diagnostic::new(
      &tasks_path,
      1,
      format!("no implementation task covers requirement `{id}`"),
    ));
  }

  let all_tasks_complete =
    !parsed_tasks.is_empty() && parsed_tasks.iter().all(|task| task.complete);
  let verification_path = change_dir.join("verification.md");
  match (all_tasks_complete, verification_path.exists()) {
    (false, true) => diagnostics.push(Diagnostic::new(
      &verification_path,
      1,
      "verification.md must not exist before every implementation task is complete",
    )),
    (true, false) => diagnostics.push(Diagnostic::new(
      &verification_path,
      1,
      "verification.md is required after every implementation task is complete",
    )),
    (true, true) => {
      if let Some(verification) = read_required(&verification_path, diagnostics) {
        validate_verification(
          &verification_path,
          &verification,
          &requirement_ids,
          &parsed_tasks,
          diagnostics,
        );
      }
    }
    (false, false) => {}
  }
}

#[derive(Debug)]
struct ImplementationTask {
  number: String,
  complete: bool,
  requirement_ids: BTreeSet<String>,
}

fn parse_requirement_id_section(
  path: &Path,
  contents: &str,
  diagnostics: &mut Vec<Diagnostic>,
) -> BTreeSet<String> {
  let mut in_section = false;
  let mut ids = BTreeSet::new();
  for (index, line) in contents.lines().enumerate() {
    if line == "## Requirement IDs" {
      in_section = true;
      continue;
    }
    if in_section && line.starts_with("## ") {
      break;
    }
    if !in_section || !line.starts_with("- ") {
      continue;
    }
    let line_ids = extract_known_requirement_ids(line);
    if line_ids.len() != 1 {
      diagnostics.push(Diagnostic::new(
        path,
        index + 1,
        "each Requirement IDs list item must contain exactly one stable requirement ID",
      ));
      continue;
    }
    let id = line_ids.into_iter().next().expect("length checked");
    if !ids.insert(id.clone()) {
      diagnostics.push(Diagnostic::new(
        path,
        index + 1,
        format!("requirement `{id}` is listed more than once"),
      ));
    }
  }
  if !in_section || ids.is_empty() {
    diagnostics.push(Diagnostic::new(
      path,
      1,
      "implementation proposal must list at least one stable ID under `## Requirement IDs`",
    ));
  }
  ids
}

fn parse_tasks(
  path: &Path,
  contents: &str,
  in_scope: &BTreeSet<String>,
  diagnostics: &mut Vec<Diagnostic>,
) -> Vec<ImplementationTask> {
  let mut tasks = Vec::new();
  let mut numbers = BTreeSet::new();
  for (index, line) in contents.lines().enumerate() {
    if !line.starts_with("- [") {
      continue;
    }
    let line_number = index + 1;
    let complete = match line.get(3..4) {
      Some(" ") => false,
      Some("x" | "X") => true,
      _ => {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          "task must use `- [ ]` or `- [x]` checkbox syntax",
        ));
        false
      }
    };
    if line.get(4..6) != Some("] ") {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        "task checkbox must be followed by a numbered task",
      ));
      continue;
    }
    let remainder = &line[6..];
    let number = remainder
      .split_whitespace()
      .next()
      .unwrap_or_default()
      .to_string();
    if !is_task_number(&number) {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        format!("invalid task number `{number}`; expected `N.N`"),
      ));
    } else if !numbers.insert(number.clone()) {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        format!("task number `{number}` is duplicated"),
      ));
    }

    let task_description = line
      .split_once("(verification:")
      .map_or(line, |(description, _)| description);
    let requirement_ids = extract_task_requirement_ids(task_description);
    if requirement_ids.is_empty() {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        format!("task `{number}` must reference at least one stable requirement ID"),
      ));
    }
    for id in requirement_ids.difference(in_scope) {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        format!("task `{number}` references out-of-scope requirement `{id}`"),
      ));
    }

    let verification = line
      .find("(verification:")
      .and_then(|start| line.get(start + "(verification:".len()..))
      .and_then(|value| value.strip_suffix(')'))
      .map(str::trim);
    if verification.is_none_or(|value| value.is_empty() || contains_placeholder(value)) {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        format!("task `{number}` must name a concrete `(verification: ...)` check"),
      ));
    }

    tasks.push(ImplementationTask {
      number,
      complete,
      requirement_ids,
    });
  }
  if tasks.is_empty() {
    diagnostics.push(Diagnostic::new(
      path,
      1,
      "implementation tasks.md must contain at least one numbered checkbox task",
    ));
  }
  tasks
}

fn validate_verification(
  path: &Path,
  contents: &str,
  requirement_ids: &BTreeSet<String>,
  tasks: &[ImplementationTask],
  diagnostics: &mut Vec<Diagnostic>,
) {
  let task_numbers = tasks
    .iter()
    .map(|task| task.number.clone())
    .collect::<BTreeSet<_>>();
  let task_requirements = tasks
    .iter()
    .map(|task| (task.number.as_str(), &task.requirement_ids))
    .collect::<BTreeMap<_, _>>();
  let mut evidenced_ids = BTreeSet::new();
  let mut evidenced_tasks = BTreeSet::new();
  let mut in_evidence = false;
  let mut in_gate = false;
  let mut completed_gates = BTreeSet::new();

  for (index, line) in contents.lines().enumerate() {
    let line_number = index + 1;
    if line.starts_with("## ") {
      in_evidence = line == "## Evidence";
      in_gate = line == "## Gate";
      continue;
    }
    if in_gate && line.starts_with("- [") {
      let complete = line.starts_with("- [x] ") || line.starts_with("- [X] ");
      let unchecked = line.starts_with("- [ ] ");
      if !complete && !unchecked {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          "verification gate must use exact `- [x] Description` checkbox syntax",
        ));
        continue;
      }
      let description = &line[6..];
      if !VERIFICATION_GATES.contains(&description) {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          format!("unknown verification gate `{description}`"),
        ));
        continue;
      }
      if !complete {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          format!("verification gate `{description}` is not complete"),
        ));
      } else if !completed_gates.insert(description.to_string()) {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          format!("verification gate `{description}` is duplicated"),
        ));
      }
      continue;
    }

    if !in_evidence || !line.starts_with('|') {
      continue;
    }
    let cells = line
      .trim_matches('|')
      .split('|')
      .map(|cell| cell.trim().trim_matches('`'))
      .collect::<Vec<_>>();
    if cells.first() == Some(&"Requirement ID")
      || cells.first().is_some_and(|cell| cell.starts_with("---"))
    {
      continue;
    }
    if cells.len() != 4 {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        "evidence rows must contain exactly requirement ID, task, evidence, and result",
      ));
      continue;
    }
    let id = cells[0];
    let task = cells[1];
    let evidence = cells[2];
    let result = cells[3];
    if extract_known_requirement_ids(id) != BTreeSet::from([id.to_string()]) {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        format!("evidence row contains invalid requirement ID `{id}`"),
      ));
      continue;
    }
    if !requirement_ids.contains(id) {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        format!("evidence references out-of-scope requirement `{id}`"),
      ));
      continue;
    }
    if !task_numbers.contains(task) {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        format!("evidence references unknown task `{task}`"),
      ));
      continue;
    }
    if task_requirements
      .get(task)
      .is_some_and(|requirements| !requirements.contains(id))
    {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        format!("task `{task}` does not implement evidenced requirement `{id}`"),
      ));
      continue;
    }
    if evidence.is_empty() || contains_placeholder(evidence) {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        format!("evidence for requirement `{id}` and task `{task}` is not concrete"),
      ));
    }
    if result != "Pass" {
      diagnostics.push(Diagnostic::new(
        path,
        line_number,
        format!("evidence for requirement `{id}` and task `{task}` must be `Pass`"),
      ));
      continue;
    }
    evidenced_ids.insert(id.to_string());
    evidenced_tasks.insert(task.to_string());
  }

  for id in requirement_ids.difference(&evidenced_ids) {
    diagnostics.push(Diagnostic::new(
      path,
      1,
      format!("verification has no passing evidence for requirement `{id}`"),
    ));
  }
  for task in task_numbers.difference(&evidenced_tasks) {
    diagnostics.push(Diagnostic::new(
      path,
      1,
      format!("verification has no passing evidence for task `{task}`"),
    ));
  }
  for gate in VERIFICATION_GATES {
    if !completed_gates.contains(gate) {
      diagnostics.push(Diagnostic::new(
        path,
        1,
        format!("verification is missing completed gate `{gate}`"),
      ));
    }
  }
  if !contents.lines().any(|line| line == "## Evidence") {
    diagnostics.push(Diagnostic::new(
      path,
      1,
      "verification must contain an `## Evidence` section",
    ));
  }
}

fn is_task_number(value: &str) -> bool {
  let Some((section, item)) = value.split_once('.') else {
    return false;
  };
  !section.is_empty()
    && !item.is_empty()
    && section.bytes().all(|byte| byte.is_ascii_digit())
    && item.bytes().all(|byte| byte.is_ascii_digit())
    && section.bytes().any(|byte| byte != b'0')
    && item.bytes().any(|byte| byte != b'0')
}

fn extract_known_requirement_ids(line: &str) -> BTreeSet<String> {
  let bytes = line.as_bytes();
  if bytes.len() < 7 {
    return BTreeSet::new();
  }
  (0..=bytes.len() - 7)
    .filter(|start| {
      (*start == 0 || !is_requirement_token_byte(bytes[*start - 1]))
        && (*start + 7 == bytes.len() || !is_requirement_token_byte(bytes[*start + 7]))
    })
    .filter_map(|start| line.get(start..start + 7))
    .filter(|candidate| {
      is_requirement_id(candidate)
        && CAPABILITY_PREFIXES
          .iter()
          .any(|(_, prefix)| candidate.starts_with(prefix))
    })
    .map(str::to_string)
    .collect()
}

fn extract_task_requirement_ids(description: &str) -> BTreeSet<String> {
  extract_known_requirement_ids(description)
    .into_iter()
    .filter(|id| description.contains(&format!("[{id}]")))
    .collect()
}

fn is_requirement_token_byte(byte: u8) -> bool {
  byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

fn contains_placeholder(value: &str) -> bool {
  let lower = value.to_ascii_lowercase();
  value.contains("<!--")
    || value.contains("<PREFIX")
    || lower.contains("todo")
    || lower.contains("placeholder")
}

fn validate_content_boundaries(
  openspec_root: &Path,
  workspace_root: &Path,
  diagnostics: &mut Vec<Diagnostic>,
) {
  for path in tracked_text_files(openspec_root, diagnostics) {
    let Some(contents) = read_required(&path, diagnostics) else {
      continue;
    };
    for (index, line) in contents.lines().enumerate() {
      if has_forbidden_local_reference(line) {
        diagnostics.push(Diagnostic::new(
          &path,
          index + 1,
          "tracked OpenSpec content must not depend on private `.local` paths or data",
        ));
      }
      if has_personal_absolute_path(line, workspace_root) {
        diagnostics.push(Diagnostic::new(
          &path,
          index + 1,
          "tracked OpenSpec content contains a personal or checkout-specific absolute path",
        ));
      }
    }
  }
}

fn tracked_text_files(root: &Path, diagnostics: &mut Vec<Diagnostic>) -> Vec<PathBuf> {
  if !root.exists() {
    return Vec::new();
  }
  let mut pending = vec![root.to_path_buf()];
  let mut files = Vec::new();
  while let Some(directory) = pending.pop() {
    let entries = match fs::read_dir(&directory) {
      Ok(entries) => entries,
      Err(error) => {
        diagnostics.push(Diagnostic::new(
          &directory,
          1,
          format!("cannot inspect OpenSpec content: {error}"),
        ));
        continue;
      }
    };
    for entry in entries {
      let entry = match entry {
        Ok(entry) => entry,
        Err(error) => {
          diagnostics.push(Diagnostic::new(
            &directory,
            1,
            format!("cannot inspect OpenSpec directory entry: {error}"),
          ));
          continue;
        }
      };
      let path = entry.path();
      let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(error) => {
          diagnostics.push(Diagnostic::new(
            &path,
            1,
            format!("cannot inspect OpenSpec path type: {error}"),
          ));
          continue;
        }
      };
      if file_type.is_symlink() {
        continue;
      }
      if file_type.is_dir() {
        pending.push(path);
      } else if file_type.is_file()
        && matches!(
          path.extension().and_then(|extension| extension.to_str()),
          Some("md" | "yaml" | "yml")
        )
      {
        files.push(path);
      }
    }
  }
  files.sort();
  files
}

fn has_forbidden_local_reference(line: &str) -> bool {
  if line.contains(".local/") || line.contains(".local\\") {
    return true;
  }
  if !line.replace(".localhost", "").contains(".local") {
    return false;
  }
  let lower = line.to_ascii_lowercase();
  ![
    "must not",
    "do not",
    "does not",
    "not require",
    "out of",
    "exclude",
    "forbid",
    "contains no",
    "free of",
    "without",
    "never",
  ]
  .iter()
  .any(|phrase| lower.contains(phrase))
}

fn has_personal_absolute_path(line: &str, workspace_root: &Path) -> bool {
  let normalized = line.replace('\\', "/");
  let workspace = workspace_root.to_string_lossy().replace('\\', "/");
  if normalized
    .to_ascii_lowercase()
    .contains(&workspace.to_ascii_lowercase())
  {
    return true;
  }
  if normalized.contains("//?/") {
    return true;
  }
  let lower = normalized.to_ascii_lowercase();
  if ["/users/", "/home/"]
    .iter()
    .any(|prefix| lower.contains(prefix) && !contains_portable_path_placeholder(&lower, prefix))
  {
    return true;
  }

  false
}

fn contains_portable_path_placeholder(line: &str, prefix: &str) -> bool {
  line
    .find(prefix)
    .and_then(|index| line.get(index + prefix.len()..))
    .is_some_and(|rest| rest.starts_with('<') || rest.starts_with('$') || rest.starts_with('%'))
}

fn capability_prefix(capability: &str) -> Option<&'static str> {
  CAPABILITY_PREFIXES
    .iter()
    .find_map(|(name, prefix)| (*name == capability).then_some(*prefix))
}

fn operation_name(operation: RequirementOperation) -> &'static str {
  match operation {
    RequirementOperation::Added => "ADDED",
    RequirementOperation::Modified => "MODIFIED",
    RequirementOperation::Removed => "REMOVED",
    RequirementOperation::Renamed => "RENAMED",
    RequirementOperation::Main => "main",
    RequirementOperation::Unknown => "unknown",
  }
}

fn sorted_directories(path: &Path, diagnostics: &mut Vec<Diagnostic>) -> Vec<PathBuf> {
  if !path.exists() {
    return Vec::new();
  }
  let entries = match fs::read_dir(path) {
    Ok(entries) => entries,
    Err(error) => {
      diagnostics.push(Diagnostic::new(
        path,
        1,
        format!("cannot read directory: {error}"),
      ));
      return Vec::new();
    }
  };
  let mut directories = entries
    .filter_map(|entry| match entry {
      Ok(entry) => match entry.file_type() {
        Ok(file_type) if file_type.is_dir() => Some(entry.path()),
        Ok(_) => None,
        Err(error) => {
          diagnostics.push(Diagnostic::new(
            &entry.path(),
            1,
            format!("cannot inspect directory entry: {error}"),
          ));
          None
        }
      },
      Err(error) => {
        diagnostics.push(Diagnostic::new(
          path,
          1,
          format!("cannot read directory entry: {error}"),
        ));
        None
      }
    })
    .collect::<Vec<_>>();
  directories.sort();
  directories
}

fn read_required(path: &Path, diagnostics: &mut Vec<Diagnostic>) -> Option<String> {
  match fs::read_to_string(path) {
    Ok(contents) => Some(contents),
    Err(error) => {
      diagnostics.push(Diagnostic::new(
        path,
        1,
        format!("cannot read required file: {error}"),
      ));
      None
    }
  }
}

fn file_name(path: &Path, diagnostics: &mut Vec<Diagnostic>) -> Option<String> {
  match path.file_name().and_then(|name| name.to_str()) {
    Some(name) => Some(name.to_string()),
    None => {
      diagnostics.push(Diagnostic::new(
        path,
        1,
        "path has no valid UTF-8 file name",
      ));
      None
    }
  }
}

fn display_path<'a>(root: &'a Path, path: &'a Path) -> std::borrow::Cow<'a, str> {
  path.strip_prefix(root).unwrap_or(path).to_string_lossy()
}

fn parse_spec_text(
  path: &Path,
  capability: &str,
  expected_prefix: &str,
  main_spec: bool,
  contents: &str,
) -> (Vec<ParsedRequirement>, Vec<Diagnostic>) {
  let mut requirements = Vec::new();
  let mut diagnostics = Vec::new();
  let mut operation = if main_spec {
    RequirementOperation::Main
  } else {
    RequirementOperation::Unknown
  };
  let mut pending: Option<PendingRequirement> = None;

  for (index, line) in contents.lines().enumerate() {
    let line_number = index + 1;
    if let Some(next_operation) = RequirementOperation::from_heading(line) {
      finish_requirement(&mut pending, &mut requirements, &mut diagnostics);
      operation = next_operation;
      continue;
    }

    if let Some(heading) = line.strip_prefix("### Requirement: ") {
      finish_requirement(&mut pending, &mut requirements, &mut diagnostics);
      if operation == RequirementOperation::Renamed {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          "RENAMED requirements must use `FROM:` and `TO:` pairs",
        ));
        continue;
      }
      let Some((id, title)) = heading.split_once(": ") else {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          "requirement heading must be `### Requirement: ABC-001: Title`",
        ));
        continue;
      };

      if !is_requirement_id(id) {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          format!("invalid requirement ID `{id}`"),
        ));
        continue;
      }
      if !id.starts_with(expected_prefix) {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          format!("requirement ID `{id}` does not use capability prefix `{expected_prefix}`"),
        ));
      }
      if title.trim().is_empty() {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          format!("requirement `{id}` has an empty title"),
        ));
      }
      if operation == RequirementOperation::Unknown {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          format!("requirement `{id}` is outside a delta operation section"),
        ));
      }

      pending = Some(PendingRequirement {
        parsed: ParsedRequirement {
          id: id.to_string(),
          capability: capability.to_string(),
          operation,
          path: path.to_path_buf(),
          line: line_number,
        },
        has_normative_text: false,
        scenario_count: 0,
      });
      continue;
    }

    let Some(requirement) = pending.as_mut() else {
      continue;
    };
    if line.starts_with("#### Scenario: ") {
      requirement.scenario_count += 1;
    }
    if line
      .split(|character: char| !character.is_ascii_alphabetic())
      .any(|word| matches!(word, "SHALL" | "MUST"))
    {
      requirement.has_normative_text = true;
    }
  }

  finish_requirement(&mut pending, &mut requirements, &mut diagnostics);
  if !main_spec {
    let (mut renamed, mut rename_diagnostics) =
      parse_renamed_requirements(path, capability, expected_prefix, contents);
    requirements.append(&mut renamed);
    diagnostics.append(&mut rename_diagnostics);
  }
  (requirements, diagnostics)
}

fn parse_renamed_requirements(
  path: &Path,
  capability: &str,
  expected_prefix: &str,
  contents: &str,
) -> (Vec<ParsedRequirement>, Vec<Diagnostic>) {
  let mut requirements = Vec::new();
  let mut diagnostics = Vec::new();
  let mut in_section = false;
  let mut from: Option<(String, String, usize)> = None;

  for (index, line) in contents.lines().enumerate() {
    let line_number = index + 1;
    if line.starts_with("## ") {
      if in_section && let Some((id, _, from_line)) = from.take() {
        diagnostics.push(Diagnostic::new(
          path,
          from_line,
          format!("RENAMED requirement `{id}` is missing its `TO:` pair"),
        ));
      }
      in_section = line == "## RENAMED Requirements";
      continue;
    }
    if !in_section {
      continue;
    }
    if let Some(value) = rename_value(line, "FROM") {
      if let Some((id, _, from_line)) = from.take() {
        diagnostics.push(Diagnostic::new(
          path,
          from_line,
          format!("RENAMED requirement `{id}` is missing its `TO:` pair"),
        ));
      }
      if let Some((id, title)) =
        parse_renamed_heading(path, line_number, value, expected_prefix, &mut diagnostics)
      {
        from = Some((id, title, line_number));
      }
      continue;
    }
    if let Some(value) = rename_value(line, "TO") {
      let Some((from_id, from_title, from_line)) = from.take() else {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          "RENAMED `TO:` entry must follow a `FROM:` entry",
        ));
        continue;
      };
      let Some((to_id, to_title)) =
        parse_renamed_heading(path, line_number, value, expected_prefix, &mut diagnostics)
      else {
        continue;
      };
      if from_id != to_id {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          format!("RENAMED requirement must retain stable ID `{from_id}`, not `{to_id}`"),
        ));
        continue;
      }
      if from_title == to_title {
        diagnostics.push(Diagnostic::new(
          path,
          line_number,
          format!("RENAMED requirement `{from_id}` does not change its title"),
        ));
      }
      requirements.push(ParsedRequirement {
        id: from_id,
        capability: capability.to_string(),
        operation: RequirementOperation::Renamed,
        path: path.to_path_buf(),
        line: from_line,
      });
    }
  }

  if let Some((id, _, from_line)) = from {
    diagnostics.push(Diagnostic::new(
      path,
      from_line,
      format!("RENAMED requirement `{id}` is missing its `TO:` pair"),
    ));
  }
  (requirements, diagnostics)
}

fn rename_value<'a>(line: &'a str, label: &str) -> Option<&'a str> {
  line
    .strip_prefix(&format!("- {label}: `### Requirement: "))
    .and_then(|value| value.strip_suffix('`'))
}

fn parse_renamed_heading(
  path: &Path,
  line: usize,
  heading: &str,
  expected_prefix: &str,
  diagnostics: &mut Vec<Diagnostic>,
) -> Option<(String, String)> {
  let Some((id, title)) = heading.split_once(": ") else {
    diagnostics.push(Diagnostic::new(
      path,
      line,
      "RENAMED heading must be `ABC-001: Title`",
    ));
    return None;
  };
  if !is_requirement_id(id) || !id.starts_with(expected_prefix) {
    diagnostics.push(Diagnostic::new(
      path,
      line,
      format!("invalid RENAMED requirement ID `{id}` for prefix `{expected_prefix}`"),
    ));
    return None;
  }
  if title.trim().is_empty() {
    diagnostics.push(Diagnostic::new(
      path,
      line,
      format!("RENAMED requirement `{id}` has an empty title"),
    ));
    return None;
  }
  Some((id.to_string(), title.to_string()))
}

fn finish_requirement(
  pending: &mut Option<PendingRequirement>,
  requirements: &mut Vec<ParsedRequirement>,
  diagnostics: &mut Vec<Diagnostic>,
) {
  let Some(pending) = pending.take() else {
    return;
  };

  if pending.parsed.operation.needs_normative_text() && !pending.has_normative_text {
    diagnostics.push(Diagnostic::new(
      &pending.parsed.path,
      pending.parsed.line,
      format!(
        "requirement `{}` must contain SHALL or MUST",
        pending.parsed.id
      ),
    ));
  }
  if pending.parsed.operation.needs_scenario() && pending.scenario_count == 0 {
    diagnostics.push(Diagnostic::new(
      &pending.parsed.path,
      pending.parsed.line,
      format!(
        "requirement `{}` must contain at least one scenario",
        pending.parsed.id
      ),
    ));
  }
  requirements.push(pending.parsed);
}

fn is_requirement_id(value: &str) -> bool {
  let bytes = value.as_bytes();
  bytes.len() == 7
    && bytes[..3].iter().all(u8::is_ascii_uppercase)
    && bytes[3] == b'-'
    && bytes[4..].iter().all(u8::is_ascii_digit)
    && &bytes[4..] != b"000"
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::tempdir;

  #[test]
  fn parser_accepts_valid_main_requirement() {
    let (requirements, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "local-control-plane",
      "IPC-",
      true,
      "### Requirement: IPC-001: Owner access\nCadder MUST authenticate the owner.\n\n#### Scenario: Owner connects\n- **WHEN** the owner connects\n- **THEN** access is granted\n",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(requirements.len(), 1);
    assert_eq!(requirements[0].id, "IPC-001");
    assert_eq!(requirements[0].operation, RequirementOperation::Main);
  }

  #[test]
  fn parser_accepts_valid_added_requirement() {
    let (requirements, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "caddy-runtime",
      "CAD-",
      false,
      "## ADDED Requirements\n\n### Requirement: CAD-001: Trusted executable\nCadder SHALL use a trusted executable.\n\n#### Scenario: Trusted path\n- **WHEN** resolution succeeds\n- **THEN** the path is pinned\n",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(requirements[0].operation, RequirementOperation::Added);
  }

  #[test]
  fn parser_reports_wrong_prefix() {
    let (_, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "caddy-runtime",
      "CAD-",
      true,
      "### Requirement: IPC-001: Wrong prefix\nCadder MUST reject it.\n\n#### Scenario: Invalid\n- **WHEN** parsed\n- **THEN** validation fails\n",
    );

    assert!(
      diagnostics[0]
        .message
        .contains("does not use capability prefix")
    );
  }

  #[test]
  fn parser_reports_missing_normative_text_and_scenario() {
    let (_, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "runtime-storage",
      "STO-",
      true,
      "### Requirement: STO-001: Durable state\nThe database stores state.\n",
    );

    assert_eq!(diagnostics.len(), 2);
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("SHALL or MUST"))
    );
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("at least one scenario"))
    );
  }

  #[test]
  fn parser_accepts_removed_requirements_without_scenarios() {
    let (requirements, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "caddy-runtime",
      "CAD-",
      false,
      "## REMOVED Requirements\n\n### Requirement: CAD-001: Retired contract\n\n**Reason**: The contract is replaced.\n\n**Migration**: Use CAD-002.\n",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(requirements[0].operation, RequirementOperation::Removed);
  }

  #[test]
  fn parser_accepts_renames_that_retain_the_stable_id() {
    let (requirements, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "caddy-runtime",
      "CAD-",
      false,
      "## RENAMED Requirements\n\n- FROM: `### Requirement: CAD-001: Old title`\n- TO: `### Requirement: CAD-001: New title`\n",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(requirements[0].id, "CAD-001");
    assert_eq!(requirements[0].operation, RequirementOperation::Renamed);
  }

  #[test]
  fn requirement_id_rejects_zero_sequence_and_lowercase_text() {
    assert!(!is_requirement_id("IPC-000"));
    assert!(!is_requirement_id("ipc-001"));
    assert!(is_requirement_id("IPC-001"));
    assert!(extract_known_requirement_ids("XIPC-001Y").is_empty());
    assert!(extract_known_requirement_ids("xIPC-001y").is_empty());
    assert!(extract_task_requirement_ids("Implement IPC-001").is_empty());
    assert_eq!(
      extract_task_requirement_ids("Implement [IPC-001]"),
      BTreeSet::from(["IPC-001".to_string()])
    );
  }

  #[test]
  fn repository_accepts_a_valid_contract_change() {
    let dir = tempdir().unwrap();
    write_main_specs_except(dir.path(), "caddy-runtime");
    write_file(
      dir.path(),
      "openspec/changes/add-runtime/.openspec.yaml",
      "schema: spec-driven\n",
    );
    write_file(
      dir.path(),
      "openspec/changes/add-runtime/proposal.md",
      "## Capabilities\n\n### New Capabilities\n\n- `caddy-runtime`: Runtime contract.\n\n### Modified Capabilities\n\nNone.\n",
    );
    write_file(
      dir.path(),
      "openspec/changes/add-runtime/specs/caddy-runtime/spec.md",
      valid_delta_spec("CAD-001"),
    );

    validate_repository(dir.path()).unwrap();
  }

  #[test]
  fn repository_reports_added_collisions_and_dangling_modifications() {
    let dir = tempdir().unwrap();
    write_complete_main_specs(dir.path());
    write_file(
      dir.path(),
      "openspec/changes/change-runtime/.openspec.yaml",
      "schema: spec-driven\n",
    );
    write_file(
      dir.path(),
      "openspec/changes/change-runtime/proposal.md",
      "## Capabilities\n\n### New capabilities\n\n- `caddy-runtime`: Runtime contract.\n",
    );
    write_file(
      dir.path(),
      "openspec/changes/change-runtime/specs/caddy-runtime/spec.md",
      "## ADDED Requirements\n\n### Requirement: CAD-001: Duplicate\nCadder MUST reject duplicates.\n\n#### Scenario: Duplicate\n- **WHEN** validation runs\n- **THEN** it fails\n\n## MODIFIED Requirements\n\n### Requirement: CAD-002: Missing\nCadder MUST reject dangling changes.\n\n#### Scenario: Missing\n- **WHEN** validation runs\n- **THEN** it fails\n",
    );

    let error = validate_repository(dir.path()).unwrap_err().to_string();
    assert!(error.contains("already exists in main specs"), "{error}");
    assert!(error.contains("does not exist in main specs"), "{error}");
  }

  #[test]
  fn implementation_verification_follows_completed_tasks() {
    let dir = tempdir().unwrap();
    write_complete_main_specs(dir.path());
    let change = "openspec/changes/secure-ipc";
    write_file(
      dir.path(),
      &format!("{change}/.openspec.yaml"),
      "schema: implementation\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/proposal.md"),
      "## Requirement IDs\n\n- `IPC-001`: Owner authentication\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/design.md"),
      "## Contract\n\n`IPC-001` uses peer identity.\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/tasks.md"),
      "## 1. Implementation\n\n- [ ] 1.1 `[IPC-001]` Authenticate peers. (verification: focused peer-auth test)\n",
    );
    validate_repository(dir.path()).unwrap();

    write_file(
      dir.path(),
      &format!("{change}/verification.md"),
      valid_verification(),
    );
    let early = validate_repository(dir.path()).unwrap_err().to_string();
    assert!(early.contains("must not exist before"), "{early}");

    write_file(
      dir.path(),
      &format!("{change}/tasks.md"),
      "## 1. Implementation\n\n- [x] 1.1 `[IPC-001]` Authenticate peers. (verification: focused peer-auth test)\n",
    );
    validate_repository(dir.path()).unwrap();
  }

  #[test]
  fn implementation_reports_task_scope_and_evidence_errors() {
    let dir = tempdir().unwrap();
    write_complete_main_specs(dir.path());
    let change = "openspec/changes/secure-ipc";
    write_file(
      dir.path(),
      &format!("{change}/.openspec.yaml"),
      "schema: implementation\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/proposal.md"),
      "## Requirement IDs\n\n- `IPC-001`: Owner authentication\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/design.md"),
      "`IPC-001` uses peer identity.\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/tasks.md"),
      "- [x] 1.1 `[CAD-001]` Wrong scope. (verification: TODO)\n- [x] 1.1 Missing ID. (verification: focused test)\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/verification.md"),
      valid_verification(),
    );

    let error = validate_repository(dir.path()).unwrap_err().to_string();
    assert!(
      error.contains("out-of-scope requirement `CAD-001`"),
      "{error}"
    );
    assert!(error.contains("task number `1.1` is duplicated"), "{error}");
    assert!(error.contains("must reference at least one"), "{error}");
    assert!(error.contains("must name a concrete"), "{error}");
  }

  #[test]
  fn content_boundaries_allow_policy_text_and_reject_private_paths() {
    let root = Path::new("D:/Projects/Personal/Cadder");
    assert!(!has_forbidden_local_reference(
      "Pages MUST NOT expose `.local` data."
    ));
    assert!(has_forbidden_local_reference("Read `.local/notes.md`."));
    assert!(has_forbidden_local_reference("Keep using `.local` notes."));
    assert!(!has_forbidden_local_reference("Use app.localhost."));
    assert!(has_personal_absolute_path(
      "Use D:\\Projects\\Personal\\Cadder\\target.",
      root
    ));
    assert!(has_personal_absolute_path("Use /home/alex/cadder.", root));
    assert!(!has_personal_absolute_path(
      "Install to C:\\Program Files\\Cadder.",
      root
    ));
    assert!(!has_personal_absolute_path(
      "Install to D:\\Apps\\Cadder.",
      root
    ));
  }

  #[test]
  fn change_classification_skips_archive_and_rejects_duplicate_schema_fields() {
    let dir = tempdir().unwrap();
    write_file(
      dir.path(),
      "openspec/changes/current/.openspec.yaml",
      "schema: spec-driven\n",
    );
    write_file(
      dir.path(),
      "openspec/changes/archive/old/.openspec.yaml",
      "schema: spec-driven\n",
    );
    assert_eq!(spec_driven_change_names(dir.path()).unwrap(), ["current"]);

    write_file(
      dir.path(),
      "openspec/changes/current/.openspec.yaml",
      "schema: spec-driven\nschema: implementation\n",
    );
    let error = spec_driven_change_names(dir.path())
      .unwrap_err()
      .to_string();
    assert!(error.contains("exactly one"), "{error}");
  }

  #[test]
  fn repository_requires_every_accepted_capability() {
    let dir = tempdir().unwrap();
    write_complete_main_specs(dir.path());
    fs::remove_file(
      dir
        .path()
        .join("openspec/specs/documentation-experience/spec.md"),
    )
    .unwrap();

    let error = validate_repository(dir.path()).unwrap_err().to_string();
    assert!(
      error.contains("missing capability `documentation-experience`"),
      "{error}"
    );
  }

  #[test]
  fn repository_rejects_generated_main_spec_purpose_placeholders() {
    let dir = tempdir().unwrap();
    write_complete_main_specs(dir.path());
    write_file(
      dir.path(),
      "openspec/specs/caddy-runtime/spec.md",
      "# caddy-runtime Specification\n\n## Purpose\nTBD - created by archiving change add-runtime. Update Purpose after archive.\n\n## Requirements\n\n### Requirement: CAD-001: Accepted contract\nCadder MUST satisfy the contract.\n\n#### Scenario: Contract holds\n- **WHEN** Cadder operates\n- **THEN** the contract holds\n",
    );

    let error = validate_repository(dir.path()).unwrap_err().to_string();
    assert!(
      error.contains("concrete product-level description"),
      "{error}"
    );
  }

  #[test]
  fn verification_requires_the_evidence_section_and_every_named_gate() {
    let path = Path::new("verification.md");
    let requirements = BTreeSet::from(["IPC-001".to_string()]);
    let tasks = [ImplementationTask {
      number: "1.1".to_string(),
      complete: true,
      requirement_ids: requirements.clone(),
    }];
    let mut diagnostics = Vec::new();
    validate_verification(
      path,
      "## Notes\n\n| `IPC-001` | `1.1` | `cargo test` | Pass |\n\n## Gate\n\n- [x Focused tests pass.\n",
      &requirements,
      &tasks,
      &mut diagnostics,
    );

    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("`## Evidence`")),
      "{diagnostics:?}"
    );
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("exact `- [x] Description`")),
      "{diagnostics:?}"
    );
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("missing completed gate")),
      "{diagnostics:?}"
    );
  }

  fn write_main_spec(root: &Path, capability: &str, id: &str) {
    write_file(
      root,
      &format!("openspec/specs/{capability}/spec.md"),
      format!(
        "# {capability} Specification\n\n## Purpose\nDefine the accepted Cadder behavior for the {capability} capability and its observable product boundaries.\n\n## Requirements\n\n### Requirement: {id}: Accepted contract\nCadder MUST satisfy the contract.\n\n#### Scenario: Contract holds\n- **WHEN** Cadder operates\n- **THEN** the contract holds\n"
      ),
    );
  }

  fn write_complete_main_specs(root: &Path) {
    write_main_specs_except(root, "");
  }

  fn write_main_specs_except(root: &Path, excluded: &str) {
    for (capability, prefix) in CAPABILITY_PREFIXES {
      if capability != excluded {
        write_main_spec(root, capability, &format!("{prefix}001"));
      }
    }
  }

  fn valid_delta_spec(id: &str) -> String {
    format!(
      "## ADDED Requirements\n\n### Requirement: {id}: New contract\nCadder MUST satisfy the contract.\n\n#### Scenario: Contract holds\n- **WHEN** Cadder operates\n- **THEN** the contract holds\n"
    )
  }

  fn valid_verification() -> &'static str {
    "## Evidence\n\n| Requirement ID | Task | Evidence | Result |\n| --- | --- | --- | --- |\n| `IPC-001` | `1.1` | `cargo test peer_auth` | Pass |\n\n## Gate\n\n- [x] Every task in `tasks.md` is complete.\n- [x] Every in-scope requirement ID has passing evidence.\n- [x] Focused tests pass.\n- [x] Repository checks required by the design pass.\n- [x] Documentation describes only verified behavior.\n- [x] `cargo xtask openspec-check` passes.\n"
  }

  fn write_file(root: &Path, relative: &str, contents: impl AsRef<[u8]>) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
  }
}
