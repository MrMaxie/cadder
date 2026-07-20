use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::{DevEnvFormat, PortableTopology, ReleaseAssetMode, ReleasePlatform};

#[derive(Debug, Parser)]
#[command(
  name = "xtask",
  version,
  about = "Cadder repository task runner",
  long_about = "Cadder repository task runner. Run `cargo xtask <command>` from the workspace root."
)]
pub(crate) struct Cli {
  #[command(subcommand)]
  pub(crate) command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
  /// Run the complete repository validation suite.
  Check,
  /// Run cargo-llvm-cov and enforce the line coverage threshold.
  Coverage(CoverageArgs),
  /// Validate OpenSpec schemas, contracts, implementation evidence, and content boundaries.
  OpenspecCheck,
  /// Install documentation dependencies and run the Astro type/content check.
  DocsCheck,
  /// Install documentation dependencies and build the Starlight site.
  DocsBuild,
  /// Build and verify a local portable runtime layout.
  Dist(DistArgs),
  /// Build a versioned portable release archive and checksum.
  Package(PackageArgs),
  /// Build native runtime installer artifacts.
  RuntimeInstaller(RuntimeInstallerArgs),
  /// Verify a portable runtime layout.
  VerifyDist(VerifyDistArgs),
  /// Verify native runtime installer artifacts, manifests, and checksums.
  VerifyRuntimeInstallerDist(VerifyRuntimeInstallerDistArgs),
  /// Verify checked-in visual assets and documentation paths.
  VerifyAssets,
  /// Verify the public release artifact matrix.
  VerifyReleaseAssets(VerifyReleaseAssetsArgs),
  /// Verify the root Cargo release profile policy.
  VerifyReleaseProfile,
  /// Verify synchronized version and download metadata.
  VerifyReleaseIdentity,
  /// Verify that workspace members match the documented topology.
  VerifyWorkspaceTopology,
  /// Print the repeatable development runtime environment.
  DevEnv(DevEnvArgs),
  /// Run a program inside the repeatable development runtime environment.
  DevRun(DevRunArgs),
  /// List the canonical command names, one per line.
  List,
}

