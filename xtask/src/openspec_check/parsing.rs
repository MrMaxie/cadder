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
