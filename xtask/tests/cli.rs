use std::{
  env, fs,
  path::{Path, PathBuf},
  process::{Command, Output},
};

#[test]
fn verify_dist_command_accepts_fake_portable_layout() {
  let dir = unique_temp_dir("verify-dist");
  fs::create_dir_all(&dir).unwrap();
  write_fake_portable_executable_layout(&dir, runtime_portable_binaries(), true);

  let output = run_xtask(["verify-dist", "--dir"], Some(&dir));

  assert_success(output);
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn help_and_list_commands_are_lightweight_and_discover_docs_release_and_dev_tasks() {
  let help = Command::new(xtask_bin()).arg("--help").output().unwrap();
  let help_stdout = help.stdout.clone();
  assert_success(help);
  let help_text = String::from_utf8_lossy(&help_stdout);
  assert!(help_text.contains("cargo xtask <command>"), "{help_text}");
  assert!(help_text.contains("documentation:"), "{help_text}");
  assert!(help_text.contains("release and packaging:"), "{help_text}");
  assert!(help_text.contains("docs-check"), "{help_text}");
  assert!(help_text.contains("verify-assets"), "{help_text}");
  assert!(help_text.contains("verify-release-assets"), "{help_text}");
  assert!(
    help_text.contains("verify-workspace-topology"),
    "{help_text}"
  );
  assert!(help_text.contains("dev-run"), "{help_text}");

  let list = Command::new(xtask_bin()).arg("list").output().unwrap();
  let list_stdout = list.stdout.clone();
  assert_success(list);
  let list_text = String::from_utf8_lossy(&list_stdout);
  assert!(list_text.lines().any(|line| line == "check"), "{list_text}");
  assert!(
    list_text.lines().any(|line| line == "docs-build"),
    "{list_text}"
  );
  assert!(
    list_text.lines().any(|line| line == "runtime-installer"),
    "{list_text}"
  );
  assert!(
    list_text.lines().any(|line| line == "verify-assets"),
    "{list_text}"
  );
  assert!(
    list_text
      .lines()
      .any(|line| line == "verify-workspace-topology"),
    "{list_text}"
  );
}

#[test]
fn verify_assets_command_accepts_current_repo_contract() {
  let output = Command::new(xtask_bin())
    .arg("verify-assets")
    .output()
    .unwrap();

  assert_success(output);
}

#[test]
fn verify_workspace_topology_command_accepts_current_repo_contract() {
  let output = Command::new(xtask_bin())
    .arg("verify-workspace-topology")
    .output()
    .unwrap();

  assert_success(output);
}

#[test]
fn docs_commands_install_dependencies_and_run_docs_scripts_from_docs_site() {
  let dir = unique_temp_dir("docs-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-bun.log");
  let cwd_log = dir.join("fake-bun-cwd.log");
  write_fake_command(&fake_bin, "bun", 0);

  let check = Command::new(xtask_bin())
    .arg("docs-check")
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .env("CADDER_FAKE_TOOL_CWD_LOG", &cwd_log)
    .output()
    .unwrap();
  assert_success(check);

  let build = Command::new(xtask_bin())
    .arg("docs-build")
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .env("CADDER_FAKE_TOOL_CWD_LOG", &cwd_log)
    .output()
    .unwrap();
  assert_success(build);

  let invocations = fs::read_to_string(&log).unwrap();
  assert!(
    invocations.matches("install --frozen-lockfile").count() >= 2,
    "{invocations}"
  );
  assert!(invocations.contains("run check"), "{invocations}");
  assert!(invocations.contains("run build"), "{invocations}");
  let cwd = fs::read_to_string(&cwd_log).unwrap().replace('\\', "/");
  assert!(cwd.contains("docs/site"), "{cwd}");
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_command_runs_validation_pipeline_with_fake_cargo() {
  let dir = unique_temp_dir("check-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-tool.log");
  write_fake_command(&fake_bin, "cargo", 0);
  write_fake_command(&fake_bin, "bun", 0);

  let output = Command::new(xtask_bin())
    .arg("check")
    .current_dir(workspace_root())
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .output()
    .unwrap();

  assert_success(output);
  let invocations = fs::read_to_string(&log).unwrap();
  assert!(invocations.contains("fmt --check"), "{invocations}");
  assert!(
    invocations.contains("clippy --workspace --all-targets -- -D warnings"),
    "{invocations}"
  );
  assert!(invocations.contains("test --workspace"), "{invocations}");
  assert!(
    invocations.contains("install --frozen-lockfile"),
    "{invocations}"
  );
  assert!(invocations.contains("run check"), "{invocations}");
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn coverage_command_invokes_cargo_llvm_cov_and_checks_lcov_threshold() {
  let dir = unique_temp_dir("coverage-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-cargo.log");
  write_fake_command(&fake_bin, "bun", 0);
  write_fake_coverage_cargo(
    &fake_bin,
    0,
    "TN:\nSF:lib.rs\nDA:1,1\nDA:2,1\nDA:3,1\nend_of_record\n",
  );

  let report = dir.join("reports").join("summary.lcov");
  let output = Command::new(xtask_bin())
    .args(["coverage", "--output"])
    .arg(&report)
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .output()
    .unwrap();

  assert_success(output);
  assert!(report.parent().unwrap().is_dir());
  let invocation = fs::read_to_string(&log).unwrap();
  assert!(invocation.contains("llvm-cov"), "{invocation}");
  assert!(invocation.contains("--workspace"), "{invocation}");
  assert!(invocation.contains("--lcov"), "{invocation}");
  assert!(invocation.contains("--output-path"), "{invocation}");
  assert!(
    invocation.contains(&report.display().to_string()),
    "{invocation}"
  );
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn coverage_command_reports_lcov_below_threshold_after_cargo_success() {
  let dir = unique_temp_dir("coverage-command-low-report");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  write_fake_command(&fake_bin, "bun", 0);
  write_fake_coverage_cargo(
    &fake_bin,
    0,
    "TN:\nSF:lib.rs\nDA:1,1\nDA:2,1\nDA:3,0\nend_of_record\n",
  );

  let output = Command::new(xtask_bin())
    .args(["coverage", "--output"])
    .arg(dir.join("summary.lcov"))
    .env("PATH", &fake_bin)
    .output()
    .unwrap();

  assert!(!output.status.success());
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("below required 85.00%"), "{stderr}");
  assert!(stderr.contains("2/3"), "{stderr}");
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn coverage_command_reports_cargo_failure() {
  let dir = unique_temp_dir("coverage-command-failure");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  write_fake_command(&fake_bin, "bun", 0);
  write_fake_command(&fake_bin, "cargo", 7);

  let output = Command::new(xtask_bin())
    .args(["coverage", "--output"])
    .arg(dir.join("summary.lcov"))
    .env("PATH", &fake_bin)
    .output()
    .unwrap();

  assert!(!output.status.success());
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("cargo"), "{stderr}");
  assert!(stderr.contains("failed"), "{stderr}");
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn dev_env_command_outputs_repeatable_json_without_local_workspace_contract() {
  let output = Command::new(xtask_bin())
    .args(["dev-env", "--format", "json"])
    .output()
    .unwrap();

  let stdout = output.stdout.clone();
  assert_success(output);
  let value: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
  assert_eq!(value["CADDER_RUNTIME_PROFILE"], "dev");
  assert_eq!(value["CADDER_CADDY_BACKEND"], "mock");
  assert!(value.get("CADDER_DESKTOP_DEV_WINDOWS").is_none());
  assert!(
    !value["CADDER_DEV_WORKSPACE"]
      .as_str()
      .unwrap()
      .contains(".local")
  );
}

#[test]
fn dev_run_command_invokes_program_with_dev_environment() {
  let dir = unique_temp_dir("dev-run-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-tool.log");
  let env_log = dir.join("fake-tool-env.log");
  write_fake_command(&fake_bin, "dev-helper", 0);

  let output = Command::new(xtask_bin())
    .args(["dev-run", "--", "dev-helper", "status"])
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .env("CADDER_FAKE_TOOL_ENV_LOG", &env_log)
    .output()
    .unwrap();

  assert_success(output);
  assert!(fs::read_to_string(&log).unwrap().contains("status"));
  let env_value = fs::read_to_string(&env_log).unwrap();
  assert!(
    env_value.contains("CADDER_RUNTIME_PROFILE=dev"),
    "{env_value}"
  );
  assert!(
    env_value.contains("CADDER_CADDY_BACKEND=mock"),
    "{env_value}"
  );
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn dist_command_builds_release_binaries_and_verifies_layout() {
  let dir = unique_temp_dir("dist-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-cargo.log");
  write_fake_command(&fake_bin, "cargo", 0);
  write_fake_command(&fake_bin, "bun", 0);

  let target = fake_target_name("dist-command");
  let release_dir = workspace_root()
    .join("target")
    .join(&target)
    .join("release");
  fs::create_dir_all(&release_dir).unwrap();
  write_fake_portable_executable_layout_for_target(
    &release_dir,
    &target,
    runtime_portable_binaries(),
  );

  let out_dir = dir.join("dist");
  let output = Command::new(xtask_bin())
    .args(["dist", "--out"])
    .arg(&out_dir)
    .args(["--target", &target])
    .current_dir(workspace_root())
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .output()
    .unwrap();

  assert_success(output);
  let invocation = fs::read_to_string(&log).unwrap();
  assert!(invocation.contains("build --release"), "{invocation}");
  assert!(invocation.contains("-p cadder-shim"), "{invocation}");
  assert!(invocation.contains("--target"), "{invocation}");
  assert!(invocation.contains(&target), "{invocation}");
  for binary in runtime_portable_binaries() {
    assert!(out_dir.join(exe_name_for_target(binary, &target)).is_file());
  }
  assert!(out_dir.join("cadder.toml").is_file());

  fs::remove_dir_all(workspace_root().join("target").join(&target)).unwrap();
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn package_command_builds_dist_and_writes_archive_checksum() {
  let dir = unique_temp_dir("package-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let log = dir.join("fake-cargo.log");
  write_fake_command(&fake_bin, "cargo", 0);
  write_fake_command(&fake_bin, "bun", 0);

  let target = format!("fake-windows-package-command-{}", unique_suffix());
  let release_dir = workspace_root()
    .join("target")
    .join(&target)
    .join("release");
  fs::create_dir_all(&release_dir).unwrap();
  write_fake_portable_executable_layout_for_target(
    &release_dir,
    &target,
    runtime_portable_binaries(),
  );

  let out_dir = dir.join("package");
  let output = Command::new(xtask_bin())
    .args(["package", "--out"])
    .arg(&out_dir)
    .args([
      "--platform",
      "windows-x64",
      "--target",
      &target,
      "--version",
      "1.2.3",
    ])
    .current_dir(workspace_root())
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &log)
    .output()
    .unwrap();

  assert_success(output);
  let invocation = fs::read_to_string(&log).unwrap();
  assert!(invocation.contains("build --release"), "{invocation}");
  assert!(invocation.contains(&target), "{invocation}");
  assert!(out_dir.join("cadder-1.2.3-windows-x64.zip").is_file());
  assert!(
    out_dir
      .join("cadder-1.2.3-windows-x64.zip.sha256")
      .is_file()
  );
  assert!(
    out_dir
      .join("layouts")
      .join("cadder-1.2.3-windows-x64")
      .join("cadder.toml")
      .is_file()
  );

  fs::remove_dir_all(workspace_root().join("target").join(&target)).unwrap();
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn runtime_installer_command_builds_runtime_only_windows_msi_and_manifest() {
  let dir = unique_temp_dir("runtime-installer-command");
  fs::create_dir_all(&dir).unwrap();
  let fake_bin = dir.join("bin");
  fs::create_dir_all(&fake_bin).unwrap();
  let cargo_log = dir.join("fake-cargo.log");
  let wix_log = dir.join("fake-wix.log");
  write_fake_command(&fake_bin, "cargo", 0);
  write_fake_command(&fake_bin, "bun", 0);
  write_fake_wix(&fake_bin);

  let target = format!("fake-windows-runtime-installer-{}", unique_suffix());
  let release_dir = workspace_root()
    .join("target")
    .join(&target)
    .join("release");
  fs::create_dir_all(&release_dir).unwrap();
  write_fake_portable_executable_layout_for_target(
    &release_dir,
    &target,
    runtime_portable_binaries(),
  );

  let out_dir = dir.join("runtime-installer");
  let output = Command::new(xtask_bin())
    .args(["runtime-installer", "--out"])
    .arg(&out_dir)
    .args([
      "--platform",
      "windows-x64",
      "--target",
      &target,
      "--version",
      "1.2.3",
    ])
    .current_dir(workspace_root())
    .env("PATH", &fake_bin)
    .env("CADDER_FAKE_TOOL_LOG", &cargo_log)
    .env("CADDER_FAKE_WIX_LOG", &wix_log)
    .output()
    .unwrap();

  assert_success(output);
  let cargo_invocation = fs::read_to_string(&cargo_log).unwrap();
  assert!(
    cargo_invocation.contains("build --release"),
    "{cargo_invocation}"
  );
  let wix_invocation = fs::read_to_string(&wix_log).unwrap();
  assert!(wix_invocation.contains("build"), "{wix_invocation}");
  assert!(wix_invocation.contains("-arch x64"), "{wix_invocation}");

  let installer = out_dir.join("cadder-runtime-1.2.3-windows-x64.msi");
  let manifest = out_dir.join("cadder-runtime-1.2.3-windows-x64.msi.manifest.json");
  assert!(installer.is_file());
  assert!(
    out_dir
      .join("cadder-runtime-1.2.3-windows-x64.msi.sha256")
      .is_file()
  );
  assert!(manifest.is_file());
  assert!(
    fs::read_to_string(&manifest)
      .unwrap()
      .contains(r#""component": "daemon-runtime""#)
  );
  fs::remove_dir_all(workspace_root().join("target").join(&target)).unwrap();
  fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn lightweight_command_parse_errors_do_not_run_heavy_build_steps() {
  let cases: &[&[&str]] = &[
    &["coverage", "--output"],
    &["definitely-not-a-heavy-command"],
    &["dist"],
    &["package", "--out", "target/package-without-platform"],
    &["runtime-installer", "--out"],
    &["verify-runtime-installer-dist", "--dir"],
  ];

  for args in cases {
    let output = Command::new(xtask_bin()).args(*args).output().unwrap();

    assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
      stderr.contains("requires a value")
        || stderr.contains("is required")
        || stderr.contains("Error:"),
      "{args:?} stderr did not describe the parse failure: {stderr}"
    );
  }
}

#[test]
fn unknown_command_reports_xtask_error() {
  let output = Command::new(xtask_bin())
    .arg("definitely-not-a-command")
    .output()
    .unwrap();

  assert!(!output.status.success());
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("unknown xtask command"));
}

fn run_xtask<const N: usize>(args: [&str; N], path_arg: Option<&Path>) -> Output {
  let mut command = Command::new(xtask_bin());
  command.args(args);
  if let Some(path) = path_arg {
    command.arg(path);
  }
  command.output().unwrap()
}

fn xtask_bin() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_xtask"))
}

fn assert_success(output: Output) {
  assert!(
    output.status.success(),
    "xtask failed\nstatus: {}\nstdout: {}\nstderr: {}",
    output.status,
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  );
}

fn write_fake_portable_executable_layout(dir: &Path, binaries: &[&str], include_config: bool) {
  let helper_dir = unique_temp_dir("fake-portable-tool");
  fs::create_dir_all(&helper_dir).unwrap();
  let helper = build_fake_portable_tool(&helper_dir);
  for binary in binaries {
    fs::copy(&helper, dir.join(exe_name(binary))).unwrap();
  }
  fs::remove_dir_all(&helper_dir).unwrap();
  if include_config {
    fs::write(dir.join("cadder.toml"), SAMPLE_CADDER_TOML).unwrap();
  }
}

fn write_fake_portable_executable_layout_for_target(dir: &Path, target: &str, binaries: &[&str]) {
  let helper_dir = unique_temp_dir("fake-portable-tool-target");
  fs::create_dir_all(&helper_dir).unwrap();
  let helper = build_fake_portable_tool(&helper_dir);
  for binary in binaries {
    fs::copy(&helper, dir.join(exe_name_for_target(binary, target))).unwrap();
  }
  fs::remove_dir_all(&helper_dir).unwrap();
}

fn build_fake_portable_tool(dir: &Path) -> PathBuf {
  let source = dir.join("fake_portable_tool.rs");
  let helper = dir.join(exe_name("fake-portable-tool"));
  fs::write(
    &source,
    r#"
fn main() {
  let args = std::env::args().collect::<Vec<_>>();
  let exe_name = std::env::current_exe()
    .ok()
    .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_string()))
    .unwrap_or_default();
  if exe_name.starts_with("caddy")
    && args.get(1).map(String::as_str) == Some("--cadder-shim-info")
  {
    println!("{{\"role\":\"caddy-shim\"}}");
    return;
  }
  if args.get(1).map(String::as_str) == Some("--help") {
    println!("fake help");
    return;
  }
  if args.get(1).map(String::as_str) == Some("--version") {
    println!("fake version");
    return;
  }
  eprintln!("unexpected fake portable tool invocation: {args:?}");
  std::process::exit(2);
}
"#,
  )
  .unwrap();
  let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
  let status = Command::new(rustc)
    .arg(&source)
    .arg("-o")
    .arg(&helper)
    .status()
    .unwrap();
  assert!(status.success(), "failed to compile fake portable tool");
  helper
}

fn write_fake_command(dir: &Path, name: &str, exit_code: i32) -> PathBuf {
  let path = dir.join(command_file_name(name));
  let source = dir.join(format!("{name}_fake_command.rs"));
  fs::write(
    &source,
    format!(
      r#"
use std::io::Write;

fn append_env_log(key: &str, line: String) {{
  if let Ok(path) = std::env::var(key) {{
    if !path.is_empty() {{
      let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
      writeln!(file, "{{line}}").unwrap();
    }}
  }}
}}

fn main() {{
  let args = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
  append_env_log("CADDER_FAKE_TOOL_LOG", args);
  append_env_log(
    "CADDER_FAKE_TOOL_CWD_LOG",
    std::env::current_dir().unwrap().display().to_string(),
  );
  append_env_log(
    "CADDER_FAKE_TOOL_ENV_LOG",
    [
      "CADDER_RUNTIME_PROFILE",
      "CADDER_DEV_WORKSPACE",
      "CADDER_CADDY_BACKEND",
    ]
    .iter()
    .map(|key| format!("{{key}}={{}}", std::env::var(key).unwrap_or_default()))
    .collect::<Vec<_>>()
    .join("\n"),
  );
  std::process::exit({exit_code});
}}
"#
    ),
  )
  .unwrap();
  let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
  let status = Command::new(rustc)
    .arg(&source)
    .arg("-o")
    .arg(&path)
    .status()
    .unwrap();
  assert!(status.success(), "failed to compile fake {name}");
  path
}

fn write_fake_wix(dir: &Path) -> PathBuf {
  let path = dir.join(command_file_name("wix"));
  let source = dir.join("wix_fake_command.rs");
  fs::write(
    &source,
    r#"
use std::io::Write;
use std::path::PathBuf;

fn append_log(line: String) {
  if let Ok(path) = std::env::var("CADDER_FAKE_WIX_LOG") {
    if !path.is_empty() {
      let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
      writeln!(file, "{line}").unwrap();
    }
  }
}

fn main() {
  let args = std::env::args().skip(1).collect::<Vec<_>>();
  append_log(args.join(" "));
  if let Some(index) = args.iter().position(|arg| arg == "-o") {
    let output = PathBuf::from(&args[index + 1]);
    if let Some(parent) = output.parent() {
      std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(output, b"fake msi").unwrap();
  }
}
"#,
  )
  .unwrap();
  let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
  let status = Command::new(rustc)
    .arg(&source)
    .arg("-o")
    .arg(&path)
    .status()
    .unwrap();
  assert!(status.success(), "failed to compile fake wix");
  path
}

fn write_fake_coverage_cargo(dir: &Path, exit_code: i32, report_text: &str) -> PathBuf {
  let path = dir.join(command_file_name("cargo"));
  let source = dir.join("cargo_fake_coverage.rs");
  let report_literal = format!("{report_text:?}");
  fs::write(
    &source,
    format!(
      r#"
use std::io::Write;
use std::path::PathBuf;

fn append_log(line: &str) {{
  if let Ok(path) = std::env::var("CADDER_FAKE_TOOL_LOG") {{
    if !path.is_empty() {{
      let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
      writeln!(file, "{{line}}").unwrap();
    }}
  }}
}}

fn main() {{
  let args = std::env::args().skip(1).collect::<Vec<_>>();
  append_log(&args.join(" "));
  if {exit_code} == 0 {{
    if let Some(index) = args.iter().position(|arg| arg == "--output-path") {{
      let report = PathBuf::from(&args[index + 1]);
      if let Some(parent) = report.parent() {{
        std::fs::create_dir_all(parent).unwrap();
      }}
      std::fs::write(&report, {report_literal}.as_bytes()).unwrap();
    }}
  }}
  std::process::exit({exit_code});
}}
"#
    ),
  )
  .unwrap();
  let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
  let status = Command::new(rustc)
    .arg(&source)
    .arg("-o")
    .arg(&path)
    .status()
    .unwrap();
  assert!(status.success(), "failed to compile fake coverage cargo");
  path
}

fn command_file_name(name: &str) -> String {
  if cfg!(windows) {
    format!("{name}.exe")
  } else {
    name.to_string()
  }
}

fn runtime_portable_binaries() -> &'static [&'static str] {
  &["cadderd", "cadder", "caddy"]
}

fn exe_name(name: &str) -> String {
  if cfg!(windows) {
    format!("{name}.exe")
  } else {
    name.to_string()
  }
}

fn exe_name_for_target(name: &str, target: &str) -> String {
  if target.contains("windows") {
    format!("{name}.exe")
  } else {
    name.to_string()
  }
}

fn fake_target_name(name: &str) -> String {
  let platform = if cfg!(windows) { "windows" } else { "linux" };
  format!("fake-{platform}-{name}-{}", unique_suffix())
}

fn workspace_root() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn unique_temp_dir(name: &str) -> PathBuf {
  let unique = unique_suffix();
  env::temp_dir().join(format!(
    "cadder-xtask-cli-{name}-{}-{unique}",
    std::process::id()
  ))
}

fn unique_suffix() -> u128 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos()
}

const SAMPLE_CADDER_TOML: &str = r#"# Cadder portable configuration.
# Uncomment and set this when the real Caddy binary is not the first safe `caddy` on PATH.
#
# [caddy]
# real_command = "/absolute/path/to/caddy"
"#;
