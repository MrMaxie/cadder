fn run(program: &str, args: &[&str]) -> Result<()> {
  let mut command = Command::new(program);
  configure_hidden_child(&mut command);
  let status = command
    .args(args)
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit())
    .status()
    .with_context(|| format!("run {program} {}", args.join(" ")))?;
  if status.success() {
    Ok(())
  } else {
    bail!("{program} {} failed with {status}", args.join(" "))
  }
}

fn run_in(program: &str, args: &[&str], current_dir: &Path) -> Result<()> {
  run_in_with_env(program, args, current_dir, &[])
}

fn run_in_with_env(
  program: &str,
  args: &[&str],
  current_dir: &Path,
  envs: &[(&str, &Path)],
) -> Result<()> {
  let mut command = Command::new(program);
  configure_hidden_child(&mut command);
  command
    .args(args)
    .current_dir(current_dir)
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit());
  for (key, value) in envs {
    command.env(key, value);
  }
  let status = command.status().with_context(|| {
    format!(
      "run {program} {} in {}",
      args.join(" "),
      current_dir.display()
    )
  })?;
  if status.success() {
    Ok(())
  } else {
    bail!("{program} {} failed with {status}", args.join(" "))
  }
}

#[cfg(windows)]
fn configure_hidden_child(command: &mut Command) {
  command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure_hidden_child(_command: &mut Command) {}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
  if path.exists() {
    fs::remove_dir_all(path).with_context(|| format!("remove {}", path.display()))?;
  }
  Ok(())
}

#[cfg(test)]
fn replace_dir_from(source: &Path, target: &Path) -> Result<()> {
  remove_dir_if_exists(target)?;
  copy_dir_contents(source, target)
}

fn copy_dir_contents(source: &Path, target: &Path) -> Result<()> {
  fs::create_dir_all(target).with_context(|| format!("create {}", target.display()))?;
  for entry in sorted_dir_entries(source)? {
    let source_path = entry.path();
    let target_path = target.join(entry.file_name());
    if source_path.is_dir() {
      copy_dir_contents(&source_path, &target_path)?;
    } else {
      fs::copy(&source_path, &target_path).with_context(|| {
        format!(
          "copy {} to {}",
          source_path.display(),
          target_path.display()
        )
      })?;
    }
  }
  Ok(())
}

fn is_macos_app_bundle(path: &Path) -> bool {
  path
    .file_name()
    .and_then(|name| name.to_str())
    .is_some_and(|name| name.to_ascii_lowercase().ends_with(".app"))
}
