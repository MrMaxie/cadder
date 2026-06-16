use anyhow::{Context, Result, bail};
use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};
use std::{
  env, fs,
  fs::File,
  io::{self, Read},
  path::{Path, PathBuf},
  process::{Command, Stdio},
};
use tar::Builder;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const COVERAGE_FAIL_UNDER_LINES: &str = "85";
const DEFAULT_COVERAGE_REPORT_PATH: &str = "target/llvm-cov/coverage-summary.json";
const WINDOWS_COVERAGE_TOOLCHAIN: &str = "stable-x86_64-pc-windows-msvc";
const WORKSPACE_MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml");
const SAMPLE_CADDER_TOML: &str = r#"# Cadder portable configuration.
# Uncomment and set this when the real Caddy binary is not the first safe `caddy` on PATH.
#
# [caddy]
# real_command = "/absolute/path/to/caddy"
"#;

fn main() -> Result<()> {
  let mut args = env::args().skip(1);
  let command = args.next().unwrap_or_else(|| "check".to_string());
  match command.as_str() {
    "check" => check(),
    "coverage" => coverage(CoverageOptions::parse(args.collect())?),
    "dist" => dist(DistOptions::parse(args.collect())?),
    "verify-dist" => verify_dist(&VerifyDistOptions::parse(args.collect())?),
    "package" => package(PackageOptions::parse(args.collect())?),
    other => bail!("unknown xtask command `{other}`"),
  }
}

fn check() -> Result<()> {
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
  run("cargo", &["test", "--workspace"])?;
  Ok(())
}

fn coverage(options: CoverageOptions) -> Result<()> {
  ensure_parent_dir(&options.output_path)?;
  let args = coverage_command_args(&options.output_path)?;
  let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
  run("cargo", &arg_refs)
}

fn dist(options: DistOptions) -> Result<()> {
  build_release_binaries(options.target.as_deref())?;

  fs::create_dir_all(&options.out_dir)
    .with_context(|| format!("create {}", options.out_dir.display()))?;
  for binary in portable_binaries() {
    let source = release_binary_path(binary, options.target.as_deref());
    let target = options
      .out_dir
      .join(exe_name(binary, options.target.as_deref()));
    fs::copy(&source, &target)
      .with_context(|| format!("copy {} to {}", source.display(), target.display()))?;
  }
  fs::write(options.out_dir.join("cadder.toml"), SAMPLE_CADDER_TOML)
    .with_context(|| format!("write {}", options.out_dir.join("cadder.toml").display()))?;

  verify_dist(&VerifyDistOptions {
    dir: options.out_dir,
    target: options.target,
  })
}

fn verify_dist(options: &VerifyDistOptions) -> Result<()> {
  for binary in portable_binaries() {
    let path = options
      .dir
      .join(exe_name(binary, options.target.as_deref()));
    if !path.is_file() {
      bail!("portable binary missing: {}", path.display());
    }
  }

  let config = options.dir.join("cadder.toml");
  if !config.is_file() {
    bail!(
      "portable sample configuration missing: {}",
      config.display()
    );
  }

  let shim = options
    .dir
    .join(exe_name("caddy", options.target.as_deref()));
  let output = Command::new(&shim)
    .arg("--cadder-shim-info")
    .stdin(Stdio::null())
    .output()
    .with_context(|| format!("run {}", shim.display()))?;
  if !output.status.success() {
    bail!("caddy --cadder-shim-info failed with {}", output.status);
  }
  let stdout = String::from_utf8_lossy(&output.stdout);
  if !stdout.contains("\"role\":\"caddy-shim\"") && !stdout.contains("\"role\": \"caddy-shim\"") {
    bail!("caddy --cadder-shim-info did not report the Cadder shim role");
  }

  let ctl = options
    .dir
    .join(exe_name("cadderctl", options.target.as_deref()));
  let ctl_output = Command::new(&ctl)
    .arg("--help")
    .stdin(Stdio::null())
    .output()
    .with_context(|| format!("run {}", ctl.display()))?;
  if !ctl_output.status.success() {
    bail!("cadderctl --help failed with {}", ctl_output.status);
  }

  let mcp = options
    .dir
    .join(exe_name("cadder-mcp", options.target.as_deref()));
  let mcp_output = Command::new(&mcp)
    .arg("--help")
    .stdin(Stdio::null())
    .output()
    .with_context(|| format!("run {}", mcp.display()))?;
  if !mcp_output.status.success() {
    bail!("cadder-mcp --help failed with {}", mcp_output.status);
  }

  Ok(())
}

