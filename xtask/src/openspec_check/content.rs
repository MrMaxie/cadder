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
