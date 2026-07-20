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

fn write_fake_openspec(dir: &Path, version: &str) -> PathBuf {
  let path = dir.join(command_file_name("openspec"));
  let source = dir.join("openspec_fake_command.rs");
  fs::write(
    &source,
    format!(
      r#"
use std::io::Write;

fn main() {{
  let args = std::env::args().skip(1).collect::<Vec<_>>();
  if let Ok(path) = std::env::var("CADDER_FAKE_TOOL_LOG") {{
    if !path.is_empty() {{
      let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
      writeln!(file, "{{}}", args.join(" ")).unwrap();
    }}
  }}
  if args == ["--version"] {{
    println!("{version}");
  }}
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
  assert!(status.success(), "failed to compile fake openspec");
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

const SAMPLE_CADDER_TOML: &str = r#"# Cadder configuration template.
# Keep this file beside the Cadder executables.

[caddy]
# real_command = "caddy-real"
# real_path = "/absolute/path/to/caddy"

"#;
