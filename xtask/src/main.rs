mod openspec_check;

use anyhow::{Context, Result, bail};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
  collections::{BTreeMap, BTreeSet},
  env, fs,
  fs::File,
  io::{self, Read},
  path::{Path, PathBuf},
  process::{Command, Stdio},
};
use tar::Builder;
use tempfile::tempdir;
use toml::{Table, Value};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const COVERAGE_FAIL_UNDER_LINES: f64 = 85.0;
const EXPECTED_OPENSPEC_VERSION: &str = "1.5.0";
const COVERAGE_EXCLUDED_PACKAGES: [&str; 0] = [];
const COVERAGE_IGNORED_FILENAME_REGEX: &str = "";
const CADDER_CADDY_BACKEND_ENV: &str = "CADDER_CADDY_BACKEND";
const DOCS_DOWNLOAD_SCRIPT: &str = "docs/site/public/cadder-downloads.js";
const DOCS_WEBMANIFEST: &str = "docs/site/public/site.webmanifest";
const DEFAULT_COVERAGE_REPORT_PATH: &str = "target/llvm-cov/coverage-summary.lcov";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const RUNTIME_INSTALLER_IDENTIFIER: &str = "dev.maxie.cadder.runtime";
const RUNTIME_INSTALLER_PACKAGE_NAME: &str = "cadder-runtime";
const RUNTIME_INSTALLER_PRODUCT_NAME: &str = "Cadder Runtime";
const RUNTIME_INSTALLER_MANUFACTURER: &str = "Maxie";
const WINDOWS_RUNTIME_INSTALLER_UPGRADE_CODE: &str = "7D7829AF-3F2B-4D5B-B758-1CFF0B31E12D";
const WINDOWS_SIGNTOOL_CERT_PATH_ENV: &str = "CADDER_WINDOWS_SIGNTOOL_CERT_PATH";
const WINDOWS_SIGNTOOL_CERT_PASSWORD_ENV: &str = "CADDER_WINDOWS_SIGNTOOL_CERT_PASSWORD";
const WINDOWS_SIGNTOOL_TIMESTAMP_URL_ENV: &str = "CADDER_WINDOWS_SIGNTOOL_TIMESTAMP_URL";
const MACOS_INSTALLER_SIGNING_IDENTITY_ENV: &str = "CADDER_MACOS_INSTALLER_SIGNING_IDENTITY";
const SIGNING_READY_ENV_VARS: [&str; 2] = [
  "CADDER_WINDOWS_SIGNING_READY",
  "CADDER_MACOS_NOTARIZATION_READY",
];
const CADDER_COVERAGE_TOOLCHAIN_ENV: &str = "CADDER_COVERAGE_TOOLCHAIN";
const WINDOWS_COVERAGE_TOOLCHAIN: &str = "stable-x86_64-pc-windows-msvc";
const WORKSPACE_MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml");
const DOCS_SITE_DIR: &str = "docs/site";
const RUNTIME_PORTABLE_BINARIES: [&str; 3] = ["cadderd", "cadder", "caddy"];
const RUNTIME_RELEASE_PACKAGES: [&str; 3] = ["cadderd", "cadder", "cadder-shim"];
const WORKSPACE_MEMBER_CONTRACTS: [WorkspaceMemberContract; 7] = [
  WorkspaceMemberContract::new(
    "crates/cadder-daemon",
    "cadder-daemon",
    WorkspaceMemberClassification::Daemon,
    false,
  ),
  WorkspaceMemberContract::new(
    "crates/cadderd",
    "cadderd",
    WorkspaceMemberClassification::Daemon,
    true,
  ),
  WorkspaceMemberContract::new(
    "crates/cadder-shim",
    "cadder-shim",
    WorkspaceMemberClassification::Shim,
    true,
  ),
  WorkspaceMemberContract::new(
    "crates/cadder",
    "cadder",
    WorkspaceMemberClassification::OperatorClient,
    true,
  ),
  WorkspaceMemberContract::new(
    "crates/cadder-operator",
    "cadder-operator",
    WorkspaceMemberClassification::OperatorClient,
    false,
  ),
  WorkspaceMemberContract::new(
    "crates/cadder-protocol",
    "cadder-protocol",
    WorkspaceMemberClassification::SharedProtocolApi,
    false,
  ),
  WorkspaceMemberContract::new(
    "xtask",
    "xtask",
    WorkspaceMemberClassification::DocsTooling,
    false,
  ),
];
const RELEASE_PROFILE_SETTINGS: [ProfileSetting; 6] = [
  ProfileSetting::new("release", "opt-level", ExpectedTomlValue::String("s")),
  ProfileSetting::new("release", "lto", ExpectedTomlValue::String("thin")),
  ProfileSetting::new("release", "codegen-units", ExpectedTomlValue::Integer(1)),
  ProfileSetting::new("release", "debug", ExpectedTomlValue::Boolean(false)),
  ProfileSetting::new("release", "strip", ExpectedTomlValue::String("symbols")),
  ProfileSetting::new("release", "panic", ExpectedTomlValue::String("unwind")),
];
const PROFILING_PROFILE_SETTINGS: [ProfileSetting; 3] = [
  ProfileSetting::new(
    "profiling",
    "inherits",
    ExpectedTomlValue::String("release"),
  ),
  ProfileSetting::new("profiling", "debug", ExpectedTomlValue::Boolean(true)),
  ProfileSetting::new("profiling", "strip", ExpectedTomlValue::String("none")),
];
const REQUIRED_DOWNLOAD_METADATA_SNIPPETS: [(&str, &str); 13] = [
  ("runtime pattern group", "const cadderRuntimeAssetPatterns"),
  (
    "runtime Windows archive",
    r#"'windows-x64': /^cadder-v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?-windows-x64\.zip$/"#,
  ),
  (
    "runtime macOS Apple Silicon archive",
    r#"'macos-arm64': /^cadder-v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?-macos-arm64\.tar\.gz$/"#,
  ),
  (
    "runtime macOS Intel archive",
    r#"'macos-x64': /^cadder-v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?-macos-x64\.tar\.gz$/"#,
  ),
  (
    "runtime Linux archive",
    r#"'linux-x64': /^cadder-v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?-linux-x64\.tar\.gz$/"#,
  ),
  (
    "runtime installer pattern group",
    "const cadderRuntimeInstallerAssetPatterns",
  ),
  (
    "runtime installer Windows MSI",
    r#"'windows-x64-msi': /^cadder-runtime-.+-windows-x64\.msi$/"#,
  ),
  (
    "runtime installer macOS Apple Silicon PKG",
    r#"'macos-arm64-pkg': /^cadder-runtime-.+-macos-arm64\.pkg$/"#,
  ),
  (
    "runtime installer macOS Intel PKG",
    r#"'macos-x64-pkg': /^cadder-runtime-.+-macos-x64\.pkg$/"#,
  ),
  (
    "runtime installer Linux DEB",
    r#"'linux-x64-deb': /^cadder-runtime-.+-linux-x64\.deb$/"#,
  ),
  (
    "runtime installer Linux RPM",
    r#"'linux-x64-rpm': /^cadder-runtime-.+-linux-x64\.rpm$/"#,
  ),
  ("runtime download attribute", "data-cadder-asset"),
  (
    "runtime installer download attribute",
    "data-cadder-runtime-installer-asset",
  ),
];
const REQUIRED_VISUAL_ASSETS: [(&str, &str); 13] = [
  ("canonical banner", "assets/banner.webp"),
  ("canonical logo", "assets/logo.png"),
  ("canonical icon logo", "assets/logo-icon.png"),
  ("canonical white logo", "assets/logo-white.png"),
  (
    "docs banner pipeline copy",
    "docs/site/src/assets/banner.webp",
  ),
  ("docs logo pipeline copy", "docs/site/src/assets/logo.png"),
  ("docs favicon ico", "docs/site/public/favicon.ico"),
  ("docs favicon 16", "docs/site/public/favicon-16x16.png"),
  ("docs favicon 32", "docs/site/public/favicon-32x32.png"),
  (
    "docs android chrome 192",
    "docs/site/public/android-chrome-192x192.png",
  ),
  (
    "docs android chrome 512",
    "docs/site/public/android-chrome-512x512.png",
  ),
  (
    "docs apple touch icon",
    "docs/site/public/apple-touch-icon.png",
  ),
  ("docs web manifest", DOCS_WEBMANIFEST),
];
const CANONICAL_ASSET_COPIES: [(&str, &str, &str); 2] = [
  (
    "docs banner pipeline copy",
    "assets/banner.webp",
    "docs/site/src/assets/banner.webp",
  ),
  (
    "docs logo pipeline copy",
    "assets/logo.png",
    "docs/site/src/assets/logo.png",
  ),
];
const OBSOLETE_SCAFFOLD_ASSETS: [(&str, &str); 0] = [];
const SAMPLE_CADDER_TOML: &str = r#"# Cadder configuration template.
# Keep this file beside the Cadder executables.

[caddy]
# real_command = "caddy-real"
# real_path = "/absolute/path/to/caddy"

"#;

#[derive(Debug, Clone, Copy)]
struct ProfileSetting {
  profile: &'static str,
  key: &'static str,
  expected: ExpectedTomlValue,
}

impl ProfileSetting {
  const fn new(profile: &'static str, key: &'static str, expected: ExpectedTomlValue) -> Self {
    Self {
      profile,
      key,
      expected,
    }
  }
}