#[derive(Debug, Args)]
pub(crate) struct CoverageArgs {
  /// Path for the generated LCOV report.
  #[arg(long)]
  pub(crate) output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct DistArgs {
  /// Directory that receives the portable runtime layout.
  #[arg(long)]
  pub(crate) out: PathBuf,
  /// Rust target triple used to locate release binaries.
  #[arg(long)]
  pub(crate) target: Option<String>,
  /// Portable application topology to build.
  #[arg(long, value_enum, default_value_t = PortableTopology::Runtime)]
  pub(crate) topology: PortableTopology,
}

#[derive(Debug, Args)]
pub(crate) struct PackageArgs {
  /// Directory that receives the release archive.
  #[arg(long)]
  pub(crate) out: PathBuf,
  /// Release version. Defaults to workspace.package.version.
  #[arg(long)]
  pub(crate) version: Option<String>,
  /// Target release platform.
  #[arg(long)]
  pub(crate) platform: ReleasePlatform,
  /// Rust target triple used to locate release binaries.
  #[arg(long)]
  pub(crate) target: Option<String>,
  /// Portable application topology to package.
  #[arg(long, value_enum, default_value_t = PortableTopology::Runtime)]
  pub(crate) topology: PortableTopology,
}

#[derive(Debug, Args)]
pub(crate) struct RuntimeInstallerArgs {
  /// Directory that receives native installer artifacts.
  #[arg(long)]
  pub(crate) out: PathBuf,
  /// Release version. Defaults to workspace.package.version.
  #[arg(long)]
  pub(crate) version: Option<String>,
  /// Rust target triple used to infer the release platform when --platform is omitted.
  #[arg(long)]
  pub(crate) target: Option<String>,
  /// Target release platform. Defaults to the target triple or host platform.
  #[arg(long)]
  pub(crate) platform: Option<ReleasePlatform>,
  /// Sign platform artifacts using the configured release credentials.
  #[arg(long)]
  pub(crate) sign: bool,
}

#[derive(Debug, Args)]
pub(crate) struct VerifyDistArgs {
  /// Portable runtime layout to verify.
  #[arg(long)]
  pub(crate) dir: PathBuf,
  /// Rust target triple that determines executable suffixes.
  #[arg(long)]
  pub(crate) target: Option<String>,
  /// Portable application topology to verify.
  #[arg(long, value_enum, default_value_t = PortableTopology::Runtime)]
  pub(crate) topology: PortableTopology,
}

#[derive(Debug, Args)]
pub(crate) struct VerifyRuntimeInstallerDistArgs {
  /// Directory containing native installer artifacts.
  #[arg(long)]
  pub(crate) dir: PathBuf,
  /// Release version. Defaults to workspace.package.version.
  #[arg(long)]
  pub(crate) version: Option<String>,
  /// Rust target triple used to infer the release platform when --platform is omitted.
  #[arg(long)]
  pub(crate) target: Option<String>,
  /// Target release platform. Defaults to the target triple or host platform.
  #[arg(long)]
  pub(crate) platform: Option<ReleasePlatform>,
}

#[derive(Debug, Args)]
pub(crate) struct VerifyReleaseAssetsArgs {
  /// Directory containing all release assets.
  #[arg(long)]
  pub(crate) dir: PathBuf,
  /// Release version. Defaults to workspace.package.version.
  #[arg(long)]
  pub(crate) version: Option<String>,
  /// Verify a dry-run matrix or release-publish matrix.
  #[arg(long, value_enum, default_value_t = ReleaseAssetMode::DryRun)]
  pub(crate) mode: ReleaseAssetMode,
}

#[derive(Debug, Args)]
pub(crate) struct DevEnvArgs {
  /// Output format for environment assignments.
  #[arg(long, value_enum)]
  pub(crate) format: Option<DevEnvFormat>,
}

#[derive(Debug, Args)]
pub(crate) struct DevRunArgs {
  /// Program and arguments to run. Use `--` before a program that starts with a dash.
  #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
  pub(crate) program: Vec<String>,
}

#[cfg(test)]
mod tests {
  use clap::error::ErrorKind;

  use super::*;

  fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("xtask").chain(args.iter().copied()))
  }

  #[test]
  fn defaults_to_check_without_a_subcommand() {
    assert!(parse(&[]).unwrap().command.is_none());
  }

  #[test]
  fn parses_typed_dist_arguments() {
    let cli = parse(&[
      "dist",
      "--out",
      "target/dist",
      "--target",
      "x86_64-unknown-linux-gnu",
    ])
    .unwrap();

    let Some(Command::Dist(args)) = cli.command else {
      panic!("expected dist command");
    };
    assert_eq!(args.out, PathBuf::from("target/dist"));
    assert_eq!(args.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
    assert_eq!(args.topology, PortableTopology::Runtime);
  }

  #[test]
  fn accepts_cli_aliases_and_rejects_unknown_values() {
    let cli = parse(&["dev-env", "--format", "pwsh"]).unwrap();
    let Some(Command::DevEnv(args)) = cli.command else {
      panic!("expected dev-env command");
    };
    assert_eq!(args.format, Some(DevEnvFormat::Powershell));

    let error = parse(&["dist", "--out", "target/dist", "--topology", "everything"]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidValue);
  }

  #[test]
  fn requires_command_options_and_preserves_dev_run_arguments() {
    let error = parse(&["package", "--out", "target/package"]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);

    let cli = parse(&["dev-run", "--", "tool", "--flag"]).unwrap();
    let Some(Command::DevRun(args)) = cli.command else {
      panic!("expected dev-run command");
    };
    assert_eq!(args.program, ["tool", "--flag"]);
  }
}
