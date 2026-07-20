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