fn package(options: PackageOptions) -> Result<()> {
  package_with_dist(options, dist)
}

fn package_with_dist(
  options: PackageOptions,
  create_dist: impl FnOnce(DistOptions) -> Result<()>,
) -> Result<()> {
  let archive_stem = format!("cadder-{}-{}", options.version, options.platform);
  let layout_parent = options.out_dir.join("layouts");
  let layout_dir = layout_parent.join(&archive_stem);
  if layout_dir.exists() {
    fs::remove_dir_all(&layout_dir).with_context(|| format!("remove {}", layout_dir.display()))?;
  }

  create_dist(DistOptions {
    out_dir: layout_dir,
    target: options.target,
  })?;

  fs::create_dir_all(&options.out_dir)
    .with_context(|| format!("create {}", options.out_dir.display()))?;
  let archive_path = options.out_dir.join(format!(
    "{archive_stem}.{}",
    archive_extension(&options.platform)
  ));
  if archive_path.exists() {
    fs::remove_file(&archive_path).with_context(|| format!("remove {}", archive_path.display()))?;
  }

  match archive_kind(&options.platform) {
    ArchiveKind::Zip => write_zip_archive(&layout_parent, &archive_stem, &archive_path)?,
    ArchiveKind::TarGz => write_tar_gz_archive(&layout_parent, &archive_stem, &archive_path)?,
  }

  let checksum_path = options.out_dir.join(format!(
    "{}.sha256",
    archive_path
      .file_name()
      .and_then(|name| name.to_str())
      .context("archive path has no UTF-8 file name")?
  ));
  write_sha256_file(&archive_path, &checksum_path)?;

  println!("created {}", archive_path.display());
  println!("created {}", checksum_path.display());
  Ok(())
}

fn portable_binaries() -> [&'static str; 5] {
  ["cadderd", "cadderctl", "cadder-mcp", "cadder-tui", "caddy"]
}

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

fn build_release_binaries(target: Option<&str>) -> Result<()> {
  let mut args = vec![
    "build",
    "--release",
    "-p",
    "cadderd",
    "-p",
    "cadderctl",
    "-p",
    "cadder-mcp",
    "-p",
    "cadder-tui",
    "-p",
    "cadder-shim",
  ];
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

  // No project-specific coverage exclusions are applied here. Future exclusions must be limited
  // to generated, platform-gated, or intentionally untestable code and documented beside this gate.
  let mut args = Vec::new();
  if cfg!(windows) {
    args.push(format!("+{WINDOWS_COVERAGE_TOOLCHAIN}"));
  }
  args.extend([
    "llvm-cov".to_string(),
    "--workspace".to_string(),
    "--json".to_string(),
    "--summary-only".to_string(),
    "--fail-under-lines".to_string(),
    COVERAGE_FAIL_UNDER_LINES.to_string(),
    "--output-path".to_string(),
    output_path.to_string(),
  ]);
  Ok(args)
}

fn archive_kind(platform: &str) -> ArchiveKind {
  if platform.starts_with("windows") {
    ArchiveKind::Zip
  } else {
    ArchiveKind::TarGz
  }
}

fn archive_extension(platform: &str) -> &'static str {
  match archive_kind(platform) {
    ArchiveKind::Zip => "zip",
    ArchiveKind::TarGz => "tar.gz",
  }
}

fn write_zip_archive(layout_parent: &Path, root_dir: &str, archive_path: &Path) -> Result<()> {
  let file =
    File::create(archive_path).with_context(|| format!("create {}", archive_path.display()))?;
  let mut zip = ZipWriter::new(file);
  let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

  append_zip_dir(
    &mut zip,
    &layout_parent.join(root_dir),
    Path::new(root_dir),
    options,
  )?;
  zip
    .finish()
    .with_context(|| format!("finish {}", archive_path.display()))?;
  Ok(())
}

