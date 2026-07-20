fn check() -> Result<()> {
  openspec_check_command()?;
  verify_workspace_topology()?;
  verify_release_profile()?;
  verify_release_identity()?;
  run("cargo", &["fmt", "--check"])?;
  run(
    "cargo",
    &[
      "clippy",
      "--workspace",
      "--all-targets",
      "--",
      "-D",
      "warnings",
    ],
  )?;
  run("cargo", &workspace_test_args())?;
  docs_check()?;
  Ok(())
}

fn openspec_check_command() -> Result<()> {
  let root = workspace_root();
  let openspec_path = resolve_openspec_program()?;
  let openspec = openspec_path
    .to_str()
    .context("OpenSpec executable path is not valid UTF-8")?;
  verify_openspec_version(&root, openspec)?;
  run_in(openspec, &["doctor", "--json"], &root)?;
  run_in(
    openspec,
    &["schema", "validate", "implementation", "--json"],
    &root,
  )?;
  run_in(
    openspec,
    &["validate", "--specs", "--strict", "--json"],
    &root,
  )?;
  for change in openspec_check::spec_driven_change_names(&root)? {
    run_in(
      openspec,
      &["validate", &change, "--strict", "--json"],
      &root,
    )?;
  }
  openspec_check::validate_repository(&root)?;
  println!("validated OpenSpec repository contract");
  Ok(())
}

fn verify_openspec_version(root: &Path, openspec: &str) -> Result<()> {
  let mut command = Command::new(openspec);
  configure_hidden_child(&mut command);
  let output = command
    .arg("--version")
    .current_dir(root)
    .output()
    .context("run openspec --version")?;
  if !output.status.success() {
    bail!("openspec --version failed with {}", output.status);
  }
  let version = String::from_utf8(output.stdout)
    .context("parse openspec --version output as UTF-8")?
    .trim()
    .to_string();
  if version != EXPECTED_OPENSPEC_VERSION {
    bail!("OpenSpec {EXPECTED_OPENSPEC_VERSION} is required, but `{version}` is installed");
  }
  Ok(())
}

fn resolve_openspec_program() -> Result<PathBuf> {
  let path = env::var_os("PATH").context("PATH is not defined")?;
  let names: &[&str] = if cfg!(windows) {
    &["openspec.exe", "openspec.cmd", "openspec.bat", "openspec"]
  } else {
    &["openspec"]
  };
  for directory in env::split_paths(&path) {
    for name in names {
      let candidate = directory.join(name);
      if candidate.is_file() {
        return Ok(candidate);
      }
    }
  }
  bail!(
    "OpenSpec {EXPECTED_OPENSPEC_VERSION} is required, but no `openspec` executable was found on PATH"
  )
}

fn docs_check() -> Result<()> {
  run_docs_script("check")
}

fn docs_build() -> Result<()> {
  run_docs_script("build")
}

fn run_docs_script(script: &str) -> Result<()> {
  let docs_dir = docs_site_dir();
  run_in("bun", &["install", "--frozen-lockfile"], &docs_dir)?;
  run_in("bun", &["run", script], &docs_dir)
}

fn coverage(options: CoverageOptions) -> Result<()> {
  ensure_parent_dir(&options.output_path)?;
  let args = coverage_command_args(&options.output_path)?;
  let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
  run("cargo", &arg_refs)?;
  enforce_lcov_line_threshold(&options.output_path, COVERAGE_FAIL_UNDER_LINES)
}

fn dev_run(args: Vec<String>) -> Result<()> {
  let args = strip_leading_separator(args);
  let Some((program, program_args)) = args.split_first() else {
    bail!("dev-run requires a program after `--`");
  };
  let mut command = Command::new(program);
  configure_hidden_child(&mut command);
  command
    .args(program_args)
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit());
  DevEnvironment::for_workspace().apply_to(&mut command);
  let status = command
    .status()
    .with_context(|| format!("run dev command {program} {}", program_args.join(" ")))?;
  if status.success() {
    Ok(())
  } else {
    bail!(
      "dev command {program} {} failed with {status}",
      program_args.join(" ")
    )
  }
}

fn strip_leading_separator(mut args: Vec<String>) -> Vec<String> {
  if args.first().is_some_and(|arg| arg == "--") {
    args.remove(0);
  }
  args
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum DevEnvFormat {
  #[value(alias = "pwsh")]
  Powershell,
  #[value(alias = "sh")]
  Bash,
  Cmd,
  Json,
}

#[derive(Debug, Clone)]
struct DevEnvironment {
  values: Vec<(&'static str, String)>,
}

impl DevEnvironment {
  fn for_workspace() -> Self {
    Self {
      values: vec![(CADDER_CADDY_BACKEND_ENV, "mock".to_string())],
    }
  }

  fn apply_to(&self, command: &mut Command) {
    for (key, value) in &self.values {
      command.env(key, value);
    }
  }

  fn print(&self, format: DevEnvFormat) -> Result<()> {
    match format {
      DevEnvFormat::Powershell => {
        for (key, value) in &self.values {
          println!("$env:{key} = '{}'", escape_powershell_single_quoted(value));
        }
      }
      DevEnvFormat::Bash => {
        for (key, value) in &self.values {
          println!("export {key}='{}'", escape_bash_single_quoted(value));
        }
      }
      DevEnvFormat::Cmd => {
        for (key, value) in &self.values {
          println!("set {key}={value}");
        }
      }
      DevEnvFormat::Json => {
        let object = self
          .values
          .iter()
          .map(|(key, value)| ((*key).to_string(), json!(value)))
          .collect::<serde_json::Map<_, _>>();
        println!(
          "{}",
          serde_json::to_string_pretty(&JsonValue::Object(object))?
        );
      }
    }
    Ok(())
  }
}

fn escape_powershell_single_quoted(value: &str) -> String {
  value.replace('\'', "''")
}

fn escape_bash_single_quoted(value: &str) -> String {
  value.replace('\'', "'\\''")
}

fn docs_site_dir() -> PathBuf {
  workspace_root().join(DOCS_SITE_DIR)
}

fn workspace_test_args() -> [&'static str; 2] {
  ["test", "--workspace"]
}

fn workspace_root() -> PathBuf {
  Path::new(WORKSPACE_MANIFEST)
    .parent()
    .expect("workspace manifest has a parent")
    .to_path_buf()
}
