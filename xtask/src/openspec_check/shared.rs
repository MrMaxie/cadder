fn validate_shared_artifact_rules(openspec_root: &Path, diagnostics: &mut Vec<Diagnostic>) {
  const SHARED_ARTIFACTS: [&str; 3] = ["proposal", "design", "tasks"];

  let path = openspec_root.join("config.yaml");
  let Ok(contents) = fs::read_to_string(&path) else {
    return;
  };
  let mut in_rules = false;
  for (index, line) in contents.lines().enumerate() {
    if line == "rules:" {
      in_rules = true;
      continue;
    }
    if in_rules && !line.is_empty() && !line.starts_with(' ') {
      in_rules = false;
    }
    if !in_rules || !line.starts_with("  ") || line.starts_with("    ") {
      continue;
    }
    let Some(artifact) = line.trim().strip_suffix(':') else {
      continue;
    };
    if !SHARED_ARTIFACTS.contains(&artifact) {
      diagnostics.push(Diagnostic::new(
        &path,
        index + 1,
        format!(
          "global artifact rule `{artifact}` is not shared by the spec-driven and implementation schemas; put the instruction in project context or a schema template"
        ),
      ));
    }
  }
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