fn append_zip_dir(
  zip: &mut ZipWriter<File>,
  dir: &Path,
  archive_dir: &Path,
  options: SimpleFileOptions,
) -> Result<()> {
  zip
    .add_directory(path_to_archive_name(archive_dir)?, options)
    .with_context(|| format!("add ZIP directory {}", archive_dir.display()))?;

  for entry in sorted_dir_entries(dir)? {
    let entry_path = entry.path();
    let archive_path = archive_dir.join(entry.file_name());
    if entry_path.is_dir() {
      append_zip_dir(zip, &entry_path, &archive_path, options)?;
    } else {
      zip
        .start_file(path_to_archive_name(&archive_path)?, options)
        .with_context(|| format!("add ZIP file {}", archive_path.display()))?;
      let mut source =
        File::open(&entry_path).with_context(|| format!("open {}", entry_path.display()))?;
      io::copy(&mut source, zip)
        .with_context(|| format!("write ZIP file {}", archive_path.display()))?;
    }
  }

  Ok(())
}

fn write_tar_gz_archive(layout_parent: &Path, root_dir: &str, archive_path: &Path) -> Result<()> {
  let file =
    File::create(archive_path).with_context(|| format!("create {}", archive_path.display()))?;
  let encoder = GzEncoder::new(file, Compression::default());
  let mut tar = Builder::new(encoder);
  tar
    .append_dir_all(root_dir, layout_parent.join(root_dir))
    .with_context(|| {
      format!(
        "write TAR layout {}",
        layout_parent.join(root_dir).display()
      )
    })?;
  tar
    .into_inner()
    .context("finish TAR stream")?
    .finish()
    .with_context(|| format!("finish {}", archive_path.display()))?;
  Ok(())
}

fn write_sha256_file(archive_path: &Path, checksum_path: &Path) -> Result<()> {
  let mut file =
    File::open(archive_path).with_context(|| format!("open {}", archive_path.display()))?;
  let mut hasher = Sha256::new();
  let mut buffer = [0; 64 * 1024];
  loop {
    let bytes_read = file
      .read(&mut buffer)
      .with_context(|| format!("read {}", archive_path.display()))?;
    if bytes_read == 0 {
      break;
    }
    hasher.update(&buffer[..bytes_read]);
  }

  let file_name = archive_path
    .file_name()
    .and_then(|name| name.to_str())
    .context("archive path has no UTF-8 file name")?;
  let checksum = hex::encode(hasher.finalize());
  fs::write(checksum_path, format!("{checksum}  {file_name}\n"))
    .with_context(|| format!("write {}", checksum_path.display()))?;
  Ok(())
}

fn path_to_archive_name(path: &Path) -> Result<String> {
  let mut parts = Vec::new();
  for component in path.components() {
    let component = component.as_os_str().to_str().with_context(|| {
      format!(
        "archive path contains non-UTF-8 component: {}",
        path.display()
      )
    })?;
    parts.push(component);
  }
  Ok(parts.join("/"))
}

fn sorted_dir_entries(dir: &Path) -> Result<Vec<fs::DirEntry>> {
  let mut entries = fs::read_dir(dir)
    .with_context(|| format!("read {}", dir.display()))?
    .collect::<std::result::Result<Vec<_>, _>>()
    .with_context(|| format!("read entries from {}", dir.display()))?;
  entries.sort_by_key(|entry| entry.file_name());
  Ok(entries)
}

