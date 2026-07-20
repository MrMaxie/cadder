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
const RUNTIME_RELEASE_PACKAGES: [&str; 3] = ["cadder-daemon", "cadder-client", "cadder-shim"];
const WORKSPACE_MEMBER_CONTRACTS: [WorkspaceMemberContract; 6] = [
  WorkspaceMemberContract::new(
    "crates/cadder-daemon",
    "cadder-daemon",
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
    "crates/cadder-client",
    "cadder-client",
    WorkspaceMemberClassification::OperatorClient,
    true,
  ),
  WorkspaceMemberContract::new(
    "crates/cadder-api",
    "cadder-api",
    WorkspaceMemberClassification::ClientApi,
    false,
  ),
  WorkspaceMemberContract::new(
    "crates/cadder-ipc",
    "cadder-ipc",
    WorkspaceMemberClassification::SharedIpc,
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
  ClientApi,
  SharedIpc,
  DocsTooling,
}

impl WorkspaceMemberClassification {
  fn label(self) -> &'static str {
    match self {
      Self::Daemon => "daemon",
      Self::Shim => "shim",
      Self::OperatorClient => "operator client",
      Self::ClientApi => "client API",
      Self::SharedIpc => "shared IPC",
      Self::DocsTooling => "docs/tooling",
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum PortableTopology {
  Runtime,
}

impl PortableTopology {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum ReleasePlatform {
  #[value(name = "windows-x64")]
  WindowsX64,
  #[value(name = "linux-x64")]
  LinuxX64,
  #[value(name = "macos-x64")]
  MacosX64,
  #[value(name = "macos-arm64")]
  MacosArm64,
}

impl ReleasePlatform {
  const ALL: [Self; 4] = [
    Self::WindowsX64,
    Self::LinuxX64,
    Self::MacosX64,
    Self::MacosArm64,
  ];

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum ReleaseAssetMode {
  #[value(name = "dry-run")]
  DryRun,
  Publish,
}
