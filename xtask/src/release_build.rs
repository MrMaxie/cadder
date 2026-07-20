fn release_binary_path(name: &str, target: Option<&str>) -> PathBuf {
  let release_dir = match target {
    Some(target) => PathBuf::from("target").join(target).join("release"),
    None => PathBuf::from("target").join("release"),
  };
  release_dir.join(exe_name(name, target))
}

fn exe_name(name: &str, target: Option<&str>) -> String {
  if target_uses_windows_executables(target) {
    format!("{name}.exe")
  } else {
    name.to_string()
  }
}

fn target_uses_windows_executables(target: Option<&str>) -> bool {
  target.map_or(cfg!(windows), |target| target.contains("windows"))
}

fn build_release_binaries(target: Option<&str>, topology: PortableTopology) -> Result<()> {
  let mut args = vec!["build", "--release"];
  for package in topology.release_packages() {
    args.push("-p");
    args.push(package);
  }
  if let Some(target) = target {
    args.push("--target");
    args.push(target);
  }
  run("cargo", &args)
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
  if let Some(parent) = path
    .parent()
    .filter(|parent| !parent.as_os_str().is_empty())
  {
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
  }
  Ok(())
}

fn coverage_command_args(output_path: &Path) -> Result<Vec<String>> {
  let output_path = output_path.to_str().with_context(|| {
    format!(
      "coverage output path is not UTF-8: {}",
      output_path.display()
    )
  })?;

  let mut args = Vec::new();
  if let Some(toolchain) = coverage_toolchain_argument_from_env()? {
    args.push(toolchain);
  }
  args.extend(["llvm-cov".to_string(), "--workspace".to_string()]);
  for package in COVERAGE_EXCLUDED_PACKAGES {
    args.push("--exclude".to_string());
    args.push(package.to_string());
  }
  if !COVERAGE_IGNORED_FILENAME_REGEX.is_empty() {
    args.push("--ignore-filename-regex".to_string());
    args.push(COVERAGE_IGNORED_FILENAME_REGEX.to_string());
  }
  args.extend([
    "--lcov".to_string(),
    "--output-path".to_string(),
    output_path.to_string(),
  ]);
  Ok(args)
}

fn coverage_toolchain_argument_from_env() -> Result<Option<String>> {
  let override_toolchain = env::var(CADDER_COVERAGE_TOOLCHAIN_ENV).ok();
  let active_toolchain = env::var("RUSTUP_TOOLCHAIN").ok();
  coverage_toolchain_argument(
    override_toolchain.as_deref(),
    active_toolchain.as_deref(),
    cfg!(windows),
    cfg!(all(windows, target_env = "gnu")),
  )
}

fn coverage_toolchain_argument(
  override_toolchain: Option<&str>,
  active_toolchain: Option<&str>,
  is_windows: bool,
  is_windows_gnu_host: bool,
) -> Result<Option<String>> {
  if let Some(toolchain) = override_toolchain.and_then(non_empty_str) {
    return normalize_rustup_toolchain_arg(toolchain);
  }

  if is_windows {
    let active_toolchain = active_toolchain.and_then(non_empty_str);
    if windows_coverage_needs_msvc_fallback(active_toolchain, is_windows_gnu_host) {
      return normalize_rustup_toolchain_arg(WINDOWS_COVERAGE_TOOLCHAIN);
    }
    return Ok(None);
  }

  if active_toolchain.and_then(non_empty_str).is_some() {
    return Ok(None);
  }

  Ok(None)
}

fn windows_coverage_needs_msvc_fallback(
  active_toolchain: Option<&str>,
  is_windows_gnu_host: bool,
) -> bool {
  let Some(active_toolchain) = active_toolchain else {
    return true;
  };
  if active_toolchain.contains("windows-msvc") {
    return false;
  }
  active_toolchain.contains("windows-gnu") || is_windows_gnu_host
}

fn normalize_rustup_toolchain_arg(toolchain: &str) -> Result<Option<String>> {
  let Some(trimmed) = non_empty_str(toolchain) else {
    return Ok(None);
  };
  let trimmed = trimmed.strip_prefix('+').unwrap_or(trimmed).trim();
  if trimmed.is_empty() {
    bail!("coverage toolchain must not be only `+`");
  }
  Ok(Some(format!("+{trimmed}")))
}

fn non_empty_str(value: &str) -> Option<&str> {
  let trimmed = value.trim();
  (!trimmed.is_empty()).then_some(trimmed)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LcovLineCoverage {
  covered: u64,
  total: u64,
}

impl LcovLineCoverage {
  fn percent(self) -> f64 {
    if self.total == 0 {
      0.0
    } else {
      self.covered as f64 * 100.0 / self.total as f64
    }
  }
}

fn enforce_lcov_line_threshold(report_path: &Path, required_percent: f64) -> Result<()> {
  let coverage = read_lcov_line_coverage(report_path)?;
  let percent = coverage.percent();
  if percent < required_percent {
    bail!(
      "line coverage {:.2}% is below required {:.2}% ({}/{} lines covered)",
      percent,
      required_percent,
      coverage.covered,
      coverage.total
    );
  }

  println!(
    "line coverage {:.2}% meets required {:.2}% ({}/{} lines covered)",
    percent, required_percent, coverage.covered, coverage.total
  );
  Ok(())
}

fn read_lcov_line_coverage(report_path: &Path) -> Result<LcovLineCoverage> {
  let report = fs::read_to_string(report_path)
    .with_context(|| format!("read coverage report {}", report_path.display()))?;
  let mut current_file = None;
  let mut line_counts = BTreeMap::<(String, u64), u64>::new();

  for (index, line) in report.lines().enumerate() {
    let line_number = index + 1;
    if let Some(file) = line.strip_prefix("SF:") {
      current_file = Some(file.to_string());
    } else if let Some(value) = line.strip_prefix("DA:") {
      let file = current_file.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
          "DA record appeared before SF on line {line_number} in {}",
          report_path.display()
        )
      })?;
      let (source_line, execution_count) = parse_lcov_da(report_path, line_number, value)?;
      line_counts
        .entry((file.clone(), source_line))
        .and_modify(|count| *count = count.saturating_add(execution_count))
        .or_insert(execution_count);
    } else if line == "end_of_record" {
      current_file = None;
    }
  }

  let coverage = LcovLineCoverage {
    covered: line_counts.values().filter(|count| **count > 0).count() as u64,
    total: line_counts.len() as u64,
  };
  if coverage.total == 0 {
    bail!(
      "coverage report {} did not contain any DA line records",
      report_path.display()
    );
  }
  if coverage.covered > coverage.total {
    bail!(
      "coverage report {} has more covered lines than total lines ({}/{})",
      report_path.display(),
      coverage.covered,
      coverage.total
    );
  }

  Ok(coverage)
}

fn parse_lcov_da(report_path: &Path, line_number: usize, value: &str) -> Result<(u64, u64)> {
  let mut parts = value.split(',');
  let source_line = parts
    .next()
    .filter(|value| !value.trim().is_empty())
    .context("missing DA source line")?
    .trim()
    .parse::<u64>()
    .with_context(|| {
      format!(
        "parse DA source line on line {line_number} in {}",
        report_path.display()
      )
    })?;
  let execution_count = parts
    .next()
    .filter(|value| !value.trim().is_empty())
    .context("missing DA execution count")?
    .trim()
    .parse::<u64>()
    .with_context(|| {
      format!(
        "parse DA execution count on line {line_number} in {}",
        report_path.display()
      )
    })?;
  Ok((source_line, execution_count))
}