fn run(program: &str, args: &[&str]) -> Result<()> {
  let status = Command::new(program)
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

#[derive(Debug, PartialEq, Eq)]
struct CoverageOptions {
  output_path: PathBuf,
}

impl CoverageOptions {
  fn parse(args: Vec<String>) -> Result<Self> {
    Ok(Self {
      output_path: optional_path_option(&args, "--output")?
        .unwrap_or_else(|| PathBuf::from(DEFAULT_COVERAGE_REPORT_PATH)),
    })
  }
}

#[derive(Debug, PartialEq, Eq)]
struct DistOptions {
  out_dir: PathBuf,
  target: Option<String>,
}

impl DistOptions {
  fn parse(args: Vec<String>) -> Result<Self> {
    Ok(Self {
      out_dir: required_path_option(&args, "--out")?,
      target: optional_string_option(&args, "--target")?,
    })
  }
}

#[derive(Debug, PartialEq, Eq)]
struct VerifyDistOptions {
  dir: PathBuf,
  target: Option<String>,
}

impl VerifyDistOptions {
  fn parse(args: Vec<String>) -> Result<Self> {
    Ok(Self {
      dir: required_path_option(&args, "--dir")?,
      target: optional_string_option(&args, "--target")?,
    })
  }
}

#[derive(Debug, PartialEq, Eq)]
struct PackageOptions {
  out_dir: PathBuf,
  version: String,
  platform: String,
  target: Option<String>,
}

impl PackageOptions {
  fn parse(args: Vec<String>) -> Result<Self> {
    Ok(Self {
      out_dir: required_path_option(&args, "--out")?,
      version: optional_string_option(&args, "--version")?
        .map_or_else(workspace_package_version, Ok)?,
      platform: required_string_option(&args, "--platform")?,
      target: optional_string_option(&args, "--target")?,
    })
  }
}

fn workspace_package_version() -> Result<String> {
  workspace_package_version_from_manifest(Path::new(WORKSPACE_MANIFEST))
}

fn workspace_package_version_from_manifest(manifest: &Path) -> Result<String> {
  let contents =
    fs::read_to_string(manifest).with_context(|| format!("read {}", manifest.display()))?;
  let mut in_workspace_package = false;

  for line in contents.lines() {
    let trimmed = line.split('#').next().unwrap_or_default().trim();
    if trimmed.is_empty() {
      continue;
    }

    if trimmed.starts_with('[') && trimmed.ends_with(']') {
      in_workspace_package = trimmed == "[workspace.package]";
      continue;
    }

    if in_workspace_package
      && let Some((key, value)) = trimmed.split_once('=')
      && key.trim() == "version"
    {
      let version = value.trim().trim_matches('"');
      if version.is_empty() {
        bail!(
          "workspace package version is empty in {}",
          manifest.display()
        );
      }
      return Ok(version.to_string());
    }
  }

  bail!(
    "workspace package version not found in {}",
    manifest.display()
  )
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ArchiveKind {
  Zip,
  TarGz,
}

fn required_path_option(args: &[String], option: &str) -> Result<PathBuf> {
  required_string_option(args, option).map(PathBuf::from)
}

fn optional_path_option(args: &[String], option: &str) -> Result<Option<PathBuf>> {
  optional_string_option(args, option).map(|value| value.map(PathBuf::from))
}

fn required_string_option(args: &[String], option: &str) -> Result<String> {
  optional_string_option(args, option)?.ok_or_else(|| anyhow::anyhow!("{option} is required"))
}

fn optional_string_option(args: &[String], option: &str) -> Result<Option<String>> {
  let mut iter = args.iter();
  while let Some(arg) = iter.next() {
    if arg == option {
      return iter
        .next()
        .cloned()
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("{option} requires a value"));
    }
  }
  Ok(None)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_path_option_reads_value_after_option() {
    let path =
      required_path_option(&["--out".to_string(), "target/dist".to_string()], "--out").unwrap();

    assert_eq!(path, PathBuf::from("target/dist"));
  }

  #[test]
  fn optional_string_option_reads_value_after_option() {
    let value = optional_string_option(
      &[
        "--target".to_string(),
        "x86_64-unknown-linux-gnu".to_string(),
      ],
      "--target",
    )
    .unwrap();

    assert_eq!(value, Some("x86_64-unknown-linux-gnu".to_string()));
  }

  #[test]
  fn dist_options_parse_target() {
    let options = DistOptions::parse(vec![
      "--out".to_string(),
      "target/dist".to_string(),
      "--target".to_string(),
      "x86_64-unknown-linux-gnu".to_string(),
    ])
    .unwrap();

    assert_eq!(
      options,
      DistOptions {
        out_dir: PathBuf::from("target/dist"),
        target: Some("x86_64-unknown-linux-gnu".to_string())
      }
    );
  }

  #[test]
  fn package_options_parse_required_values() {
    let options = PackageOptions::parse(vec![
      "--out".to_string(),
      "target/artifacts".to_string(),
      "--version".to_string(),
      "0.1.0".to_string(),
      "--platform".to_string(),
      "linux-x64".to_string(),
      "--target".to_string(),
      "x86_64-unknown-linux-gnu".to_string(),
    ])
    .unwrap();

    assert_eq!(
      options,
      PackageOptions {
        out_dir: PathBuf::from("target/artifacts"),
        version: "0.1.0".to_string(),
        platform: "linux-x64".to_string(),
        target: Some("x86_64-unknown-linux-gnu".to_string())
      }
    );
  }

  #[test]
  fn package_options_default_to_workspace_package_version() {
    let options = PackageOptions::parse(vec![
      "--out".to_string(),
      "target/artifacts".to_string(),
      "--platform".to_string(),
      "linux-x64".to_string(),
    ])
    .unwrap();

    assert_eq!(options.version, workspace_package_version().unwrap());
  }

  #[test]
  fn workspace_package_version_from_manifest_reads_workspace_package_version() {
    let dir = unique_temp_dir("workspace-version");
    let manifest = dir.join("Cargo.toml");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
      &manifest,
      r#"
[package]
name = "sample"
version = "0.1.0"

[workspace.package]
version = "1.2.3"
edition = "2024"
"#,
    )
    .unwrap();

    let version = workspace_package_version_from_manifest(&manifest).unwrap();

    assert_eq!(version, "1.2.3");
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn coverage_options_uses_default_report_path() {
    let options = CoverageOptions::parse(Vec::new()).unwrap();

    assert_eq!(
      options,
      CoverageOptions {
        output_path: PathBuf::from(DEFAULT_COVERAGE_REPORT_PATH)
      }
    );
  }

  #[test]
  fn coverage_options_parse_output_path() {
    let options = CoverageOptions::parse(vec![
      "--output".to_string(),
      "target/custom/summary.json".to_string(),
    ])
    .unwrap();

    assert_eq!(
      options,
      CoverageOptions {
        output_path: PathBuf::from("target/custom/summary.json")
      }
    );
  }

  #[test]
  fn coverage_command_args_enforce_workspace_line_threshold() {
    let args = coverage_command_args(Path::new("target/custom/summary.json")).unwrap();

    let mut expected = Vec::new();
    if cfg!(windows) {
      expected.push(format!("+{WINDOWS_COVERAGE_TOOLCHAIN}"));
    }
    expected.extend([
      "llvm-cov".to_string(),
      "--workspace".to_string(),
      "--json".to_string(),
      "--summary-only".to_string(),
      "--fail-under-lines".to_string(),
      "85".to_string(),
      "--output-path".to_string(),
      "target/custom/summary.json".to_string(),
    ]);

    assert_eq!(args, expected);
  }

  #[test]
  fn ensure_parent_dir_creates_report_directory() {
    let dir = unique_temp_dir("coverage-report-parent");
    let report_path = dir.join("nested").join("summary.json");

    ensure_parent_dir(&report_path).unwrap();

    assert!(dir.join("nested").is_dir());
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn parse_path_option_rejects_missing_option() {
    let error = required_path_option(&[], "--out").unwrap_err();

    assert!(error.to_string().contains("--out is required"));
  }

  #[test]
  fn portable_layout_includes_expected_binaries() {
    assert_eq!(
      portable_binaries(),
      ["cadderd", "cadderctl", "cadder-mcp", "cadder-tui", "caddy"]
    );
  }

  #[test]
  fn parse_path_option_reads_value_after_unrelated_arguments() {
    let path = required_path_option(
      &[
        "--verbose".to_string(),
        "--dir".to_string(),
        "target/dist".to_string(),
      ],
      "--dir",
    )
    .unwrap();

    assert_eq!(path, PathBuf::from("target/dist"));
  }

  #[test]
  fn parse_path_option_rejects_option_without_value() {
    let error = required_path_option(&["--dir".to_string()], "--dir").unwrap_err();

    assert!(error.to_string().contains("--dir requires a value"));
  }

  #[test]
  fn release_binary_path_uses_release_directory_and_executable_name() {
    assert_eq!(
      release_binary_path("cadderd", None),
      PathBuf::from("target")
        .join("release")
        .join(exe_name("cadderd", None))
    );
  }

  #[test]
  fn release_binary_path_uses_target_release_directory() {
    assert_eq!(
      release_binary_path("cadderd", Some("x86_64-unknown-linux-gnu")),
      PathBuf::from("target")
        .join("x86_64-unknown-linux-gnu")
        .join("release")
        .join("cadderd")
    );
  }

  #[test]
  fn exe_name_uses_windows_suffix_for_windows_target() {
    assert_eq!(
      exe_name("cadderd", Some("x86_64-pc-windows-msvc")),
      "cadderd.exe"
    );
  }

  #[test]
  fn archive_extension_uses_zip_for_windows() {
    assert_eq!(archive_extension("windows-x64"), "zip");
  }

  #[test]
  fn archive_extension_uses_tar_gz_for_unix_platforms() {
    assert_eq!(archive_extension("linux-x64"), "tar.gz");
    assert_eq!(archive_extension("macos-arm64"), "tar.gz");
  }

  #[test]
  fn verify_dist_rejects_missing_portable_files() {
    let dir = unique_temp_dir("missing-portable-files");
    fs::create_dir_all(&dir).unwrap();

    let error = verify_dist(&VerifyDistOptions {
      dir: dir.clone(),
      target: None,
    })
    .unwrap_err();

    assert!(error.to_string().contains("portable binary missing"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_dist_rejects_missing_sample_config_after_binaries_exist() {
    let dir = unique_temp_dir("missing-sample-config");
    fs::create_dir_all(&dir).unwrap();
    for binary in portable_binaries() {
      fs::write(dir.join(exe_name(binary, None)), b"not executable").unwrap();
    }

    let error = verify_dist(&VerifyDistOptions {
      dir: dir.clone(),
      target: None,
    })
    .unwrap_err();

    assert!(
      error
        .to_string()
        .contains("portable sample configuration missing")
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn write_sha256_file_writes_hash_and_file_name() {
    let dir = unique_temp_dir("checksum");
    fs::create_dir_all(&dir).unwrap();
    let archive_path = dir.join("artifact.tar.gz");
    let checksum_path = dir.join("artifact.tar.gz.sha256");
    fs::write(&archive_path, b"artifact").unwrap();

    write_sha256_file(&archive_path, &checksum_path).unwrap();

    let checksum = fs::read_to_string(&checksum_path).unwrap();
    assert!(checksum.ends_with("  artifact.tar.gz\n"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn package_with_dist_writes_windows_zip_and_checksum() {
    let out_dir = unique_temp_dir("windows-package");
    let expected_layout = out_dir.join("layouts").join("cadder-1.2.3-windows-x64");

    package_with_dist(
      PackageOptions {
        out_dir: out_dir.clone(),
        version: "1.2.3".to_string(),
        platform: "windows-x64".to_string(),
        target: Some("x86_64-pc-windows-msvc".to_string()),
      },
      |dist_options| {
        assert_eq!(dist_options.out_dir, expected_layout);
        assert_eq!(
          dist_options.target,
          Some("x86_64-pc-windows-msvc".to_string())
        );
        write_fake_portable_layout(
          &dist_options.out_dir,
          &[
            "cadderd.exe",
            "cadderctl.exe",
            "cadder-mcp.exe",
            "cadder-tui.exe",
            "caddy.exe",
          ],
        )
      },
    )
    .unwrap();

    let archive_path = out_dir.join("cadder-1.2.3-windows-x64.zip");
    let checksum_path = out_dir.join("cadder-1.2.3-windows-x64.zip.sha256");
    assert!(archive_path.is_file());
    assert!(checksum_path.is_file());
    assert_zip_entries(
      &archive_path,
      &[
        "cadder-1.2.3-windows-x64/",
        "cadder-1.2.3-windows-x64/cadderctl.exe",
        "cadder-1.2.3-windows-x64/cadder-mcp.exe",
        "cadder-1.2.3-windows-x64/cadder-tui.exe",
        "cadder-1.2.3-windows-x64/cadder.toml",
        "cadder-1.2.3-windows-x64/cadderd.exe",
        "cadder-1.2.3-windows-x64/caddy.exe",
      ],
    );
    assert!(
      fs::read_to_string(&checksum_path)
        .unwrap()
        .ends_with("  cadder-1.2.3-windows-x64.zip\n")
    );

    fs::remove_dir_all(&out_dir).unwrap();
  }

  #[test]
  fn package_with_dist_writes_unix_tar_gz_and_checksum() {
    let out_dir = unique_temp_dir("linux-package");
    let expected_layout = out_dir.join("layouts").join("cadder-1.2.3-linux-x64");

    package_with_dist(
      PackageOptions {
        out_dir: out_dir.clone(),
        version: "1.2.3".to_string(),
        platform: "linux-x64".to_string(),
        target: Some("x86_64-unknown-linux-gnu".to_string()),
      },
      |dist_options| {
        assert_eq!(dist_options.out_dir, expected_layout);
        assert_eq!(
          dist_options.target,
          Some("x86_64-unknown-linux-gnu".to_string())
        );
        write_fake_portable_layout(
          &dist_options.out_dir,
          &["cadderd", "cadderctl", "cadder-mcp", "cadder-tui", "caddy"],
        )
      },
    )
    .unwrap();

    let archive_path = out_dir.join("cadder-1.2.3-linux-x64.tar.gz");
    let checksum_path = out_dir.join("cadder-1.2.3-linux-x64.tar.gz.sha256");
    assert!(archive_path.is_file());
    assert!(checksum_path.is_file());
    assert_tar_gz_entries(
      &archive_path,
      &[
        "cadder-1.2.3-linux-x64/",
        "cadder-1.2.3-linux-x64/cadderctl",
        "cadder-1.2.3-linux-x64/cadder-mcp",
        "cadder-1.2.3-linux-x64/cadder-tui",
        "cadder-1.2.3-linux-x64/cadder.toml",
        "cadder-1.2.3-linux-x64/cadderd",
        "cadder-1.2.3-linux-x64/caddy",
      ],
    );
    assert!(
      fs::read_to_string(&checksum_path)
        .unwrap()
        .ends_with("  cadder-1.2.3-linux-x64.tar.gz\n")
    );

    fs::remove_dir_all(&out_dir).unwrap();
  }

  fn write_fake_portable_layout(dir: &Path, binaries: &[&str]) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    for binary in binaries {
      fs::write(dir.join(binary), binary).with_context(|| format!("write {binary}"))?;
    }
    fs::write(dir.join("cadder.toml"), SAMPLE_CADDER_TOML)
      .with_context(|| format!("write {}", dir.join("cadder.toml").display()))?;
    Ok(())
  }

  fn assert_zip_entries(archive_path: &Path, expected: &[&str]) {
    let file = File::open(archive_path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entries = (0..archive.len())
      .map(|index| archive.by_index(index).unwrap().name().to_string())
      .collect::<Vec<_>>();
    entries.sort();

    let mut expected = expected
      .iter()
      .map(|entry| entry.to_string())
      .collect::<Vec<_>>();
    expected.sort();

    assert_eq!(entries, expected);
  }

  fn assert_tar_gz_entries(archive_path: &Path, expected: &[&str]) {
    let file = File::open(archive_path).unwrap();
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut entries = archive
      .entries()
      .unwrap()
      .map(|entry| {
        entry
          .unwrap()
          .path()
          .unwrap()
          .to_string_lossy()
          .replace('\\', "/")
      })
      .collect::<Vec<_>>();
    entries.sort();

    let mut expected = expected
      .iter()
      .map(|entry| entry.to_string())
      .collect::<Vec<_>>();
    expected.sort();

    assert_eq!(entries, expected);
  }

  fn unique_temp_dir(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_nanos();
    env::temp_dir().join(format!(
      "cadder-xtask-{name}-{}-{unique}",
      std::process::id()
    ))
  }
}
