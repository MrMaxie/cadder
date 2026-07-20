mod cli;
mod openspec_check;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser};
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
  process::{Command, ExitCode, Stdio},
};
use tar::Builder;
use tempfile::tempdir;
use toml::{Table, Value};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::cli::{Cli, Command as CliCommand};

include!("core.rs");

include!("commands.rs");
include!("portable.rs");
include!("installer_build.rs");
include!("installer_metadata.rs");
include!("release_verify.rs");
include!("release_build.rs");
include!("archive.rs");
include!("support.rs");
include!("options.rs");

fn main() -> ExitCode {
  match Cli::try_parse() {
    Ok(cli) => match run_cli(cli) {
      Ok(()) => ExitCode::SUCCESS,
      Err(error) => {
        eprintln!("Error: {error:#}");
        ExitCode::FAILURE
      }
    },
    Err(error) => {
      let exit_code = error.exit_code();
      let _ = error.print();
      ExitCode::from(exit_code as u8)
    }
  }
}

fn run_cli(cli: Cli) -> Result<()> {
  match cli.command {
    None | Some(CliCommand::Check) => check(),
    Some(CliCommand::Coverage(args)) => coverage(CoverageOptions {
      output_path: args
        .output
        .unwrap_or_else(|| PathBuf::from(DEFAULT_COVERAGE_REPORT_PATH)),
    }),
    Some(CliCommand::OpenspecCheck) => openspec_check_command(),
    Some(CliCommand::DocsCheck) => docs_check(),
    Some(CliCommand::DocsBuild) => docs_build(),
    Some(CliCommand::DevEnv(args)) => DevEnvironment::for_workspace()
      .print(args.format.unwrap_or_else(default_dev_env_format_value)),
    Some(CliCommand::DevRun(args)) => dev_run(args.program),
    Some(CliCommand::Dist(args)) => dist(DistOptions {
      out_dir: args.out,
      target: args.target,
      topology: args.topology,
    }),
    Some(CliCommand::VerifyDist(args)) => verify_dist(&VerifyDistOptions {
      dir: args.dir,
      target: args.target,
      topology: args.topology,
    }),
    Some(CliCommand::VerifyAssets) => verify_assets(),
    Some(CliCommand::VerifyReleaseAssets(args)) => verify_release_assets(&ReleaseAssetsOptions {
      dir: args.dir,
      version: args.version.map_or_else(workspace_package_version, Ok)?,
      mode: args.mode,
    }),
    Some(CliCommand::VerifyReleaseIdentity) => verify_release_identity(),
    Some(CliCommand::VerifyReleaseProfile) => verify_release_profile(),
    Some(CliCommand::VerifyWorkspaceTopology) => verify_workspace_topology(),
    Some(CliCommand::VerifyRuntimeInstallerDist(args)) => {
      let platform = args.platform.map_or_else(
        || ReleasePlatform::infer_from_target(args.target.as_deref()),
        Ok,
      )?;
      verify_runtime_installer_dist(&VerifyRuntimeInstallerDistOptions {
        dir: args.dir,
        version: args.version.map_or_else(workspace_package_version, Ok)?,
        platform,
      })
    }
    Some(CliCommand::Package(args)) => package(PackageOptions {
      out_dir: args.out,
      version: args.version.map_or_else(workspace_package_version, Ok)?,
      platform: args.platform.name().to_string(),
      target: args.target,
      topology: args.topology,
    }),
    Some(CliCommand::RuntimeInstaller(args)) => {
      let platform = args.platform.map_or_else(
        || ReleasePlatform::infer_from_target(args.target.as_deref()),
        Ok,
      )?;
      runtime_installer(RuntimeInstallerOptions {
        out_dir: args.out,
        version: args.version.map_or_else(workspace_package_version, Ok)?,
        target: args.target,
        platform,
        signing_mode: if args.sign {
          SigningMode::SignedRelease
        } else {
          SigningMode::UnsignedDryRun
        },
      })
    }
    Some(CliCommand::List) => {
      list_commands();
      Ok(())
    }
  }
}

fn list_commands() {
  for command in Cli::command().get_subcommands() {
    if command.get_name() != "help" {
      println!("{}", command.get_name());
    }
  }
}

fn default_dev_env_format_value() -> DevEnvFormat {
  if cfg!(windows) {
    DevEnvFormat::Powershell
  } else {
    DevEnvFormat::Bash
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  include!("tests/cli_parser.rs");
  include!("tests/contracts.rs");
  include!("tests/release_assets.rs");
  include!("tests/coverage.rs");
  include!("tests/portable.rs");
  include!("tests/support.rs");
}
