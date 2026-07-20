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