#[derive(Debug, Clone, Copy)]
enum ExpectedTomlValue {
  Boolean(bool),
  Integer(i64),
  String(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkspaceMemberContract {
  path: &'static str,
  package: &'static str,
  classification: WorkspaceMemberClassification,
  runtime_release_package: bool,
}

impl WorkspaceMemberContract {
  const fn new(
    path: &'static str,
    package: &'static str,
    classification: WorkspaceMemberClassification,
    runtime_release_package: bool,
  ) -> Self {
    Self {
      path,
      package,
      classification,
      runtime_release_package,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceMemberClassification {
  Daemon,
  Shim,
  OperatorClient,
  SharedProtocolApi,
  DocsTooling,
}

impl WorkspaceMemberClassification {
  fn label(self) -> &'static str {
    match self {
      Self::Daemon => "daemon",
      Self::Shim => "shim",
      Self::OperatorClient => "operator client",
      Self::SharedProtocolApi => "shared protocol/API",
      Self::DocsTooling => "docs/tooling",
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortableTopology {
  Runtime,
}

impl PortableTopology {
  const DEFAULT: Self = Self::Runtime;

  fn parse(value: &str) -> Result<Self> {
    match value {
      "runtime" => Ok(Self::Runtime),
      other => bail!("unknown portable topology `{other}`; expected runtime"),
    }
  }

  fn parse_option(args: &[String]) -> Result<Self> {
    optional_string_option(args, "--topology")?
      .map_or(Ok(Self::DEFAULT), |value| Self::parse(&value))
  }

  fn name(self) -> &'static str {
    match self {
      Self::Runtime => "runtime",
    }
  }

  fn binaries(self) -> &'static [&'static str] {
    match self {
      Self::Runtime => &RUNTIME_PORTABLE_BINARIES,
    }
  }

  fn release_packages(self) -> &'static [&'static str] {
    match self {
      Self::Runtime => &RUNTIME_RELEASE_PACKAGES,
    }
  }

  fn includes_runtime(self) -> bool {
    true
  }

  fn package_archive_stem(self, version: &str, platform: &str) -> Result<String> {
    match self {
      Self::Runtime => Ok(format!("cadder-{version}-{platform}")),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleasePlatform {
  WindowsX64,
  LinuxX64,
  MacosX64,
  MacosArm64,
}

impl ReleasePlatform {
  const ALL: [Self; 4] = [
    Self::WindowsX64,
    Self::LinuxX64,
    Self::MacosX64,
    Self::MacosArm64,
  ];

  fn parse(value: &str) -> Result<Self> {
    match value {
      "windows-x64" => Ok(Self::WindowsX64),
      "linux-x64" => Ok(Self::LinuxX64),
      "macos-x64" => Ok(Self::MacosX64),
      "macos-arm64" => Ok(Self::MacosArm64),
      other => bail!(
        "unknown release platform `{other}`; expected windows-x64, linux-x64, macos-x64, or macos-arm64"
      ),
    }
  }

  fn parse_option(args: &[String], target: Option<&str>) -> Result<Self> {
    optional_string_option(args, "--platform")?.map_or_else(
      || Self::infer_from_target(target),
      |platform| Self::parse(&platform),
    )
  }

  fn infer_from_target(target: Option<&str>) -> Result<Self> {
    if let Some(target) = target {
      if target == "x86_64-pc-windows-msvc" {
        return Ok(Self::WindowsX64);
      }
      if target == "x86_64-unknown-linux-gnu" {
        return Ok(Self::LinuxX64);
      }
      if target == "x86_64-apple-darwin" {
        return Ok(Self::MacosX64);
      }
      if target == "aarch64-apple-darwin" {
        return Ok(Self::MacosArm64);
      }
      bail!("cannot infer release platform from target `{target}`; pass --platform explicitly");
    }

    if cfg!(target_os = "windows") {
      Ok(Self::WindowsX64)
    } else if cfg!(target_os = "linux") {
      Ok(Self::LinuxX64)
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
      Ok(Self::MacosArm64)
    } else if cfg!(target_os = "macos") {
      Ok(Self::MacosX64)
    } else {
      bail!("cannot infer release platform for this host; pass --platform explicitly")
    }
  }

  fn name(self) -> &'static str {
    match self {
      Self::WindowsX64 => "windows-x64",
      Self::LinuxX64 => "linux-x64",
      Self::MacosX64 => "macos-x64",
      Self::MacosArm64 => "macos-arm64",
    }
  }

  fn portable_archive_extension(self) -> &'static str {
    match self {
      Self::WindowsX64 => "zip",
      Self::LinuxX64 | Self::MacosX64 | Self::MacosArm64 => "tar.gz",
    }
  }

  fn uses_windows_executables(self) -> bool {
    matches!(self, Self::WindowsX64)
  }

  fn debian_architecture(self) -> Result<&'static str> {
    match self {
      Self::LinuxX64 => Ok("amd64"),
      _ => bail!(
        "Debian runtime installer metadata is not defined for {}",
        self.name()
      ),
    }
  }

  fn rpm_architecture(self) -> Result<&'static str> {
    match self {
      Self::LinuxX64 => Ok("x86_64"),
      _ => bail!(
        "RPM runtime installer metadata is not defined for {}",
        self.name()
      ),
    }
  }

  fn required_runtime_installer_artifact_kinds(self) -> &'static [RuntimeInstallerArtifactKind] {
    match self {
      Self::WindowsX64 => &[RuntimeInstallerArtifactKind::WindowsMsi],
      Self::LinuxX64 => &[
        RuntimeInstallerArtifactKind::LinuxDeb,
        RuntimeInstallerArtifactKind::LinuxRpm,
      ],
      Self::MacosX64 | Self::MacosArm64 => &[RuntimeInstallerArtifactKind::MacosPkg],
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeInstallerArtifactKind {
  WindowsMsi,
  LinuxDeb,
  LinuxRpm,
  MacosPkg,
}

impl RuntimeInstallerArtifactKind {
  fn label(self) -> &'static str {
    match self {
      Self::WindowsMsi => "Windows MSI runtime installer",
      Self::LinuxDeb => "Linux deb runtime package",
      Self::LinuxRpm => "Linux rpm runtime package",
      Self::MacosPkg => "macOS pkg runtime installer",
    }
  }

  fn extension(self) -> &'static str {
    match self {
      Self::WindowsMsi => "msi",
      Self::LinuxDeb => "deb",
      Self::LinuxRpm => "rpm",
      Self::MacosPkg => "pkg",
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SigningMode {
  UnsignedDryRun,
  SignedRelease,
}

impl SigningMode {
  fn parse(args: &[String]) -> Self {
    if has_flag(args, "--sign") {
      Self::SignedRelease
    } else {
      Self::UnsignedDryRun
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseAssetMode {
  DryRun,
  Publish,
}

impl ReleaseAssetMode {
  fn parse(args: &[String]) -> Result<Self> {
    optional_string_option(args, "--mode")?.map_or(Ok(Self::DryRun), |mode| match mode.as_str() {
      "dry-run" => Ok(Self::DryRun),
      "publish" => Ok(Self::Publish),
      other => bail!("unknown release asset mode `{other}`; expected dry-run or publish"),
    })
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XtaskCommandGroup {
  Validation,
  Documentation,
  Distribution,
  Verification,
  Development,
}

impl XtaskCommandGroup {
  fn label(self) -> &'static str {
    match self {
      Self::Validation => "validation",
      Self::Documentation => "documentation",
      Self::Distribution => "release and packaging",
      Self::Verification => "verification",
      Self::Development => "development",
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct XtaskCommand {
  name: &'static str,
  usage: &'static str,
  summary: &'static str,
  group: XtaskCommandGroup,
}

const XTASK_COMMANDS: &[XtaskCommand] = &[
  XtaskCommand {
    name: "check",
    usage: "check",
    summary: "Run release profile, identity, Rust, docs, and release validation.",
    group: XtaskCommandGroup::Validation,
  },
  XtaskCommand {
    name: "coverage",
    usage: "coverage [--output <path>]",
    summary: "Run cargo-llvm-cov for required crates and enforce the line coverage threshold.",
    group: XtaskCommandGroup::Validation,
  },
  XtaskCommand {
    name: "openspec-check",
    usage: "openspec-check",
    summary: "Validate OpenSpec schemas, contracts, implementation evidence, and content boundaries.",
    group: XtaskCommandGroup::Validation,
  },
  XtaskCommand {
    name: "docs-check",
    usage: "docs-check",
    summary: "Install docs dependencies and run the Astro type/content check.",
    group: XtaskCommandGroup::Documentation,
  },
  XtaskCommand {
    name: "docs-build",
    usage: "docs-build",
    summary: "Install docs dependencies and build the Starlight documentation site.",
    group: XtaskCommandGroup::Documentation,
  },
  XtaskCommand {
    name: "dist",
    usage: "dist --out <dir> [--target <triple>]",
    summary: "Build a local portable runtime layout and verify it.",
    group: XtaskCommandGroup::Distribution,
  },
  XtaskCommand {
    name: "package",
    usage: "package --out <dir> --platform <platform> [--version <semver>] [--target <triple>]",
    summary: "Build a versioned portable release archive plus checksum.",
    group: XtaskCommandGroup::Distribution,
  },
  XtaskCommand {
    name: "runtime-installer",
    usage: "runtime-installer --out <dir> [--version <semver>] [--platform <platform>] [--target <triple>] [--sign]",
    summary: "Build daemon-first native runtime installer artifacts.",
    group: XtaskCommandGroup::Distribution,
  },
  XtaskCommand {
    name: "verify-dist",
    usage: "verify-dist --dir <dir> [--target <triple>]",
    summary: "Verify a portable layout's files, binary metadata, shim info, and size summary.",
    group: XtaskCommandGroup::Verification,
  },
  XtaskCommand {
    name: "verify-runtime-installer-dist",
    usage: "verify-runtime-installer-dist --dir <dir> [--version <semver>] [--platform <platform>] [--target <triple>]",
    summary: "Verify native runtime installer artifacts, manifests, and checksums.",
    group: XtaskCommandGroup::Verification,
  },
  XtaskCommand {
    name: "verify-assets",
    usage: "verify-assets",
    summary: "Verify checked-in visual asset mapping and required docs paths.",
    group: XtaskCommandGroup::Verification,
  },
  XtaskCommand {
    name: "verify-release-assets",
    usage: "verify-release-assets --dir <dir> [--version <semver>] [--mode dry-run|publish]",
    summary: "Verify the full public release artifact matrix before upload.",
    group: XtaskCommandGroup::Verification,
  },
  XtaskCommand {
    name: "verify-release-profile",
    usage: "verify-release-profile",
    summary: "Verify root Cargo release/profile policy.",
    group: XtaskCommandGroup::Verification,
  },
  XtaskCommand {
    name: "verify-release-identity",
    usage: "verify-release-identity",
    summary: "Verify synchronized version and download metadata.",
    group: XtaskCommandGroup::Verification,
  },
  XtaskCommand {
    name: "verify-workspace-topology",
    usage: "verify-workspace-topology",
    summary: "Verify workspace members and release packages match the documented topology.",
    group: XtaskCommandGroup::Verification,
  },
  XtaskCommand {
    name: "dev-env",
    usage: "dev-env [--format powershell|bash|cmd|json]",
    summary: "Print the repeatable dev runtime environment.",
    group: XtaskCommandGroup::Development,
  },
  XtaskCommand {
    name: "dev-run",
    usage: "dev-run -- <program> [args...]",
    summary: "Run a command inside the repeatable dev runtime environment.",
    group: XtaskCommandGroup::Development,
  },
];

fn handle_global_help(args: &[String]) -> Result<bool> {
  let Some(first) = args.first().map(String::as_str) else {
    return Ok(false);
  };

  match first {
    "-h" | "--help" => {
      print!("{}", xtask_help_text());
      Ok(true)
    }
    "list" => {
      print!("{}", xtask_command_list_text());
      Ok(true)
    }
    "help" => {
      if let Some(command) = args.get(1) {
        print!("{}", xtask_command_help_text(command)?);
      } else {
        print!("{}", xtask_help_text());
      }
      Ok(true)
    }
    _ => Ok(false),
  }
}

fn xtask_help_text() -> String {
  let mut output = String::from(
    "Cadder repository task runner\n\n\
Usage:\n\
  cargo xtask <command> [args...]\n\
  cargo run -p xtask -- <command> [args...]\n\n\
Default command:\n\
  check\n\n\
Commands:\n",
  );

  let groups = [
    XtaskCommandGroup::Validation,
    XtaskCommandGroup::Documentation,
    XtaskCommandGroup::Distribution,
    XtaskCommandGroup::Verification,
    XtaskCommandGroup::Development,
  ];
  for group in groups {
    output.push_str(&format!("  {}:\n", group.label()));
    for command in XTASK_COMMANDS
      .iter()
      .filter(|command| command.group == group)
    {
      output.push_str(&format!("    {:<31} {}\n", command.name, command.summary));
    }
  }

  output.push_str("\nRun `cargo xtask help <command>` for command usage.\n");
  output
}

fn xtask_command_list_text() -> String {
  let mut output = String::new();
  for command in XTASK_COMMANDS {
    output.push_str(command.name);
    output.push('\n');
  }
  output
}

fn xtask_command_help_text(command_name: &str) -> Result<String> {
  let command = XTASK_COMMANDS
    .iter()
    .find(|command| command.name == command_name)
    .ok_or_else(|| anyhow::anyhow!("unknown xtask command `{command_name}`"))?;

  Ok(format!(
    "{}\n\nUsage:\n  cargo xtask {}\n\n{}\n",
    command.name, command.usage, command.summary
  ))
}

fn main() -> Result<()> {
  let mut args = env::args().skip(1).collect::<Vec<_>>();
  if handle_global_help(&args)? {
    return Ok(());
  }

  let command = if args.is_empty() {
    "check".to_string()
  } else {
    args.remove(0)
  };
  match command.as_str() {
    "check" => check(),
    "coverage" => coverage(CoverageOptions::parse(args)?),
    "openspec-check" => openspec_check_command(),
    "docs-check" => docs_check(),
    "docs-build" => docs_build(),
    "dev-env" => dev_env_command(args),
    "dev-run" => dev_run(args),
    "dist" => dist(DistOptions::parse(args)?),
    "verify-dist" => verify_dist(&VerifyDistOptions::parse(args)?),
    "verify-assets" => verify_assets(),
    "verify-release-assets" => verify_release_assets(&ReleaseAssetsOptions::parse(args)?),
    "verify-release-identity" => verify_release_identity(),
    "verify-release-profile" => verify_release_profile(),
    "verify-workspace-topology" => verify_workspace_topology(),
    "verify-runtime-installer-dist" => {
      verify_runtime_installer_dist(&VerifyRuntimeInstallerDistOptions::parse(args)?)
    }
    "package" => package(PackageOptions::parse(args)?),
    "runtime-installer" => runtime_installer(RuntimeInstallerOptions::parse(args)?),
    other => bail!("unknown xtask command `{other}`; run `cargo xtask --help` for commands"),
  }
}

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

fn dev_env_command(args: Vec<String>) -> Result<()> {
  let format = DevEnvFormat::parse(&args)?;
  DevEnvironment::for_workspace().print(format)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DevEnvFormat {
  Powershell,
  Bash,
  Cmd,
  Json,
}

impl DevEnvFormat {
  fn parse(args: &[String]) -> Result<Self> {
    let format = optional_string_option(args, "--format")?.unwrap_or_else(default_dev_env_format);
    match format.as_str() {
      "powershell" | "pwsh" => Ok(Self::Powershell),
      "bash" | "sh" => Ok(Self::Bash),
      "cmd" => Ok(Self::Cmd),
      "json" => Ok(Self::Json),
      other => bail!("unknown dev-env format `{other}`; expected powershell, bash, cmd, or json"),
    }
  }
}

fn default_dev_env_format() -> String {
  if cfg!(windows) {
    "powershell".to_string()
  } else {
    "bash".to_string()
  }
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

fn verify_release_profile() -> Result<()> {
  verify_release_profile_manifest(Path::new(WORKSPACE_MANIFEST))?;
  println!("verified release profile policy");
  Ok(())
}

fn verify_release_identity() -> Result<()> {
  verify_assets()?;
  verify_release_identity_files(
    Path::new(WORKSPACE_MANIFEST),
    &workspace_root().join(DOCS_DOWNLOAD_SCRIPT),
  )?;
  println!("verified release identity metadata");
  Ok(())
}

fn verify_assets() -> Result<()> {
  verify_assets_at(&workspace_root())?;
  println!("verified visual asset contract");
  Ok(())
}

fn verify_workspace_topology() -> Result<()> {
  verify_workspace_topology_manifest(Path::new(WORKSPACE_MANIFEST))?;
  println!("verified workspace topology contract");
  Ok(())
}

fn verify_assets_at(root: &Path) -> Result<()> {
  verify_required_asset_paths(root, &REQUIRED_VISUAL_ASSETS)?;
  verify_canonical_asset_copies(root, &CANONICAL_ASSET_COPIES)?;
  verify_obsolete_assets_absent(root, &OBSOLETE_SCAFFOLD_ASSETS)?;
  verify_docs_webmanifest(root, &root.join(DOCS_WEBMANIFEST))
}

fn verify_release_identity_files(workspace_manifest: &Path, download_script: &Path) -> Result<()> {
  workspace_package_version_from_manifest(workspace_manifest)?;
  verify_docs_download_metadata(download_script)
}

fn verify_docs_download_metadata(download_script: &Path) -> Result<()> {
  let contents = fs::read_to_string(download_script)
    .with_context(|| format!("read {}", download_script.display()))?;
  for (label, snippet) in REQUIRED_DOWNLOAD_METADATA_SNIPPETS {
    if !contents.contains(snippet) {
      bail!(
        "download metadata for {label} is missing expected snippet {snippet:?} in {}",
        download_script.display()
      );
    }
  }
  Ok(())
}

fn verify_required_asset_paths(root: &Path, assets: &[(&str, &str)]) -> Result<()> {
  for (label, relative_path) in assets {
    let path = root.join(relative_path);
    if !path.is_file() {
      bail!(
        "{label} is missing at {}",
        normalize_repo_path(relative_path)
      );
    }
  }
  Ok(())
}

fn verify_canonical_asset_copies(root: &Path, copies: &[(&str, &str, &str)]) -> Result<()> {
  for (label, canonical_path, copy_path) in copies {
    let canonical = root.join(canonical_path);
    let copy = root.join(copy_path);
    let canonical_hash = compute_sha256(&canonical)?;
    let copy_hash = compute_sha256(&copy)?;
    if canonical_hash != copy_hash {
      bail!(
        "{label} at {} differs from canonical asset {}",
        normalize_repo_path(copy_path),
        normalize_repo_path(canonical_path)
      );
    }
  }
  Ok(())
}

fn verify_obsolete_assets_absent(root: &Path, assets: &[(&str, &str)]) -> Result<()> {
  for (label, relative_path) in assets {
    if root.join(relative_path).exists() {
      bail!(
        "obsolete scaffold asset {label} must not be checked in at {}",
        normalize_repo_path(relative_path)
      );
    }
  }
  Ok(())
}

fn verify_docs_webmanifest(root: &Path, manifest_path: &Path) -> Result<()> {
  let manifest = read_json_value(manifest_path)?;
  ensure_json_string(
    manifest_path,
    &manifest,
    &["name"],
    "Cadder",
    "web manifest name",
  )?;
  ensure_json_string(
    manifest_path,
    &manifest,
    &["short_name"],
    "Cadder",
    "web manifest short_name",
  )?;

  let icons = manifest
    .get("icons")
    .and_then(JsonValue::as_array)
    .with_context(|| format!("icons missing in {}", manifest_path.display()))?;
  if icons.is_empty() {
    bail!("icons is empty in {}", manifest_path.display());
  }

  let public_root = root.join("docs/site/public");
  for icon in icons {
    let src = json_path_string(manifest_path, icon, &["src"], "web manifest icon src")?;
    let relative_path = src.strip_prefix('/').unwrap_or(src);
    let path = public_root.join(relative_path);
    if !path.is_file() {
      bail!(
        "web manifest icon {} is missing at {}",
        src,
        normalize_repo_path(&path_relative_to(root, &path))
      );
    }
  }
  Ok(())
}

fn normalize_repo_path(path: &str) -> String {
  path.replace('\\', "/")
}

fn path_relative_to(root: &Path, path: &Path) -> String {
  path
    .strip_prefix(root)
    .unwrap_or(path)
    .display()
    .to_string()
}

fn read_json_value(path: &Path) -> Result<JsonValue> {
  let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
  serde_json::from_str(&contents).with_context(|| format!("parse JSON {}", path.display()))
}

fn ensure_json_string(
  path: &Path,
  root: &JsonValue,
  keys: &[&str],
  expected: &str,
  label: &str,
) -> Result<()> {
  let value = json_path_string(path, root, keys, label)?;
  if value == expected {
    return Ok(());
  }

  bail!(
    "{label} expected {expected:?}, found {value:?} in {}",
    path.display()
  )
}

fn json_path_string<'a>(
  path: &Path,
  root: &'a JsonValue,
  keys: &[&str],
  label: &str,
) -> Result<&'a str> {
  let mut value = root;
  for key in keys {
    value = value.get(*key).with_context(|| {
      format!(
        "{label} missing key `{}` in {}",
        keys.join("."),
        path.display()
      )
    })?;
  }
  value
    .as_str()
    .with_context(|| format!("{label} is not a string in {}", path.display()))
}

fn verify_release_profile_manifest(manifest: &Path) -> Result<()> {
  let manifest_table = read_toml_table(manifest)?;
  for setting in RELEASE_PROFILE_SETTINGS
    .iter()
    .chain(PROFILING_PROFILE_SETTINGS.iter())
  {
    ensure_profile_setting(manifest, &manifest_table, setting)?;
  }
  Ok(())
}

fn verify_workspace_topology_manifest(manifest: &Path) -> Result<()> {
  let root = manifest
    .parent()
    .with_context(|| format!("workspace manifest has no parent: {}", manifest.display()))?;
  let manifest_table = read_toml_table(manifest)?;
  let actual_members = workspace_members_from_manifest(manifest, &manifest_table)?;

  ensure_workspace_members_match(manifest, &actual_members)?;
  ensure_workspace_member_manifests_match(root)?;
  ensure_runtime_release_packages_match_contract()?;
  Ok(())
}

fn workspace_members_from_manifest(manifest: &Path, root: &Table) -> Result<BTreeSet<String>> {
  let workspace = root
    .get("workspace")
    .and_then(Value::as_table)
    .with_context(|| format!("workspace table missing in {}", manifest.display()))?;
  let members = workspace
    .get("members")
    .and_then(Value::as_array)
    .with_context(|| format!("workspace.members array missing in {}", manifest.display()))?;

  let mut result = BTreeSet::new();
  for member in members {
    let member = member
      .as_str()
      .with_context(|| format!("workspace member is not a string in {}", manifest.display()))?;
    let normalized = normalize_repo_path(member);
    if !result.insert(normalized.clone()) {
      bail!(
        "duplicate workspace member {normalized:?} in {}",
        manifest.display()
      );
    }
  }
  Ok(result)
}

fn ensure_workspace_members_match(manifest: &Path, actual: &BTreeSet<String>) -> Result<()> {
  let expected = expected_workspace_members();
  if actual == &expected {
    return Ok(());
  }

  bail!(
    "workspace members in {} do not match documented topology; missing: {}; unexpected: {}; documented topology: {}",
    manifest.display(),
    format_string_set(expected.difference(actual)),
    format_string_set(actual.difference(&expected)),
    documented_workspace_topology()
  )
}

fn ensure_workspace_member_manifests_match(root: &Path) -> Result<()> {
  for contract in WORKSPACE_MEMBER_CONTRACTS {
    let manifest = root.join(contract.path).join("Cargo.toml");
    let package_name = workspace_member_package_name(&manifest)?;
    if package_name != contract.package {
      bail!(
        "{} is classified as {} but package name must be {:?}, found {:?}",
        contract.path,
        contract.classification.label(),
        contract.package,
        package_name
      );
    }
  }
  Ok(())
}

fn workspace_member_package_name(manifest: &Path) -> Result<String> {
  let manifest_table = read_toml_table(manifest)?;
  manifest_table
    .get("package")
    .and_then(Value::as_table)
    .and_then(|package| package.get("name"))
    .and_then(Value::as_str)
    .map(ToOwned::to_owned)
    .with_context(|| format!("package.name missing in {}", manifest.display()))
}

fn ensure_runtime_release_packages_match_contract() -> Result<()> {
  let actual = RUNTIME_RELEASE_PACKAGES
    .iter()
    .map(|package| (*package).to_string())
    .collect::<BTreeSet<_>>();
  let expected = WORKSPACE_MEMBER_CONTRACTS
    .iter()
    .filter(|contract| contract.runtime_release_package)
    .map(|contract| contract.package.to_string())
    .collect::<BTreeSet<_>>();

  if actual == expected {
    return Ok(());
  }

  bail!(
    "runtime release packages do not match documented topology; missing: {}; unexpected: {}",
    format_string_set(expected.difference(&actual)),
    format_string_set(actual.difference(&expected))
  )
}

fn expected_workspace_members() -> BTreeSet<String> {
  WORKSPACE_MEMBER_CONTRACTS
    .iter()
    .map(|contract| contract.path.to_string())
    .collect()
}

fn documented_workspace_topology() -> String {
  WORKSPACE_MEMBER_CONTRACTS
    .iter()
    .map(|contract| {
      format!(
        "{}={} ({})",
        contract.path,
        contract.package,
        contract.classification.label()
      )
    })
    .collect::<Vec<_>>()
    .join(", ")
}

fn format_string_set<'a>(items: impl Iterator<Item = &'a String>) -> String {
  let items = items.map(String::as_str).collect::<Vec<_>>();
  if items.is_empty() {
    "none".to_string()
  } else {
    items.join(", ")
  }
}

fn read_toml_table(path: &Path) -> Result<Table> {
  let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
  contents
    .parse::<Table>()
    .with_context(|| format!("parse TOML {}", path.display()))
}

fn ensure_profile_setting(manifest: &Path, root: &Table, setting: &ProfileSetting) -> Result<()> {
  let profile = root
    .get("profile")
    .and_then(Value::as_table)
    .with_context(|| format!("profile table missing in {}", manifest.display()))?;
  let profile_table = profile
    .get(setting.profile)
    .and_then(Value::as_table)
    .with_context(|| {
      format!(
        "profile.{} table missing in {}",
        setting.profile,
        manifest.display()
      )
    })?;
  let value = profile_table.get(setting.key).with_context(|| {
    format!(
      "profile.{}.{} missing in {}",
      setting.profile,
      setting.key,
      manifest.display()
    )
  })?;

  if setting.expected.matches(value) {
    return Ok(());
  }

  bail!(
    "profile.{}.{} expected {}, found {} in {}",
    setting.profile,
    setting.key,
    setting.expected.describe(),
    describe_toml_value(value),
    manifest.display()
  )
}

impl ExpectedTomlValue {
  fn matches(self, value: &Value) -> bool {
    match self {
      Self::Boolean(expected) => value.as_bool() == Some(expected),
      Self::Integer(expected) => value.as_integer() == Some(expected),
      Self::String(expected) => value.as_str() == Some(expected),
    }
  }

  fn describe(self) -> String {
    match self {
      Self::Boolean(value) => value.to_string(),
      Self::Integer(value) => value.to_string(),
      Self::String(value) => format!("{value:?}"),
    }
  }
}

fn describe_toml_value(value: &Value) -> String {
  match value {
    Value::String(value) => format!("{value:?}"),
    Value::Integer(value) => value.to_string(),
    Value::Float(value) => value.to_string(),
    Value::Boolean(value) => value.to_string(),
    Value::Datetime(value) => value.to_string(),
    Value::Array(_) => "array".to_string(),
    Value::Table(_) => "table".to_string(),
  }
}

fn dist(options: DistOptions) -> Result<()> {
  verify_release_profile()?;
  dist_with_builder(options, build_release_binaries)
}

fn dist_with_builder(
  options: DistOptions,
  build_binaries: impl FnOnce(Option<&str>, PortableTopology) -> Result<()>,
) -> Result<()> {
  build_binaries(options.target.as_deref(), options.topology)?;
  prepare_dist_dir(&options.out_dir, options.target.as_deref())?;
  for binary in options.topology.binaries() {
    let source = release_binary_path(binary, options.target.as_deref());
    let target = options
      .out_dir
      .join(exe_name(binary, options.target.as_deref()));
    fs::copy(&source, &target)
      .with_context(|| format!("copy {} to {}", source.display(), target.display()))?;
  }
  if options.topology.includes_runtime() {
    fs::write(options.out_dir.join("cadder.toml"), SAMPLE_CADDER_TOML)
      .with_context(|| format!("write {}", options.out_dir.join("cadder.toml").display()))?;
  }

  verify_dist(&VerifyDistOptions {
    dir: options.out_dir,
    target: options.target,
    topology: options.topology,
  })
}

fn prepare_dist_dir(dir: &Path, target: Option<&str>) -> Result<()> {
  fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
  for binary in RUNTIME_PORTABLE_BINARIES {
    remove_file_if_exists(&dir.join(exe_name(binary, target)))?;
  }
  remove_file_if_exists(&dir.join("cadder.toml"))
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
  match fs::metadata(path) {
    Ok(metadata) if metadata.is_file() => {
      fs::remove_file(path).with_context(|| format!("remove {}", path.display()))
    }
    Ok(_) => bail!(
      "expected removable portable file, found non-file {}",
      path.display()
    ),
    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
  }
}

fn verify_dist(options: &VerifyDistOptions) -> Result<()> {
  for binary in options.topology.binaries() {
    let path = portable_binary_path(options, binary);
    if !path.is_file() {
      bail!("portable binary missing: {}", path.display());
    }
  }
  if options.topology.includes_runtime() {
    let config = options.dir.join("cadder.toml");
    if !config.is_file() {
      bail!(
        "portable sample configuration missing: {}",
        config.display()
      );
    }
  }

  verify_dist_file_set(options)?;

  for binary in options.topology.binaries() {
    let path = portable_binary_path(options, binary);
    verify_portable_binary_command(binary, &path, "--help")?;
    verify_portable_binary_command(binary, &path, "--version")?;
  }

  if options.topology.includes_runtime() {
    let shim = portable_binary_path(options, "caddy");
    let stdout = run_portable_binary_command("caddy", &shim, "--cadder-shim-info")?;
    if !stdout.contains("\"role\":\"caddy-shim\"") && !stdout.contains("\"role\": \"caddy-shim\"") {
      bail!("caddy --cadder-shim-info did not report the Cadder shim role");
    }
  }

  let summary_label = format!("portable {} dist", options.topology.name());
  print_artifact_summary(&summary_label, &options.dir)?;

  Ok(())
}

fn verify_dist_file_set(options: &VerifyDistOptions) -> Result<()> {
  let mut expected = BTreeSet::new();
  for binary in options.topology.binaries() {
    expected.insert(exe_name(binary, options.target.as_deref()));
  }
  if options.topology.includes_runtime() {
    expected.insert("cadder.toml".to_string());
  }

  let mut actual = BTreeSet::new();
  for entry in sorted_dir_entries(&options.dir)? {
    if entry.file_type()?.is_file() {
      actual.insert(entry.file_name().to_string_lossy().to_string());
    }
  }

  if actual != expected {
    bail!(
      "portable dist {} contains unexpected file set: expected {:?}, found {:?}",
      options.dir.display(),
      expected,
      actual
    );
  }

  Ok(())
}

fn portable_binary_path(options: &VerifyDistOptions, binary: &str) -> PathBuf {
  options
    .dir
    .join(exe_name(binary, options.target.as_deref()))
}

fn verify_portable_binary_command(binary: &str, path: &Path, argument: &str) -> Result<()> {
  let stdout = run_portable_binary_command(binary, path, argument)?;
  if stdout.trim().is_empty() {
    bail!("{binary} {argument} produced empty stdout");
  }
  Ok(())
}

fn run_portable_binary_command(binary: &str, path: &Path, argument: &str) -> Result<String> {
  let mut command = Command::new(path);
  configure_hidden_child(&mut command);
  let output = command
    .arg(argument)
    .stdin(Stdio::null())
    .output()
    .with_context(|| format!("run {} {argument}", path.display()))?;
  if !output.status.success() {
    bail!("{binary} {argument} failed with {}", output.status);
  }
  Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn package(options: PackageOptions) -> Result<()> {
  verify_release_profile()?;
  package_with_dist(options, |dist_options| {
    dist_with_builder(dist_options, build_release_binaries)
  })
}

fn package_with_dist(
  options: PackageOptions,
  create_dist: impl FnOnce(DistOptions) -> Result<()>,
) -> Result<()> {
  let archive_stem = options
    .topology
    .package_archive_stem(&options.version, &options.platform)?;
  let layout_parent = options.out_dir.join("layouts");
  let layout_dir = layout_parent.join(&archive_stem);
  if layout_dir.exists() {
    fs::remove_dir_all(&layout_dir).with_context(|| format!("remove {}", layout_dir.display()))?;
  }

  create_dist(DistOptions {
    out_dir: layout_dir,
    target: options.target,
    topology: options.topology,
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
  let checksum = write_sha256_file(&archive_path, &checksum_path)?;

  println!("created {}", archive_path.display());
  println!("created {}", checksum_path.display());
  println!("archive sha256 {checksum}");
  let summary_label = format!("portable {} package", options.topology.name());
  print_artifact_summary(&summary_label, &options.out_dir)?;
  Ok(())
}

fn runtime_installer(options: RuntimeInstallerOptions) -> Result<()> {
  verify_release_profile()?;
  if options.signing_mode == SigningMode::SignedRelease {
    verify_runtime_installer_signing_inputs(options.platform)?;
  }

  let work_dir = tempdir().context("create runtime installer work directory")?;
  let layout_dir = work_dir.path().join("runtime-layout");
  dist_with_builder(
    DistOptions {
      out_dir: layout_dir.clone(),
      target: options.target.clone(),
      topology: PortableTopology::Runtime,
    },
    build_release_binaries,
  )?;

  let payload_root = work_dir.path().join("payload-root");
  prepare_runtime_installer_payload(
    &layout_dir,
    &payload_root,
    options.target.as_deref(),
    options.platform,
  )?;

  remove_dir_if_exists(&options.out_dir)?;
  fs::create_dir_all(&options.out_dir)
    .with_context(|| format!("create {}", options.out_dir.display()))?;

  let artifacts = build_runtime_installer_artifacts(&payload_root, &layout_dir, &options)?;
  for artifact in &artifacts {
    let checksum_path = checksum_path_for(artifact)?;
    write_sha256_file(artifact, &checksum_path)?;
    write_runtime_installer_manifest(artifact, &options)?;
  }

  verify_runtime_installer_dist(&VerifyRuntimeInstallerDistOptions {
    dir: options.out_dir,
    version: options.version,
    platform: options.platform,
  })
}

fn prepare_runtime_installer_payload(
  layout_dir: &Path,
  payload_root: &Path,
  target: Option<&str>,
  platform: ReleasePlatform,
) -> Result<()> {
  remove_dir_if_exists(payload_root)?;
  match platform {
    ReleasePlatform::WindowsX64 => copy_dir_contents(
      layout_dir,
      &payload_root.join("ProgramFiles").join("Cadder"),
    ),
    ReleasePlatform::LinuxX64 | ReleasePlatform::MacosX64 | ReleasePlatform::MacosArm64 => {
      let bin_dir = payload_root.join("usr").join("local").join("bin");
      let config_dir = payload_root
        .join("usr")
        .join("local")
        .join("etc")
        .join("cadder");
      fs::create_dir_all(&bin_dir).with_context(|| format!("create {}", bin_dir.display()))?;
      fs::create_dir_all(&config_dir)
        .with_context(|| format!("create {}", config_dir.display()))?;
      for binary in RUNTIME_PORTABLE_BINARIES {
        let source = layout_dir.join(exe_name(binary, target));
        let target_path = bin_dir.join(binary);
        fs::copy(&source, &target_path)
          .with_context(|| format!("copy {} to {}", source.display(), target_path.display()))?;
      }
      fs::copy(
        layout_dir.join("cadder.toml"),
        config_dir.join("cadder.toml"),
      )
      .context("copy runtime installer sample config")?;
      Ok(())
    }
  }
}

fn build_runtime_installer_artifacts(
  payload_root: &Path,
  windows_layout_dir: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<Vec<PathBuf>> {
  let mut artifacts = Vec::new();
  for kind in options.platform.required_runtime_installer_artifact_kinds() {
    let artifact = options.out_dir.join(runtime_installer_artifact_name(
      &options.version,
      options.platform,
      *kind,
    ));
    match kind {
      RuntimeInstallerArtifactKind::WindowsMsi => {
        build_windows_runtime_msi(windows_layout_dir, &artifact, options)?
      }
      RuntimeInstallerArtifactKind::LinuxDeb => {
        build_linux_runtime_deb(payload_root, &artifact, options)?
      }
      RuntimeInstallerArtifactKind::LinuxRpm => {
        build_linux_runtime_rpm(payload_root, &artifact, options)?
      }
      RuntimeInstallerArtifactKind::MacosPkg => {
        build_macos_runtime_pkg(payload_root, &artifact, options)?
      }
    }
    if !artifact.is_file() {
      bail!("{} did not create {}", kind.label(), artifact.display());
    }
    artifacts.push(artifact);
  }
  Ok(artifacts)
}

fn build_windows_runtime_msi(
  layout_dir: &Path,
  artifact: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<()> {
  let wxs_path = artifact.with_extension("wxs");
  fs::write(
    &wxs_path,
    windows_runtime_wxs(layout_dir, &options.version, options.target.as_deref())?,
  )
  .with_context(|| format!("write {}", wxs_path.display()))?;
  let wxs = path_argument(&wxs_path)?;
  let output = path_argument(artifact)?;
  run("wix", &["build", &wxs, "-arch", "x64", "-o", &output])?;
  if options.signing_mode == SigningMode::SignedRelease {
    sign_windows_artifact(artifact)?;
  }
  fs::remove_file(&wxs_path).with_context(|| format!("remove {}", wxs_path.display()))?;
  Ok(())
}

fn windows_runtime_wxs(layout_dir: &Path, version: &str, target: Option<&str>) -> Result<String> {
  let mut components = String::new();
  let mut component_refs = String::new();
  for binary in RUNTIME_PORTABLE_BINARIES {
    let file_name = exe_name(binary, target);
    let source = xml_escape(&path_argument(&layout_dir.join(&file_name))?);
    let component_id = windows_installer_id(&format!("cmp_{file_name}"));
    let file_id = windows_installer_id(&format!("file_{file_name}"));
    components.push_str(&format!(
      r#"
        <Component Id="{component_id}" Guid="*">
          <File Id="{file_id}" Source="{source}" KeyPath="yes" />
        </Component>"#
    ));
    component_refs.push_str(&format!(r#"<ComponentRef Id="{component_id}" />"#));
  }
  let config_source = xml_escape(&path_argument(&layout_dir.join("cadder.toml"))?);
  components.push_str(&format!(
    r#"
        <Component Id="cmp_cadder_toml" Guid="*">
          <File Id="file_cadder_toml" Source="{config_source}" KeyPath="yes" />
        </Component>"#
  ));
  component_refs.push_str(r#"<ComponentRef Id="cmp_cadder_toml" />"#);

  Ok(format!(
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="{product}" Manufacturer="{manufacturer}" Version="{version}" UpgradeCode="{upgrade_code}" Scope="perMachine">
    <MajorUpgrade DowngradeErrorMessage="A newer version of {product} is already installed." />
    <MediaTemplate EmbedCab="yes" />
    <Feature Id="RuntimeFeature" Title="{product}" Level="1">
      <ComponentGroupRef Id="RuntimeComponents" />
    </Feature>
  </Package>
  <Fragment>
    <StandardDirectory Id="ProgramFiles64Folder">
      <Directory Id="INSTALLFOLDER" Name="Cadder">
{components}
      </Directory>
    </StandardDirectory>
  </Fragment>
  <Fragment>
    <ComponentGroup Id="RuntimeComponents">
      {component_refs}
    </ComponentGroup>
  </Fragment>
</Wix>
"#,
    product = RUNTIME_INSTALLER_PRODUCT_NAME,
    manufacturer = RUNTIME_INSTALLER_MANUFACTURER,
    upgrade_code = WINDOWS_RUNTIME_INSTALLER_UPGRADE_CODE,
  ))
}

fn build_linux_runtime_deb(
  payload_root: &Path,
  artifact: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<()> {
  let control_dir = payload_root.join("DEBIAN");
  fs::create_dir_all(&control_dir).with_context(|| format!("create {}", control_dir.display()))?;
  let control = format!(
    "Package: {package}\nVersion: {version}\nSection: devel\nPriority: optional\nArchitecture: {arch}\nMaintainer: Cadder Maintainers <noreply@example.com>\nDescription: Cadder daemon-first runtime\n Cadder coordinates local Caddy reverse proxies through a per-user daemon, CLI, and PATH-facing Caddy shim.\n",
    package = RUNTIME_INSTALLER_PACKAGE_NAME,
    version = options.version,
    arch = options.platform.debian_architecture()?,
  );
  fs::write(control_dir.join("control"), control)
    .with_context(|| format!("write {}", control_dir.join("control").display()))?;
  let payload = path_argument(payload_root)?;
  let output = path_argument(artifact)?;
  run(
    "dpkg-deb",
    &["--build", "--root-owner-group", &payload, &output],
  )
}

fn build_linux_runtime_rpm(
  payload_root: &Path,
  artifact: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<()> {
  let top_dir = artifact.with_extension("rpmbuild");
  remove_dir_if_exists(&top_dir)?;
  for name in ["BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"] {
    fs::create_dir_all(top_dir.join(name))
      .with_context(|| format!("create {}", top_dir.join(name).display()))?;
  }
  let spec_path = top_dir.join("SPECS").join("cadder-runtime.spec");
  fs::write(
    &spec_path,
    linux_runtime_rpm_spec(payload_root, &options.version, options.platform)?,
  )
  .with_context(|| format!("write {}", spec_path.display()))?;
  let top = path_argument(&top_dir)?;
  let spec = path_argument(&spec_path)?;
  run(
    "rpmbuild",
    &[
      "-bb",
      "--define",
      &format!("_topdir {top}"),
      "--define",
      "_build_id_links none",
      &spec,
    ],
  )?;
  let built_rpm = top_dir
    .join("RPMS")
    .join(options.platform.rpm_architecture()?)
    .join(format!(
      "{package}-{version}-1.{arch}.rpm",
      package = RUNTIME_INSTALLER_PACKAGE_NAME,
      version = options.version,
      arch = options.platform.rpm_architecture()?,
    ));
  fs::copy(&built_rpm, artifact)
    .with_context(|| format!("copy {} to {}", built_rpm.display(), artifact.display()))?;
  remove_dir_if_exists(&top_dir)
}

fn linux_runtime_rpm_spec(
  payload_root: &Path,
  version: &str,
  platform: ReleasePlatform,
) -> Result<String> {
  let payload = path_argument(payload_root)?;
  Ok(format!(
    r#"Name: {package}
Version: {version}
Release: 1
Summary: Cadder daemon-first runtime
License: MIT
BuildArch: {arch}
AutoReqProv: no

%description
Cadder coordinates local Caddy reverse proxies through a per-user daemon, CLI,
and PATH-facing Caddy shim.

%prep

%build

%install
mkdir -p "%{{buildroot}}/usr/local/bin"
mkdir -p "%{{buildroot}}/usr/local/etc/cadder"
cp -a "{payload}/usr/local/bin/." "%{{buildroot}}/usr/local/bin/"
cp -a "{payload}/usr/local/etc/cadder/." "%{{buildroot}}/usr/local/etc/cadder/"

%files
/usr/local/bin/cadderd
/usr/local/bin/cadder
/usr/local/bin/caddy
/usr/local/etc/cadder/cadder.toml
"#,
    package = RUNTIME_INSTALLER_PACKAGE_NAME,
    arch = platform.rpm_architecture()?,
  ))
}

fn build_macos_runtime_pkg(
  payload_root: &Path,
  artifact: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<()> {
  let payload = path_argument(payload_root)?;
  let output = path_argument(artifact)?;
  let mut args = vec![
    "--root".to_string(),
    payload,
    "--identifier".to_string(),
    RUNTIME_INSTALLER_IDENTIFIER.to_string(),
    "--version".to_string(),
    options.version.clone(),
    "--install-location".to_string(),
    "/".to_string(),
  ];
  if options.signing_mode == SigningMode::SignedRelease {
    args.push("--sign".to_string());
    args.push(required_env(MACOS_INSTALLER_SIGNING_IDENTITY_ENV)?);
  }
  args.push(output);
  let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
  run("pkgbuild", &refs)
}

fn sign_windows_artifact(artifact: &Path) -> Result<()> {
  let cert = required_env(WINDOWS_SIGNTOOL_CERT_PATH_ENV)?;
  let password = required_env(WINDOWS_SIGNTOOL_CERT_PASSWORD_ENV)?;
  let timestamp = env::var(WINDOWS_SIGNTOOL_TIMESTAMP_URL_ENV)
    .unwrap_or_else(|_| "http://timestamp.digicert.com".to_string());
  let artifact = path_argument(artifact)?;
  run(
    "signtool",
    &[
      "sign", "/fd", "sha256", "/td", "sha256", "/tr", &timestamp, "/f", &cert, "/p", &password,
      &artifact,
    ],
  )
}

fn verify_runtime_installer_signing_inputs(platform: ReleasePlatform) -> Result<()> {
  match platform {
    ReleasePlatform::WindowsX64 => {
      required_env(WINDOWS_SIGNTOOL_CERT_PATH_ENV)?;
      required_env(WINDOWS_SIGNTOOL_CERT_PASSWORD_ENV)?;
    }
    ReleasePlatform::MacosX64 | ReleasePlatform::MacosArm64 => {
      required_env(MACOS_INSTALLER_SIGNING_IDENTITY_ENV)?;
    }
    ReleasePlatform::LinuxX64 => {}
  }
  Ok(())
}

fn required_env(name: &str) -> Result<String> {
  let value = env::var(name).with_context(|| format!("{name} is required"))?;
  if value.trim().is_empty() {
    bail!("{name} must not be empty");
  }
  Ok(value)
}

fn verify_runtime_installer_dist(options: &VerifyRuntimeInstallerDistOptions) -> Result<()> {
  verify_runtime_installer_release_assets_for_platform(
    &options.dir,
    &options.version,
    options.platform,
  )?;
  print_selected_artifact_summary(
    "runtime installer",
    &options.dir,
    &runtime_installer_dist_artifacts(&options.dir, &options.version, options.platform)?,
  )
}

fn runtime_installer_dist_artifacts(
  dir: &Path,
  version: &str,
  platform: ReleasePlatform,
) -> Result<Vec<PathBuf>> {
  let mut artifacts = Vec::new();
  for kind in platform.required_runtime_installer_artifact_kinds() {
    let artifact = dir.join(runtime_installer_artifact_name(version, platform, *kind));
    artifacts.push(artifact.clone());
    artifacts.push(checksum_path_for(&artifact)?);
    let manifest = runtime_installer_manifest_path_for(&artifact)?;
    artifacts.push(manifest.clone());
    artifacts.push(checksum_path_for(&manifest)?);
  }
  Ok(artifacts)
}

fn runtime_installer_artifact_name(
  version: &str,
  platform: ReleasePlatform,
  kind: RuntimeInstallerArtifactKind,
) -> String {
  format!(
    "{package}-{version}-{platform}.{extension}",
    package = RUNTIME_INSTALLER_PACKAGE_NAME,
    platform = platform.name(),
    extension = kind.extension(),
  )
}

fn runtime_installer_manifest_path_for(artifact_path: &Path) -> Result<PathBuf> {
  let file_name = artifact_path
    .file_name()
    .and_then(|name| name.to_str())
    .context("runtime installer artifact path has no UTF-8 file name")?;
  Ok(artifact_path.with_file_name(format!("{file_name}.manifest.json")))
}

fn write_runtime_installer_manifest(
  artifact: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<()> {
  let file_name = artifact
    .file_name()
    .and_then(|name| name.to_str())
    .context("runtime installer artifact path has no UTF-8 file name")?;
  let kind = runtime_installer_artifact_kind(options.platform, artifact)
    .context("runtime installer artifact kind is not recognized")?;
  let manifest_path = runtime_installer_manifest_path_for(artifact)?;
  let manifest = json!({
    "artifact": file_name,
    "component": "daemon-runtime",
    "platform": options.platform.name(),
    "packageKind": kind.label(),
    "version": options.version,
    "installPaths": runtime_installer_expected_install_paths(options.platform),
  });
  fs::write(
    &manifest_path,
    serde_json::to_string_pretty(&manifest).context("serialize runtime installer manifest")?,
  )
  .with_context(|| format!("write {}", manifest_path.display()))?;
  let checksum_path = checksum_path_for(&manifest_path)?;
  write_sha256_file(&manifest_path, &checksum_path)?;
  Ok(())
}

fn verify_release_assets(options: &ReleaseAssetsOptions) -> Result<()> {
  verify_release_identity()?;
  if options.mode == ReleaseAssetMode::Publish {
    verify_release_signing_inputs()?;
  }

  reject_raw_app_bundles(&options.dir)?;
  for platform in ReleasePlatform::ALL {
    verify_portable_release_asset(
      &options.dir,
      &options.version,
      platform,
      PortableTopology::Runtime,
    )?;
    verify_runtime_installer_release_assets_for_platform(&options.dir, &options.version, platform)?;
  }

  reject_unmatched_runtime_installer_release_assets(&options.dir, &options.version)?;
  print_artifact_summary("combined release assets", &options.dir)
}

fn verify_release_signing_inputs() -> Result<()> {
  for variable in SIGNING_READY_ENV_VARS {
    if env::var(variable).ok().as_deref() != Some("true") {
      bail!(
        "publish release requires {variable}=true after platform signing/notarization inputs are configured"
      );
    }
  }
  Ok(())
}

fn verify_portable_release_asset(
  dir: &Path,
  version: &str,
  platform: ReleasePlatform,
  topology: PortableTopology,
) -> Result<()> {
  let archive_stem = topology.package_archive_stem(version, platform.name())?;
  let artifact = dir.join(format!(
    "{archive_stem}.{}",
    platform.portable_archive_extension()
  ));
  if !artifact.is_file() {
    bail!("release asset missing: {}", artifact.display());
  }
  verify_sha256_file(&artifact)?;
  verify_portable_archive_contents(&artifact, &archive_stem, topology, platform)
}

fn verify_portable_archive_contents(
  artifact: &Path,
  archive_stem: &str,
  topology: PortableTopology,
  platform: ReleasePlatform,
) -> Result<()> {
  let entries = portable_archive_entries(artifact, platform)?;
  for binary in topology.binaries() {
    let expected = format!(
      "{archive_stem}/{}",
      release_platform_exe_name(binary, platform)
    );
    if !entries.iter().any(|entry| entry == &expected) {
      bail!(
        "portable archive {} is missing expected entry {expected}",
        artifact.display()
      );
    }
  }
  let config_entry = format!("{archive_stem}/cadder.toml");
  let has_config = entries.iter().any(|entry| entry == &config_entry);
  if topology.includes_runtime() && !has_config {
    bail!(
      "portable archive {} is missing expected entry {config_entry}",
      artifact.display()
    );
  }
  verify_portable_archive_file_set(artifact, archive_stem, topology, platform, entries)?;

  Ok(())
}

fn verify_portable_archive_file_set(
  artifact: &Path,
  archive_stem: &str,
  topology: PortableTopology,
  platform: ReleasePlatform,
  entries: Vec<String>,
) -> Result<()> {
  let mut expected = BTreeSet::new();
  for binary in topology.binaries() {
    expected.insert(format!(
      "{archive_stem}/{}",
      release_platform_exe_name(binary, platform)
    ));
  }
  if topology.includes_runtime() {
    expected.insert(format!("{archive_stem}/cadder.toml"));
  }

  let actual = entries
    .into_iter()
    .filter(|entry| !entry.ends_with('/'))
    .collect::<BTreeSet<_>>();
  if actual != expected {
    bail!(
      "portable archive {} contains unexpected file set: expected {:?}, found {:?}",
      artifact.display(),
      expected,
      actual
    );
  }

  Ok(())
}

fn portable_archive_entries(artifact: &Path, platform: ReleasePlatform) -> Result<Vec<String>> {
  if platform == ReleasePlatform::WindowsX64 {
    return zip_archive_entries(artifact);
  }
  tar_gz_archive_entries(artifact)
}

fn zip_archive_entries(artifact: &Path) -> Result<Vec<String>> {
  let file = File::open(artifact).with_context(|| format!("open {}", artifact.display()))?;
  let mut archive =
    zip::ZipArchive::new(file).with_context(|| format!("read ZIP {}", artifact.display()))?;
  let mut entries = Vec::new();
  for index in 0..archive.len() {
    let file = archive
      .by_index(index)
      .with_context(|| format!("read ZIP entry {index} from {}", artifact.display()))?;
    if file.is_file() {
      entries.push(file.name().trim_end_matches('/').to_string());
    }
  }
  entries.sort();
  Ok(entries)
}

fn tar_gz_archive_entries(artifact: &Path) -> Result<Vec<String>> {
  let file = File::open(artifact).with_context(|| format!("open {}", artifact.display()))?;
  let decoder = GzDecoder::new(file);
  let mut archive = tar::Archive::new(decoder);
  let mut entries = Vec::new();
  for entry in archive
    .entries()
    .with_context(|| format!("read TAR {}", artifact.display()))?
  {
    let entry = entry.with_context(|| format!("read TAR entry from {}", artifact.display()))?;
    if entry.header().entry_type().is_file() {
      let path = entry.path()?;
      entries.push(path_to_archive_name(path.as_ref())?);
    }
  }
  entries.sort();
  Ok(entries)
}

fn release_platform_exe_name(name: &str, platform: ReleasePlatform) -> String {
  if platform.uses_windows_executables() {
    format!("{name}.exe")
  } else {
    name.to_string()
  }
}

fn verify_runtime_installer_release_assets_for_platform(
  dir: &Path,
  version: &str,
  platform: ReleasePlatform,
) -> Result<()> {
  for kind in platform.required_runtime_installer_artifact_kinds() {
    let artifact = dir.join(runtime_installer_artifact_name(version, platform, *kind));
    if !artifact.is_file() {
      bail!(
        "runtime installer release asset missing: {}",
        artifact.display()
      );
    }
    verify_sha256_file(&artifact)?;
    verify_runtime_installer_manifest(&artifact, version, platform, *kind)?;
  }
  Ok(())
}

fn verify_runtime_installer_manifest(
  artifact: &Path,
  version: &str,
  platform: ReleasePlatform,
  kind: RuntimeInstallerArtifactKind,
) -> Result<()> {
  let manifest_path = runtime_installer_manifest_path_for(artifact)?;
  if !manifest_path.is_file() {
    bail!(
      "runtime installer manifest missing: {}",
      manifest_path.display()
    );
  }
  verify_sha256_file(&manifest_path)?;
  let manifest = read_json_value(&manifest_path)?;
  let artifact_name = artifact
    .file_name()
    .and_then(|name| name.to_str())
    .context("runtime installer artifact path has no UTF-8 file name")?;
  ensure_json_string(
    &manifest_path,
    &manifest,
    &["artifact"],
    artifact_name,
    "runtime installer artifact",
  )?;
  ensure_json_string(
    &manifest_path,
    &manifest,
    &["component"],
    "daemon-runtime",
    "runtime installer component",
  )?;
  ensure_json_string(
    &manifest_path,
    &manifest,
    &["platform"],
    platform.name(),
    "runtime installer platform",
  )?;
  ensure_json_string(
    &manifest_path,
    &manifest,
    &["packageKind"],
    kind.label(),
    "runtime installer package kind",
  )?;
  ensure_json_string(
    &manifest_path,
    &manifest,
    &["version"],
    version,
    "runtime installer version",
  )?;
  let install_paths = json_path_array(
    &manifest_path,
    &manifest,
    &["installPaths"],
    "runtime installer install paths",
  )?;
  let expected_paths = runtime_installer_expected_install_paths(platform);
  for expected in &expected_paths {
    if !install_paths
      .iter()
      .any(|value| value.as_str() == Some(expected.as_str()))
    {
      bail!(
        "runtime installer manifest {} is missing install path {expected}",
        manifest_path.display()
      );
    }
  }
  for value in install_paths {
    let Some(actual) = value.as_str() else {
      bail!(
        "runtime installer manifest {} contains non-string install path {}",
        manifest_path.display(),
        value
      );
    };
    if !expected_paths.iter().any(|expected| expected == actual) {
      bail!(
        "runtime installer manifest {} contains unexpected install path {actual}",
        manifest_path.display()
      );
    }
  }
  if install_paths.len() != expected_paths.len() {
    bail!(
      "runtime installer manifest {} expected {} install paths, found {}",
      manifest_path.display(),
      expected_paths.len(),
      install_paths.len()
    );
  }
  Ok(())
}

fn json_path_array<'a>(
  path: &Path,
  root: &'a JsonValue,
  keys: &[&str],
  label: &str,
) -> Result<&'a Vec<JsonValue>> {
  json_path_value(path, root, keys, label)?
    .as_array()
    .with_context(|| format!("{label} is not an array in {}", path.display()))
}

fn json_path_value<'a>(
  path: &Path,
  root: &'a JsonValue,
  keys: &[&str],
  label: &str,
) -> Result<&'a JsonValue> {
  let mut value = root;
  for key in keys {
    value = value.get(*key).with_context(|| {
      format!(
        "{label} missing key `{}` in {}",
        keys.join("."),
        path.display()
      )
    })?;
  }
  Ok(value)
}

fn runtime_installer_expected_install_paths(platform: ReleasePlatform) -> Vec<String> {
  match platform {
    ReleasePlatform::WindowsX64 => RUNTIME_PORTABLE_BINARIES
      .iter()
      .map(|binary| format!(r"C:\Program Files\Cadder\{}.exe", binary))
      .chain(std::iter::once(
        r"C:\Program Files\Cadder\cadder.toml".to_string(),
      ))
      .collect(),
    ReleasePlatform::LinuxX64 | ReleasePlatform::MacosX64 | ReleasePlatform::MacosArm64 => {
      RUNTIME_PORTABLE_BINARIES
        .iter()
        .map(|binary| format!("/usr/local/bin/{binary}"))
        .chain(std::iter::once(
          "/usr/local/etc/cadder/cadder.toml".to_string(),
        ))
        .collect()
    }
  }
}

fn runtime_installer_artifact_kind(
  platform: ReleasePlatform,
  path: &Path,
) -> Option<RuntimeInstallerArtifactKind> {
  let name = path.file_name()?.to_str()?.to_ascii_lowercase();
  match platform {
    ReleasePlatform::WindowsX64 if name.ends_with(".msi") => {
      Some(RuntimeInstallerArtifactKind::WindowsMsi)
    }
    ReleasePlatform::LinuxX64 if name.ends_with(".deb") => {
      Some(RuntimeInstallerArtifactKind::LinuxDeb)
    }
    ReleasePlatform::LinuxX64 if name.ends_with(".rpm") => {
      Some(RuntimeInstallerArtifactKind::LinuxRpm)
    }
    ReleasePlatform::MacosX64 | ReleasePlatform::MacosArm64 if name.ends_with(".pkg") => {
      Some(RuntimeInstallerArtifactKind::MacosPkg)
    }
    _ => None,
  }
}

fn reject_unmatched_runtime_installer_release_assets(dir: &Path, version: &str) -> Result<()> {
  let mut expected_names = BTreeSet::new();
  for platform in ReleasePlatform::ALL {
    for kind in platform.required_runtime_installer_artifact_kinds() {
      let artifact_name = runtime_installer_artifact_name(version, platform, *kind);
      expected_names.insert(artifact_name.clone());
      expected_names.insert(format!("{artifact_name}.sha256"));
      let manifest_name = format!("{artifact_name}.manifest.json");
      expected_names.insert(manifest_name.clone());
      expected_names.insert(format!("{manifest_name}.sha256"));
    }
  }

  let mut paths = Vec::new();
  collect_file_paths(dir, &mut paths)?;
  for path in paths {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
      continue;
    };
    if file_name.starts_with("cadder-runtime-") && !expected_names.contains(file_name) {
      bail!(
        "unexpected runtime installer release asset: {}",
        path.display()
      );
    }
  }
  Ok(())
}

fn reject_raw_app_bundles(dir: &Path) -> Result<()> {
  for entry in sorted_dir_entries(dir)? {
    let path = entry.path();
    if path.is_dir() {
      if is_macos_app_bundle(&path) {
        bail!(
          "raw macOS .app bundle is not a publishable release asset: {}",
          path.display()
        );
      }
      reject_raw_app_bundles(&path)?;
    }
  }
  Ok(())
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

fn write_sha256_file(archive_path: &Path, checksum_path: &Path) -> Result<String> {
  let file_name = archive_path
    .file_name()
    .and_then(|name| name.to_str())
    .context("archive path has no UTF-8 file name")?;
  let checksum = compute_sha256(archive_path)?;
  fs::write(checksum_path, format!("{checksum}  {file_name}\n"))
    .with_context(|| format!("write {}", checksum_path.display()))?;
  Ok(checksum)
}

fn checksum_path_for(artifact_path: &Path) -> Result<PathBuf> {
  let file_name = artifact_path
    .file_name()
    .and_then(|name| name.to_str())
    .context("artifact path has no UTF-8 file name")?;
  Ok(artifact_path.with_file_name(format!("{file_name}.sha256")))
}

fn verify_sha256_file(artifact_path: &Path) -> Result<()> {
  let checksum_path = checksum_path_for(artifact_path)?;
  let expected = compute_sha256(artifact_path)?;
  let file_name = artifact_path
    .file_name()
    .and_then(|name| name.to_str())
    .context("artifact path has no UTF-8 file name")?;
  let expected_line = format!("{expected}  {file_name}");
  let actual = fs::read_to_string(&checksum_path)
    .with_context(|| format!("read {}", checksum_path.display()))?;
  if actual.trim() != expected_line {
    bail!(
      "checksum file {} does not match {}",
      checksum_path.display(),
      artifact_path.display()
    );
  }
  Ok(())
}

fn compute_sha256(path: &Path) -> Result<String> {
  let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
  let mut hasher = Sha256::new();
  let mut buffer = [0_u8; 8192];
  loop {
    let bytes_read = file
      .read(&mut buffer)
      .with_context(|| format!("read {}", path.display()))?;
    if bytes_read == 0 {
      break;
    }
    hasher.update(&buffer[..bytes_read]);
  }
  Ok(hex::encode(hasher.finalize()))
}

fn print_artifact_summary(label: &str, root: &Path) -> Result<()> {
  let mut paths = Vec::new();
  collect_file_paths(root, &mut paths)?;
  print_selected_artifact_summary(label, root, &paths)
}

fn collect_file_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
  for entry in sorted_dir_entries(dir)? {
    let path = entry.path();
    if path.is_dir() {
      collect_file_paths(&path, paths)?;
    } else {
      paths.push(path);
    }
  }
  paths.sort();
  Ok(())
}

fn print_selected_artifact_summary(label: &str, root: &Path, paths: &[PathBuf]) -> Result<()> {
  println!("{label} artifact summary:");
  if paths.is_empty() {
    println!("  no files");
    return Ok(());
  }

  for path in paths {
    let size = path_size(path)?;
    let display_path = relative_display_path(root, path);
    println!("  {display_path}\t{size} bytes");
  }
  Ok(())
}

fn path_size(path: &Path) -> Result<u64> {
  let metadata = fs::metadata(path).with_context(|| format!("inspect {}", path.display()))?;
  if metadata.is_file() {
    return Ok(metadata.len());
  }

  let mut size = 0_u64;
  collect_dir_size(path, &mut size)?;
  Ok(size)
}

fn collect_dir_size(dir: &Path, size: &mut u64) -> Result<()> {
  for entry in sorted_dir_entries(dir)? {
    let path = entry.path();
    let metadata = fs::metadata(&path).with_context(|| format!("inspect {}", path.display()))?;
    if metadata.is_dir() {
      collect_dir_size(&path, size)?;
    } else {
      *size = size.saturating_add(metadata.len());
    }
  }
  Ok(())
}

fn relative_display_path(root: &Path, path: &Path) -> String {
  path
    .strip_prefix(root)
    .unwrap_or(path)
    .display()
    .to_string()
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

fn path_argument(path: &Path) -> Result<String> {
  path
    .to_str()
    .map(ToOwned::to_owned)
    .with_context(|| format!("path contains non-UTF-8 data: {}", path.display()))
}

fn xml_escape(value: &str) -> String {
  value
    .replace('&', "&amp;")
    .replace('"', "&quot;")
    .replace('<', "&lt;")
    .replace('>', "&gt;")
}

fn windows_installer_id(value: &str) -> String {
  value
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() {
        character
      } else {
        '_'
      }
    })
    .collect()
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
  topology: PortableTopology,
}

impl DistOptions {
  fn parse(args: Vec<String>) -> Result<Self> {
    Ok(Self {
      out_dir: required_path_option(&args, "--out")?,
      target: optional_string_option(&args, "--target")?,
      topology: PortableTopology::parse_option(&args)?,
    })
  }
}

#[derive(Debug, PartialEq, Eq)]
struct RuntimeInstallerOptions {
  out_dir: PathBuf,
  version: String,
  target: Option<String>,
  platform: ReleasePlatform,
  signing_mode: SigningMode,
}

impl RuntimeInstallerOptions {
  fn parse(args: Vec<String>) -> Result<Self> {
    let target = optional_string_option(&args, "--target")?;
    let platform = ReleasePlatform::parse_option(&args, target.as_deref())?;
    Ok(Self {
      out_dir: required_path_option(&args, "--out")?,
      version: optional_string_option(&args, "--version")?
        .map_or_else(workspace_package_version, Ok)?,
      target,
      platform,
      signing_mode: SigningMode::parse(&args),
    })
  }
}

#[derive(Debug, PartialEq, Eq)]
struct VerifyDistOptions {
  dir: PathBuf,
  target: Option<String>,
  topology: PortableTopology,
}

impl VerifyDistOptions {
  fn parse(args: Vec<String>) -> Result<Self> {
    Ok(Self {
      dir: required_path_option(&args, "--dir")?,
      target: optional_string_option(&args, "--target")?,
      topology: PortableTopology::parse_option(&args)?,
    })
  }
}

#[derive(Debug, PartialEq, Eq)]
struct VerifyRuntimeInstallerDistOptions {
  dir: PathBuf,
  version: String,
  platform: ReleasePlatform,
}

impl VerifyRuntimeInstallerDistOptions {
  fn parse(args: Vec<String>) -> Result<Self> {
    let target = optional_string_option(&args, "--target")?;
    let platform = ReleasePlatform::parse_option(&args, target.as_deref())?;
    Ok(Self {
      dir: required_path_option(&args, "--dir")?,
      version: optional_string_option(&args, "--version")?
        .map_or_else(workspace_package_version, Ok)?,
      platform,
    })
  }
}

#[derive(Debug, PartialEq, Eq)]
struct ReleaseAssetsOptions {
  dir: PathBuf,
  version: String,
  mode: ReleaseAssetMode,
}

impl ReleaseAssetsOptions {
  fn parse(args: Vec<String>) -> Result<Self> {
    Ok(Self {
      dir: required_path_option(&args, "--dir")?,
      version: optional_string_option(&args, "--version")?
        .map_or_else(workspace_package_version, Ok)?,
      mode: ReleaseAssetMode::parse(&args)?,
    })
  }
}

#[derive(Debug, PartialEq, Eq)]
struct PackageOptions {
  out_dir: PathBuf,
  version: String,
  platform: String,
  target: Option<String>,
  topology: PortableTopology,
}

impl PackageOptions {
  fn parse(args: Vec<String>) -> Result<Self> {
    Ok(Self {
      out_dir: required_path_option(&args, "--out")?,
      version: optional_string_option(&args, "--version")?
        .map_or_else(workspace_package_version, Ok)?,
      platform: required_string_option(&args, "--platform")?,
      target: optional_string_option(&args, "--target")?,
      topology: PortableTopology::parse_option(&args)?,
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

fn has_flag(args: &[String], flag: &str) -> bool {
  args.iter().any(|arg| arg == flag)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn xtask_help_lists_canonical_entrypoint_and_command_groups() {
    let help = xtask_help_text();

    assert!(help.contains("cargo xtask <command> [args...]"), "{help}");
    assert!(help.contains("validation:"), "{help}");
    assert!(help.contains("documentation:"), "{help}");
    assert!(help.contains("release and packaging:"), "{help}");
    assert!(help.contains("docs-check"), "{help}");
    assert!(help.contains("verify-assets"), "{help}");
    assert!(help.contains("verify-release-assets"), "{help}");
    assert!(help.contains("verify-release-identity"), "{help}");
    assert!(help.contains("verify-workspace-topology"), "{help}");
    assert!(help.contains("dev-run"), "{help}");
  }

  #[test]
  fn xtask_command_help_and_list_are_stable() {
    let list = xtask_command_list_text();
    assert!(list.lines().any(|line| line == "check"), "{list}");
    assert!(list.lines().any(|line| line == "docs-build"), "{list}");
    assert!(
      list.lines().any(|line| line == "runtime-installer"),
      "{list}"
    );
    assert!(
      list.lines().any(|line| line == "verify-release-identity"),
      "{list}"
    );
    assert!(
      list.lines().any(|line| line == "verify-workspace-topology"),
      "{list}"
    );
    assert!(list.lines().any(|line| line == "verify-assets"), "{list}");

    let docs_help = xtask_command_help_text("docs-check").unwrap();
    assert!(docs_help.contains("cargo xtask docs-check"), "{docs_help}");
    assert!(docs_help.contains("Astro"), "{docs_help}");
  }

  #[test]
  fn cargo_xtask_alias_is_configured() {
    let config_path = workspace_root().join(".cargo").join("config.toml");
    let config = fs::read_to_string(&config_path).unwrap();

    assert!(config.contains("[alias]"), "{config}");
    assert!(config.contains("xtask = \"run -p xtask --\""), "{config}");
  }

  #[test]
  fn docs_site_dir_points_to_docs_package() {
    assert!(docs_site_dir().join("package.json").is_file());
    assert!(docs_site_dir().join("bun.lock").is_file());
  }

  #[test]
  fn coverage_toolchain_selection_respects_override_active_and_windows_fallback() {
    assert_eq!(
      coverage_toolchain_argument(Some("nightly"), Some("stable"), true, true).unwrap(),
      Some("+nightly".to_string())
    );
    assert_eq!(
      coverage_toolchain_argument(Some("+beta"), None, false, false).unwrap(),
      Some("+beta".to_string())
    );
    assert_eq!(
      coverage_toolchain_argument(None, Some("stable-x86_64-pc-windows-msvc"), true, true).unwrap(),
      None
    );
    assert_eq!(
      coverage_toolchain_argument(None, Some("stable-x86_64-pc-windows-gnu"), true, true).unwrap(),
      Some(format!("+{WINDOWS_COVERAGE_TOOLCHAIN}"))
    );
    assert_eq!(
      coverage_toolchain_argument(None, Some("stable"), true, true).unwrap(),
      Some(format!("+{WINDOWS_COVERAGE_TOOLCHAIN}"))
    );
    assert_eq!(
      coverage_toolchain_argument(None, None, true, false).unwrap(),
      Some(format!("+{WINDOWS_COVERAGE_TOOLCHAIN}"))
    );
    assert_eq!(
      coverage_toolchain_argument(None, None, false, false).unwrap(),
      None
    );
  }

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
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        topology: PortableTopology::DEFAULT
      }
    );
  }

  #[test]
  fn dist_options_rejects_unknown_topology() {
    let error = DistOptions::parse(vec![
      "--out".to_string(),
      "target/dist".to_string(),
      "--topology".to_string(),
      "alternate".to_string(),
    ])
    .unwrap_err();

    assert!(
      error
        .to_string()
        .contains("unknown portable topology `alternate`")
    );
  }

  #[test]
  fn portable_topology_rejects_unknown_value() {
    let error = DistOptions::parse(vec![
      "--out".to_string(),
      "target/dist".to_string(),
      "--topology".to_string(),
      "everything".to_string(),
    ])
    .unwrap_err();

    assert!(
      error
        .to_string()
        .contains("unknown portable topology `everything`")
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
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        topology: PortableTopology::DEFAULT
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
  fn workspace_package_version_from_manifest_reports_missing_and_empty_versions() {
    let dir = unique_temp_dir("workspace-version-errors");
    fs::create_dir_all(&dir).unwrap();

    let missing = dir.join("missing-version.toml");
    fs::write(
      &missing,
      r#"
[package]
name = "sample"
version = "0.1.0"

[workspace.package]
edition = "2024"
"#,
    )
    .unwrap();
    let missing_error = workspace_package_version_from_manifest(&missing).unwrap_err();
    assert!(missing_error.to_string().contains("version not found"));

    let empty = dir.join("empty-version.toml");
    fs::write(
      &empty,
      r#"
[workspace.package]
version = ""
"#,
    )
    .unwrap();
    let empty_error = workspace_package_version_from_manifest(&empty).unwrap_err();
    assert!(empty_error.to_string().contains("version is empty"));

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_workspace_topology_manifest_accepts_expected_members() {
    let dir = unique_temp_dir("workspace-topology");
    fs::create_dir_all(&dir).unwrap();
    write_workspace_topology_fixture(&dir);

    verify_workspace_topology_manifest(&dir.join("Cargo.toml")).unwrap();

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_workspace_topology_manifest_rejects_undocumented_member() {
    let dir = unique_temp_dir("workspace-topology-extra");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
      dir.join("Cargo.toml"),
      workspace_topology_manifest(&["crates/cadder2"]),
    )
    .unwrap();

    let error = verify_workspace_topology_manifest(&dir.join("Cargo.toml")).unwrap_err();

    assert!(error.to_string().contains("unexpected: crates/cadder2"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_workspace_topology_manifest_rejects_wrong_member_package_name() {
    let dir = unique_temp_dir("workspace-topology-wrong-package");
    fs::create_dir_all(&dir).unwrap();
    write_workspace_topology_fixture(&dir);
    let cadder = WORKSPACE_MEMBER_CONTRACTS
      .iter()
      .find(|contract| contract.path == "crates/cadder")
      .copied()
      .unwrap();
    write_workspace_member_manifest(&dir, cadder, "cadder-web");

    let error = verify_workspace_topology_manifest(&dir.join("Cargo.toml")).unwrap_err();

    assert!(
      error
        .to_string()
        .contains("package name must be \"cadder\"")
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_identity_accepts_current_repo_metadata() {
    verify_release_identity_files(
      Path::new(WORKSPACE_MANIFEST),
      &workspace_root().join(DOCS_DOWNLOAD_SCRIPT),
    )
    .unwrap();
  }

  #[test]
  fn verify_assets_accepts_current_repo_contract() {
    verify_assets_at(&workspace_root()).unwrap();
  }

  #[test]
  fn verify_canonical_asset_copies_rejects_drifted_docs_copy() {
    let dir = unique_temp_dir("asset-copy-drift");
    fs::create_dir_all(dir.join("assets")).unwrap();
    fs::create_dir_all(dir.join("docs/site/src/assets")).unwrap();
    fs::write(dir.join("assets/logo.png"), b"canonical").unwrap();
    fs::write(dir.join("docs/site/src/assets/logo.png"), b"copy").unwrap();

    let error = verify_canonical_asset_copies(
      &dir,
      &[(
        "docs logo pipeline copy",
        "assets/logo.png",
        "docs/site/src/assets/logo.png",
      )],
    )
    .unwrap_err();

    assert!(
      error
        .to_string()
        .contains("docs logo pipeline copy at docs/site/src/assets/logo.png differs")
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_obsolete_assets_absent_rejects_scaffold_asset() {
    let dir = unique_temp_dir("obsolete-asset");
    fs::create_dir_all(dir.join("scaffold/public")).unwrap();
    fs::write(dir.join("scaffold/public/vite.svg"), "<svg />").unwrap();

    let error =
      verify_obsolete_assets_absent(&dir, &[("Vite starter logo", "scaffold/public/vite.svg")])
        .unwrap_err();

    assert!(error.to_string().contains("obsolete scaffold asset"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_docs_webmanifest_rejects_empty_product_name() {
    let dir = unique_temp_dir("webmanifest-product-name");
    let public_dir = dir.join("docs/site/public");
    fs::create_dir_all(&public_dir).unwrap();
    fs::write(public_dir.join("android-chrome-192x192.png"), b"icon").unwrap();
    let manifest = public_dir.join("site.webmanifest");
    fs::write(
      &manifest,
      serde_json::to_string_pretty(&json!({
        "name": "",
        "short_name": "Cadder",
        "icons": [
          {
            "src": "/android-chrome-192x192.png",
            "sizes": "192x192",
            "type": "image/png",
          },
        ],
      }))
      .unwrap(),
    )
    .unwrap();

    let error = verify_docs_webmanifest(&dir, &manifest).unwrap_err();

    assert!(error.to_string().contains("web manifest name"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_docs_download_metadata_rejects_missing_asset_pattern() {
    let dir = unique_temp_dir("download-metadata-drift");
    let script = dir.join("cadder-downloads.js");
    fs::create_dir_all(&dir).unwrap();
    fs::write(&script, "const cadderRuntimeAssetPatterns = {};\n").unwrap();

    let error = verify_docs_download_metadata(&script).unwrap_err();

    assert!(error.to_string().contains("runtime Windows archive"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_profile_manifest_accepts_expected_policy() {
    let dir = unique_temp_dir("release-profile");
    let manifest = dir.join("Cargo.toml");
    fs::create_dir_all(&dir).unwrap();
    fs::write(&manifest, expected_release_profile_manifest()).unwrap();

    verify_release_profile_manifest(&manifest).unwrap();

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_profile_manifest_rejects_drifted_release_setting() {
    let dir = unique_temp_dir("release-profile-drift");
    let manifest = dir.join("Cargo.toml");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
      &manifest,
      expected_release_profile_manifest().replace("opt-level = \"s\"", "opt-level = 3"),
    )
    .unwrap();

    let error = verify_release_profile_manifest(&manifest).unwrap_err();

    assert!(
      error
        .to_string()
        .contains("profile.release.opt-level expected \"s\", found 3")
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_profile_manifest_rejects_missing_profiling_setting() {
    let dir = unique_temp_dir("release-profile-missing");
    let manifest = dir.join("Cargo.toml");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
      &manifest,
      expected_release_profile_manifest().replace("strip = \"none\"\n", ""),
    )
    .unwrap();

    let error = verify_release_profile_manifest(&manifest).unwrap_err();

    assert!(
      error
        .to_string()
        .contains("profile.profiling.strip missing")
    );
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
      "target/custom/summary.lcov".to_string(),
    ])
    .unwrap();

    assert_eq!(
      options,
      CoverageOptions {
        output_path: PathBuf::from("target/custom/summary.lcov")
      }
    );
  }

  #[test]
  fn runtime_installer_options_parse_target_platform_and_signing() {
    let options = RuntimeInstallerOptions::parse(vec![
      "--out".to_string(),
      "target/runtime-installers".to_string(),
      "--version".to_string(),
      "1.2.3".to_string(),
      "--target".to_string(),
      "x86_64-pc-windows-msvc".to_string(),
      "--sign".to_string(),
    ])
    .unwrap();

    assert_eq!(
      options,
      RuntimeInstallerOptions {
        out_dir: PathBuf::from("target/runtime-installers"),
        version: "1.2.3".to_string(),
        target: Some("x86_64-pc-windows-msvc".to_string()),
        platform: ReleasePlatform::WindowsX64,
        signing_mode: SigningMode::SignedRelease
      }
    );
  }

  #[test]
  fn verify_runtime_installer_dist_options_parse_required_values() {
    let options = VerifyRuntimeInstallerDistOptions::parse(vec![
      "--dir".to_string(),
      "target/runtime-installers".to_string(),
      "--version".to_string(),
      "1.2.3".to_string(),
      "--platform".to_string(),
      "linux-x64".to_string(),
    ])
    .unwrap();

    assert_eq!(
      options,
      VerifyRuntimeInstallerDistOptions {
        dir: PathBuf::from("target/runtime-installers"),
        version: "1.2.3".to_string(),
        platform: ReleasePlatform::LinuxX64
      }
    );
  }

  #[test]
  fn release_platform_parses_and_infers_targets() {
    assert_eq!(
      ReleasePlatform::parse("macos-arm64").unwrap(),
      ReleasePlatform::MacosArm64
    );
    assert_eq!(
      ReleasePlatform::infer_from_target(Some("x86_64-unknown-linux-gnu")).unwrap(),
      ReleasePlatform::LinuxX64
    );

    let error = ReleasePlatform::parse("freebsd-x64").unwrap_err();
    assert!(
      error
        .to_string()
        .contains("unknown release platform `freebsd-x64`")
    );
  }

  #[test]
  fn release_platform_defines_runtime_installer_artifact_kinds() {
    assert_eq!(
      ReleasePlatform::WindowsX64.required_runtime_installer_artifact_kinds(),
      &[RuntimeInstallerArtifactKind::WindowsMsi]
    );
    assert_eq!(
      ReleasePlatform::LinuxX64.required_runtime_installer_artifact_kinds(),
      &[
        RuntimeInstallerArtifactKind::LinuxDeb,
        RuntimeInstallerArtifactKind::LinuxRpm
      ]
    );
    assert_eq!(
      ReleasePlatform::MacosArm64.required_runtime_installer_artifact_kinds(),
      &[RuntimeInstallerArtifactKind::MacosPkg]
    );
  }

  #[test]
  fn runtime_installer_artifact_names_use_canonical_release_platforms() {
    assert_eq!(
      runtime_installer_artifact_name(
        "1.2.3",
        ReleasePlatform::WindowsX64,
        RuntimeInstallerArtifactKind::WindowsMsi
      ),
      "cadder-runtime-1.2.3-windows-x64.msi"
    );
    assert_eq!(
      runtime_installer_artifact_name(
        "1.2.3",
        ReleasePlatform::LinuxX64,
        RuntimeInstallerArtifactKind::LinuxDeb
      ),
      "cadder-runtime-1.2.3-linux-x64.deb"
    );
  }

  #[test]
  fn verify_release_assets_accepts_complete_dry_run_matrix() {
    let dir = unique_temp_dir("release-assets");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");

    verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap();

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_assets_rejects_missing_portable_checksum() {
    let dir = unique_temp_dir("release-assets-missing-checksum");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");
    fs::remove_file(dir.join("cadder-1.2.3-windows-x64.zip.sha256")).unwrap();

    let error = verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap_err();
    assert!(
      error
        .to_string()
        .contains("cadder-1.2.3-windows-x64.zip.sha256")
    );

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_assets_rejects_portable_archive_with_wrong_contents() {
    let dir = unique_temp_dir("release-assets-wrong-archive");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");
    let archive = dir.join("cadder-1.2.3-windows-x64.zip");
    fs::remove_file(&archive).unwrap();
    fs::remove_file(dir.join("cadder-1.2.3-windows-x64.zip.sha256")).unwrap();
    write_release_asset(&dir, "cadder-1.2.3-windows-x64.zip");

    let error = verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap_err();
    assert!(error.to_string().contains("read ZIP"));

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_assets_rejects_missing_runtime_installer() {
    let dir = unique_temp_dir("release-assets-missing-runtime-installer");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");
    fs::remove_file(dir.join("cadder-runtime-1.2.3-windows-x64.msi")).unwrap();

    let error = verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap_err();
    assert!(
      error
        .to_string()
        .contains("cadder-runtime-1.2.3-windows-x64.msi")
    );

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_assets_rejects_runtime_installer_manifest_with_extra_install_path() {
    let dir = unique_temp_dir("release-assets-runtime-manifest-extra-path");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");
    let manifest = dir.join("cadder-runtime-1.2.3-windows-x64.msi.manifest.json");
    let mut value = read_json_value(&manifest).unwrap();
    value["installPaths"]
      .as_array_mut()
      .unwrap()
      .push(JsonValue::String(
        r"C:\Program Files\Cadder\extra-tool.exe".to_string(),
      ));
    fs::write(&manifest, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    write_sha256_file(&manifest, &checksum_path_for(&manifest).unwrap()).unwrap();

    let error = verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap_err();
    assert!(
      error
        .to_string()
        .contains("contains unexpected install path"),
      "{error}"
    );

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_release_assets_rejects_unexpected_runtime_installer_asset() {
    let dir = unique_temp_dir("release-assets-unexpected-runtime-installer");
    fs::create_dir_all(&dir).unwrap();
    write_complete_release_asset_matrix(&dir, "1.2.3");
    fs::write(dir.join("cadder-runtime-1.2.3-linux-x64.AppImage"), b"bad").unwrap();

    let error = verify_release_assets(&ReleaseAssetsOptions {
      dir: dir.clone(),
      version: "1.2.3".to_string(),
      mode: ReleaseAssetMode::DryRun,
    })
    .unwrap_err();
    assert!(
      error
        .to_string()
        .contains("unexpected runtime installer release asset")
    );

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn coverage_command_args_emit_workspace_lcov_report() {
    let args = coverage_command_args(Path::new("target/custom/summary.lcov")).unwrap();

    let expected_tail = [
      "llvm-cov".to_string(),
      "--workspace".to_string(),
      "--lcov".to_string(),
      "--output-path".to_string(),
      "target/custom/summary.lcov".to_string(),
    ];
    let tail_start = args.len() - expected_tail.len();

    assert_eq!(&args[tail_start..], expected_tail);
    assert!(tail_start <= 1, "{args:?}");
    if tail_start == 1 {
      assert!(args[0].starts_with('+'), "{args:?}");
    }
  }

  #[test]
  fn read_lcov_line_coverage_sums_file_records() {
    let dir = unique_temp_dir("lcov-line-coverage");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(
      &report,
      "TN:\nSF:first.rs\nDA:10,1\nDA:11,0\nDA:11,3\nend_of_record\nTN:\nSF:second.rs\nDA:20,5\nDA:21,0\nend_of_record\n",
    )
    .unwrap();

    let coverage = read_lcov_line_coverage(&report).unwrap();

    assert_eq!(
      coverage,
      LcovLineCoverage {
        covered: 3,
        total: 4
      }
    );
    assert_eq!(coverage.percent(), 75.0);
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn read_lcov_line_coverage_accepts_da_checksum_fields() {
    let dir = unique_temp_dir("lcov-line-coverage-checksum");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(
      &report,
      "TN:\nSF:lib.rs\nDA:10,1,0123456789abcdef\nDA:11,0,abcdef0123456789\nend_of_record\n",
    )
    .unwrap();

    let coverage = read_lcov_line_coverage(&report).unwrap();

    assert_eq!(
      coverage,
      LcovLineCoverage {
        covered: 1,
        total: 2
      }
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn read_lcov_line_coverage_rejects_empty_reports() {
    let dir = unique_temp_dir("empty-lcov-line-coverage");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(&report, "TN:\nend_of_record\n").unwrap();

    let error = read_lcov_line_coverage(&report).unwrap_err();

    assert!(error.to_string().contains("did not contain any DA"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn read_lcov_line_coverage_rejects_da_before_source_file() {
    let dir = unique_temp_dir("lcov-da-before-source-file");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(&report, "TN:\nDA:1,1\nend_of_record\n").unwrap();

    let error = read_lcov_line_coverage(&report).unwrap_err();

    assert!(error.to_string().contains("DA record appeared before SF"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn read_lcov_line_coverage_rejects_malformed_da_records() {
    let dir = unique_temp_dir("malformed-lcov-da");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(&report, "TN:\nSF:lib.rs\nDA:abc,1\nend_of_record\n").unwrap();

    let error = read_lcov_line_coverage(&report).unwrap_err();

    assert!(error.to_string().contains("parse DA source line"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn enforce_lcov_line_threshold_accepts_exact_threshold() {
    let dir = unique_temp_dir("threshold-lcov-line-coverage");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(&report, lcov_report_with_line_counts(85, 100)).unwrap();

    enforce_lcov_line_threshold(&report, 85.0).unwrap();

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn enforce_lcov_line_threshold_reports_low_coverage() {
    let dir = unique_temp_dir("low-lcov-line-coverage");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(&report, lcov_report_with_line_counts(84, 100)).unwrap();

    let error = enforce_lcov_line_threshold(&report, 85.0).unwrap_err();

    assert!(error.to_string().contains("below required 85.00%"));
    assert!(error.to_string().contains("84/100"));
    fs::remove_dir_all(&dir).unwrap();
  }

  fn lcov_report_with_line_counts(covered: u64, total: u64) -> String {
    let mut report = "TN:\nSF:lib.rs\n".to_string();
    for line in 1..=total {
      let count = u64::from(line <= covered);
      report.push_str(&format!("DA:{line},{count}\n"));
    }
    report.push_str("end_of_record\n");
    report
  }

  #[test]
  fn coverage_command_args_use_workspace_lcov_without_package_exclusions() {
    assert!(COVERAGE_EXCLUDED_PACKAGES.is_empty());
    assert!(COVERAGE_IGNORED_FILENAME_REGEX.is_empty());
  }

  #[test]
  fn ensure_parent_dir_creates_report_directory() {
    let dir = unique_temp_dir("coverage-report-parent");
    let report_path = dir.join("nested").join("summary.lcov");

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
      PortableTopology::Runtime.binaries(),
      &["cadderd", "cadder", "caddy"]
    );
  }

  #[test]
  fn portable_topology_archive_stem_uses_runtime_name() {
    assert_eq!(
      PortableTopology::Runtime
        .package_archive_stem("1.2.3", "windows-x64")
        .unwrap(),
      "cadder-1.2.3-windows-x64"
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
      topology: PortableTopology::Runtime,
    })
    .unwrap_err();

    assert!(error.to_string().contains("portable binary missing"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_dist_rejects_missing_sample_config_after_binaries_exist() {
    let dir = unique_temp_dir("missing-sample-config");
    fs::create_dir_all(&dir).unwrap();
    for binary in PortableTopology::Runtime.binaries() {
      fs::write(dir.join(exe_name(binary, None)), b"not executable").unwrap();
    }

    let error = verify_dist(&VerifyDistOptions {
      dir: dir.clone(),
      target: None,
      topology: PortableTopology::Runtime,
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
  fn verify_dist_accepts_fake_portable_layout_and_binary_contract() {
    let dir = unique_temp_dir("portable-layout");
    fs::create_dir_all(&dir).unwrap();
    write_fake_portable_executable_layout(&dir, PortableTopology::Runtime);

    verify_dist(&VerifyDistOptions {
      dir: dir.clone(),
      target: None,
      topology: PortableTopology::Runtime,
    })
    .unwrap();

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn verify_dist_rejects_extra_portable_files() {
    let dir = unique_temp_dir("portable-extra-files");
    fs::create_dir_all(&dir).unwrap();
    write_fake_portable_executable_layout(&dir, PortableTopology::Runtime);
    fs::write(dir.join("extra.txt"), b"extra").unwrap();

    let error = verify_dist(&VerifyDistOptions {
      dir: dir.clone(),
      target: None,
      topology: PortableTopology::Runtime,
    })
    .unwrap_err();

    assert!(error.to_string().contains("contains unexpected file set"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn dist_with_builder_copies_target_release_binaries_and_sample_config() {
    let unique = unique_suffix();
    let target = format!("cadder-test-windows-{unique}");
    let release_dir = PathBuf::from("target").join(&target).join("release");
    let out_dir = unique_temp_dir("dist-layout");
    fs::create_dir_all(&release_dir).unwrap();
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(out_dir.join("cadder.exe"), b"stale operator").unwrap();

    let result = dist_with_builder(
      DistOptions {
        out_dir: out_dir.clone(),
        target: Some(target.clone()),
        topology: PortableTopology::Runtime,
      },
      |requested_target, topology| {
        assert_eq!(requested_target, Some(target.as_str()));
        assert_eq!(topology, PortableTopology::Runtime);
        write_fake_portable_executable_layout(&release_dir, topology);
        Ok(())
      },
    );

    result.unwrap();
    for binary in PortableTopology::Runtime.binaries() {
      assert!(out_dir.join(format!("{binary}.exe")).is_file());
    }
    assert_ne!(
      fs::read(out_dir.join("cadder.exe")).unwrap(),
      b"stale operator"
    );
    assert_eq!(
      fs::read_to_string(out_dir.join("cadder.toml")).unwrap(),
      SAMPLE_CADDER_TOML
    );

    fs::remove_dir_all(&out_dir).unwrap();
    fs::remove_dir_all(PathBuf::from("target").join(&target)).unwrap();
  }

  #[test]
  fn replace_dir_from_recursively_overwrites_existing_target_tree() {
    let dir = unique_temp_dir("replace-tree");
    let source = dir.join("source");
    let target = dir.join("target");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::create_dir_all(target.join("stale")).unwrap();
    fs::write(source.join("root.txt"), "root").unwrap();
    fs::write(source.join("nested").join("child.txt"), "child").unwrap();
    fs::write(target.join("stale").join("old.txt"), "old").unwrap();

    replace_dir_from(&source, &target).unwrap();

    assert_eq!(fs::read_to_string(target.join("root.txt")).unwrap(), "root");
    assert_eq!(
      fs::read_to_string(target.join("nested").join("child.txt")).unwrap(),
      "child"
    );
    assert!(!target.join("stale").exists());
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
  fn artifact_summary_helpers_collect_file_paths_and_directory_sizes() {
    let dir = unique_temp_dir("artifact-summary");
    let nested = dir.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(dir.join("root.bin"), [1, 2, 3]).unwrap();
    fs::write(nested.join("child.bin"), [4, 5]).unwrap();

    let mut paths = Vec::new();
    collect_file_paths(&dir, &mut paths).unwrap();

    assert_eq!(
      paths,
      vec![dir.join("nested").join("child.bin"), dir.join("root.bin")]
    );
    assert_eq!(path_size(&nested).unwrap(), 2);
    assert_eq!(
      relative_display_path(&dir, &nested.join("child.bin")),
      PathBuf::from("nested")
        .join("child.bin")
        .display()
        .to_string()
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn run_helpers_report_process_success_failure_cwd_and_env() {
    let dir = unique_temp_dir("process-helper");
    let cwd = dir.join("work");
    let env_value = dir.join("env-value");
    fs::create_dir_all(&cwd).unwrap();
    let helper = build_process_helper(&dir);
    let helper = helper.to_str().unwrap();
    let cwd_arg = cwd.to_str().unwrap();
    let env_arg = env_value.to_str().unwrap();

    run(helper, &["ok"]).unwrap();
    run_in(helper, &["ok"], &cwd).unwrap();
    run_in_with_env(
      helper,
      &["cwd-env", cwd_arg, env_arg],
      &cwd,
      &[("CADDER_XTASK_PROCESS_HELPER", &env_value)],
    )
    .unwrap();

    let run_error = run(helper, &["fail"]).unwrap_err();
    assert!(run_error.to_string().contains("failed with"));
    let run_in_error = run_in(helper, &["fail"], &cwd).unwrap_err();
    assert!(run_in_error.to_string().contains("failed with"));

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
        topology: PortableTopology::Runtime,
      },
      |dist_options| {
        assert_eq!(dist_options.out_dir, expected_layout);
        assert_eq!(
          dist_options.target,
          Some("x86_64-pc-windows-msvc".to_string())
        );
        assert_eq!(dist_options.topology, PortableTopology::Runtime);
        write_fake_portable_layout(
          &dist_options.out_dir,
          &["cadderd.exe", "cadder.exe", "caddy.exe"],
          true,
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
        "cadder-1.2.3-windows-x64/cadder.exe",
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
        topology: PortableTopology::Runtime,
      },
      |dist_options| {
        assert_eq!(dist_options.out_dir, expected_layout);
        assert_eq!(
          dist_options.target,
          Some("x86_64-unknown-linux-gnu".to_string())
        );
        assert_eq!(dist_options.topology, PortableTopology::Runtime);
        write_fake_portable_layout(&dist_options.out_dir, &["cadderd", "cadder", "caddy"], true)
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
        "cadder-1.2.3-linux-x64/cadder",
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

  fn write_fake_portable_layout(dir: &Path, binaries: &[&str], include_config: bool) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    for binary in binaries {
      fs::write(dir.join(binary), binary).with_context(|| format!("write {binary}"))?;
    }
    if include_config {
      fs::write(dir.join("cadder.toml"), SAMPLE_CADDER_TOML)
        .with_context(|| format!("write {}", dir.join("cadder.toml").display()))?;
    }
    Ok(())
  }

  fn write_complete_release_asset_matrix(dir: &Path, version: &str) {
    for platform in ReleasePlatform::ALL {
      write_portable_release_asset(dir, version, platform, PortableTopology::Runtime);
      write_runtime_installer_release_assets(dir, version, platform);
    }
  }

  fn write_portable_release_asset(
    dir: &Path,
    version: &str,
    platform: ReleasePlatform,
    topology: PortableTopology,
  ) {
    let archive_stem = topology
      .package_archive_stem(version, platform.name())
      .unwrap();
    let layout_parent = unique_temp_dir("release-archive-layout");
    let layout_dir = layout_parent.join(&archive_stem);
    let binaries = topology
      .binaries()
      .iter()
      .map(|binary| release_platform_exe_name(binary, platform))
      .collect::<Vec<_>>();
    let binary_refs = binaries.iter().map(String::as_str).collect::<Vec<_>>();
    write_fake_portable_layout(&layout_dir, &binary_refs, topology.includes_runtime()).unwrap();

    let archive = dir.join(format!(
      "{archive_stem}.{}",
      platform.portable_archive_extension()
    ));
    match platform {
      ReleasePlatform::WindowsX64 => {
        write_zip_archive(&layout_parent, &archive_stem, &archive).unwrap();
      }
      ReleasePlatform::LinuxX64 | ReleasePlatform::MacosX64 | ReleasePlatform::MacosArm64 => {
        write_tar_gz_archive(&layout_parent, &archive_stem, &archive).unwrap();
      }
    }
    write_sha256_file(&archive, &checksum_path_for(&archive).unwrap()).unwrap();
    fs::remove_dir_all(&layout_parent).unwrap();
  }

  fn write_runtime_installer_release_assets(dir: &Path, version: &str, platform: ReleasePlatform) {
    let options = RuntimeInstallerOptions {
      out_dir: dir.to_path_buf(),
      version: version.to_string(),
      target: None,
      platform,
      signing_mode: SigningMode::UnsignedDryRun,
    };
    for kind in platform.required_runtime_installer_artifact_kinds() {
      let artifact = dir.join(runtime_installer_artifact_name(version, platform, *kind));
      fs::write(&artifact, kind.label()).unwrap();
      write_sha256_file(&artifact, &checksum_path_for(&artifact).unwrap()).unwrap();
      write_runtime_installer_manifest(&artifact, &options).unwrap();
    }
  }

  fn write_release_asset(dir: &Path, file_name: &str) {
    let artifact = dir.join(file_name);
    fs::write(&artifact, file_name).unwrap();
    write_sha256_file(&artifact, &checksum_path_for(&artifact).unwrap()).unwrap();
  }

  fn write_fake_portable_executable_layout(dir: &Path, topology: PortableTopology) {
    let helper_dir = tempfile::tempdir().unwrap();
    let helper = build_fake_portable_tool(helper_dir.path());
    for binary in topology.binaries() {
      fs::copy(&helper, dir.join(exe_name(binary, None)))
        .with_context(|| format!("copy fake executable for {binary}"))
        .unwrap();
    }
    if topology.includes_runtime() {
      fs::write(dir.join("cadder.toml"), SAMPLE_CADDER_TOML)
        .with_context(|| format!("write {}", dir.join("cadder.toml").display()))
        .unwrap();
    }
  }

  fn build_fake_portable_tool(dir: &Path) -> PathBuf {
    let source = dir.join("fake_portable_tool.rs");
    let helper = dir.join(exe_name("fake-portable-tool", None));
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
    let mut command = Command::new(rustc);
    configure_hidden_child(&mut command);
    let status = command
      .arg(&source)
      .arg("-o")
      .arg(&helper)
      .status()
      .with_context(|| format!("compile {}", source.display()))
      .unwrap();
    assert!(status.success(), "failed to compile fake portable tool");
    helper
  }

  fn build_process_helper(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let source = dir.join("process_helper.rs");
    let helper = dir.join(exe_name("process-helper", None));
    fs::write(
      &source,
      r#"
use std::path::PathBuf;

fn main() {
  let args = std::env::args().collect::<Vec<_>>();
  match args.get(1).map(String::as_str) {
    Some("ok") => {}
    Some("fail") => std::process::exit(7),
    Some("cwd-env") => {
      let expected_cwd = args.get(2).map(PathBuf::from).expect("missing cwd");
      let expected_env = args.get(3).expect("missing env");
      if std::env::current_dir().ok().as_ref() != Some(&expected_cwd) {
        std::process::exit(8);
      }
      if std::env::var("CADDER_XTASK_PROCESS_HELPER").ok().as_ref() != Some(expected_env) {
        std::process::exit(9);
      }
    }
    other => {
      eprintln!("unexpected process helper argument: {other:?}");
      std::process::exit(10);
    }
  }
}
"#,
    )
    .unwrap();
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let mut command = Command::new(rustc);
    configure_hidden_child(&mut command);
    let status = command
      .arg(&source)
      .arg("-o")
      .arg(&helper)
      .status()
      .with_context(|| format!("compile {}", source.display()))
      .unwrap();
    assert!(status.success(), "failed to compile process helper");
    helper
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
    env::temp_dir().join(format!(
      "cadder-xtask-{name}-{}-{}",
      std::process::id(),
      unique_suffix()
    ))
  }

  fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_nanos()
  }

  fn expected_release_profile_manifest() -> &'static str {
    r#"
[workspace]
members = []

[profile.release]
opt-level = "s"
lto = "thin"
codegen-units = 1
debug = false
strip = "symbols"
panic = "unwind"

[profile.profiling]
inherits = "release"
debug = true
strip = "none"
"#
  }

  fn write_workspace_topology_fixture(root: &Path) {
    fs::write(root.join("Cargo.toml"), workspace_topology_manifest(&[])).unwrap();
    for contract in WORKSPACE_MEMBER_CONTRACTS {
      write_workspace_member_manifest(root, contract, contract.package);
    }
  }

  fn write_workspace_member_manifest(
    root: &Path,
    contract: WorkspaceMemberContract,
    package_name: &str,
  ) {
    let crate_dir = root.join(contract.path);
    fs::create_dir_all(&crate_dir).unwrap();
    fs::write(
      crate_dir.join("Cargo.toml"),
      format!(
        r#"
[package]
name = {package_name:?}
version = "0.1.0"
edition = "2024"
"#
      ),
    )
    .unwrap();
  }

  fn workspace_topology_manifest(extra_members: &[&str]) -> String {
    let mut manifest = String::from(
      r#"
[workspace]
members = [
"#,
    );
    for contract in WORKSPACE_MEMBER_CONTRACTS {
      manifest.push_str(&format!("  {:?},\n", contract.path));
    }
    for member in extra_members {
      manifest.push_str(&format!("  {member:?},\n"));
    }
    manifest.push_str(
      r#"]
"#,
    );
    manifest
  }
}
