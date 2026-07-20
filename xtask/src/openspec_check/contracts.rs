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
