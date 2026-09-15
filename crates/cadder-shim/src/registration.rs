use anyhow::{Context, Result};
use cadder_ipc::{
  ActivationState, EntrypointInstanceIdentity, EntrypointRegistration, LogStreamIdentity,
  OwnerProcessIdentity, ShimRunMetadata, SourcePath,
};
use chrono::Utc;
use std::{
  env,
  path::{Path, PathBuf},
};

pub(crate) fn build_registration(args: &[String]) -> Result<EntrypointRegistration> {
  let now = Utc::now();
  let cwd = env::current_dir().context("resolve current directory")?;
  let (config_path, adapter) = parse_run_args(args, &cwd);
  let canonical_cwd = cwd.canonicalize().ok();
  let canonical_config = config_path.canonicalize().ok();
  let identity = EntrypointInstanceIdentity::new(now);
  let executable_path = env::current_exe().ok();

  Ok(EntrypointRegistration {
    registration_id: identity.instance_id.clone(),
    source_working_directory: SourcePath::new(
      cwd.display().to_string(),
      canonical_cwd.map(|path| path.display().to_string()),
    ),
    source_config_path: SourcePath::new(
      config_path.display().to_string(),
      canonical_config.map(|path| path.display().to_string()),
    ),
    registered_domains: Vec::new(),
    activation_state: ActivationState::Active,
    owner_process: OwnerProcessIdentity {
      process_id: std::process::id(),
      process_start_time_utc: now,
      shim_session_nonce: identity.shim_session_nonce.clone(),
      executable_path: executable_path.map(|path| path.display().to_string()),
    },
    log_stream: LogStreamIdentity::entrypoint(&identity.instance_id),
    shim_run: Some(ShimRunMetadata {
      adapter,
      raw_arguments: args.to_vec(),
      command_line: args.join(" "),
    }),
    created_at_utc: now,
    last_heartbeat_utc: now,
    entrypoint_instance: identity,
  })
}

pub(crate) fn parse_run_args(args: &[String], cwd: &Path) -> (PathBuf, Option<String>) {
  let mut config = None;
  let mut adapter = None;
  let mut iter = args.iter().skip(1);
  while let Some(arg) = iter.next() {
    match arg.as_str() {
      "--config" | "-c" => config = iter.next().map(PathBuf::from),
      "--adapter" | "-a" => adapter = iter.next().cloned(),
      _ => {}
    }
  }
  (config.unwrap_or_else(|| cwd.join("Caddyfile")), adapter)
}

#[cfg(test)]
mod tests;
