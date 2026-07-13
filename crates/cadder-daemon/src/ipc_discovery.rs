//! Crash-safe publication and validation of the local IPC endpoint.

use crate::{
  IpcClientError, IpcClientPhase, IpcClientResult, IpcPrincipal, LocalIpcErrorCode,
  LocalIpcErrorKind, RuntimePaths, current_privilege_status,
  ipc_client_error::LocalIpcErrorContext,
};
use anyhow::{Context, Result, bail};
use cadder_protocol::{
  CapabilityId, ProtocolVersionRange, SUPPORTED_PROTOCOL_VERSIONS, capabilities,
};
use chrono::{DateTime, Utc};
use fs4::{FileExt, TryLockError};
use serde::{Deserialize, Serialize};
use std::{
  fs::{self, File},
  io::{self, Write},
  path::{Path, PathBuf},
};
use tokio::time::{Instant, timeout_at};

const IPC_DISCOVERY_SCHEMA_VERSION: u16 = 2;
const DISCOVERY_TEMP_PREFIX: &str = ".cadder-ipc.";
const DISCOVERY_TEMP_SUFFIX: &str = ".tmp";
const RANDOM_ID_BYTES: usize = 16;

/// The operating-system endpoint selected by the daemon instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "camelCase")]
pub enum IpcEndpoint {
  #[serde(rename_all = "camelCase")]
  UnixSocket { path: PathBuf },
  #[serde(rename_all = "camelCase")]
  WindowsNamedPipe { name: String },
}

/// Owner-readable metadata that identifies one live daemon publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcEndpointMetadata {
  pub schema_version: u16,
  pub cadder_version: String,
  pub profile: String,
  pub runtime_id: String,
  pub daemon_instance_id: String,
  pub endpoint: IpcEndpoint,
  pub supported_versions: ProtocolVersionRange,
  pub capabilities: Box<[CapabilityId]>,
  pub publication_generation: String,
  pub process_id: u32,
  pub published_at_utc: DateTime<Utc>,
}

impl IpcEndpointMetadata {
  /// Creates metadata for a new daemon instance and publication generation.
  pub fn new(paths: &RuntimePaths) -> io::Result<Self> {
    Ok(Self {
      schema_version: IPC_DISCOVERY_SCHEMA_VERSION,
      cadder_version: env!("CARGO_PKG_VERSION").to_string(),
      profile: paths.runtime_profile().to_string(),
      runtime_id: paths.instance_key().to_string(),
      daemon_instance_id: random_id()?,
      endpoint: current_endpoint(paths),
      supported_versions: SUPPORTED_PROTOCOL_VERSIONS,
      capabilities: capabilities::ALL
        .iter()
        .map(|capability| CapabilityId::parse(*capability).expect("protocol capability is valid"))
        .collect(),
      publication_generation: random_id()?,
      process_id: std::process::id(),
      published_at_utc: Utc::now(),
    })
  }

  /// Creates metadata after proving that the process has an authenticated runtime-owner identity.
  pub fn current(paths: &RuntimePaths) -> io::Result<Self> {
    IpcPrincipal::current_process(current_privilege_status())?;
    Self::new(paths)
  }

  fn validate_for(&self, paths: &RuntimePaths) -> io::Result<()> {
    if self.schema_version != IPC_DISCOVERY_SCHEMA_VERSION {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
          "unsupported IPC discovery schema {}; expected {}",
          self.schema_version, IPC_DISCOVERY_SCHEMA_VERSION
        ),
      ));
    }
    if self.profile != paths.runtime_profile().as_str() || self.runtime_id != paths.instance_key() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "IPC discovery does not identify the selected runtime",
      ));
    }
    if self.endpoint != current_endpoint(paths) {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "IPC discovery endpoint does not belong to the selected runtime",
      ));
    }
    validate_random_id("daemon instance", &self.daemon_instance_id)?;
    validate_random_id("publication generation", &self.publication_generation)?;
    Ok(())
  }
}

/// A generation-aware guard for one authoritative discovery publication.
#[derive(Debug)]
pub struct IpcEndpointPublication {
  paths: RuntimePaths,
  runtime_id: String,
  daemon_instance_id: String,
  publication_generation: String,
  active: bool,
}

impl IpcEndpointPublication {
  pub fn publish_current(paths: &RuntimePaths) -> Result<Self> {
    let metadata = IpcEndpointMetadata::current(paths)
      .context("authenticate the Cadder runtime-owner identity")?;
    Self::publish(paths, &metadata)
  }

  pub fn publish(paths: &RuntimePaths, metadata: &IpcEndpointMetadata) -> Result<Self> {
    publish_with(paths, metadata, || Ok(()), || Ok(()))?;
    Ok(Self {
      paths: paths.clone(),
      runtime_id: metadata.runtime_id.clone(),
      daemon_instance_id: metadata.daemon_instance_id.clone(),
      publication_generation: metadata.publication_generation.clone(),
      active: true,
    })
  }

  /// Removes only the still-current generation and leaves the persistent lock file in place.
  pub fn cleanup(&mut self) -> Result<()> {
    self.cleanup_with(cleanup_matching_publication)
  }

  /// Removes the current generation within the normal budget or retains process ownership
  /// until the non-interruptible filesystem cleanup finishes.
  pub(crate) async fn cleanup_until(&mut self, deadline: Instant) -> Result<()> {
    if !self.active {
      return Ok(());
    }
    let paths = self.paths.clone();
    let runtime_id = self.runtime_id.clone();
    let daemon_instance_id = self.daemon_instance_id.clone();
    let publication_generation = self.publication_generation.clone();
    let mut cleanup = tokio::task::spawn_blocking(move || {
      cleanup_publication_owned(
        &paths,
        &runtime_id,
        &daemon_instance_id,
        &publication_generation,
      )
    });

    if let Ok(joined) = timeout_at(deadline, &mut cleanup).await {
      joined.context("join IPC discovery cleanup task")??;
      self.active = false;
      return Ok(());
    }

    cleanup
      .await
      .context("join contained IPC discovery cleanup task")??;
    self.active = false;
    bail!(
      "IPC discovery cleanup exceeded its normal shutdown budget and completed under fail-stop containment"
    )
  }

  fn cleanup_with(
    &mut self,
    cleanup: impl FnOnce(&RuntimePaths, &str, &str, &str) -> Result<()>,
  ) -> Result<()> {
    if !self.active {
      return Ok(());
    }
    let lock = open_publication_lock(&self.paths)?;
    FileExt::lock(&lock)?;
    let result = cleanup(
      &self.paths,
      &self.runtime_id,
      &self.daemon_instance_id,
      &self.publication_generation,
    );
    let unlocked = FileExt::unlock(&lock);
    result?;
    unlocked?;
    self.active = false;
    Ok(())
  }
}

fn cleanup_publication_owned(
  paths: &RuntimePaths,
  runtime_id: &str,
  daemon_instance_id: &str,
  publication_generation: &str,
) -> Result<()> {
  let lock = open_publication_lock(paths)?;
  FileExt::lock(&lock)?;
  let result = cleanup_matching_publication(
    paths,
    runtime_id,
    daemon_instance_id,
    publication_generation,
  );
  let unlocked = FileExt::unlock(&lock);
  result?;
  unlocked?;
  Ok(())
}

impl Drop for IpcEndpointPublication {
  fn drop(&mut self) {
    if !self.active {
      return;
    }
    let Ok(lock) = open_publication_lock(&self.paths) else {
      return;
    };
    match FileExt::try_lock(&lock) {
      Ok(()) => {
        let _ = cleanup_matching_publication(
          &self.paths,
          &self.runtime_id,
          &self.daemon_instance_id,
          &self.publication_generation,
        );
        let _ = FileExt::unlock(&lock);
      }
      Err(TryLockError::WouldBlock) | Err(TryLockError::Error(_)) => {}
    }
  }
}

/// Reads and validates the authoritative discovery document for the selected runtime.
pub fn discover_ipc_endpoint(paths: &RuntimePaths) -> IpcClientResult<IpcEndpointMetadata> {
  let lock = open_publication_lock(paths).map_err(discovery_read_error)?;
  FileExt::lock_shared(&lock).map_err(discovery_read_error)?;
  let content = fs::read(paths.ipc_endpoint_path());
  let unlocked = FileExt::unlock(&lock);
  let content = content.map_err(discovery_read_error)?;
  unlocked.map_err(discovery_read_error)?;
  let metadata: IpcEndpointMetadata =
    serde_json::from_slice(&content).map_err(discovery_decode_error)?;
  metadata
    .validate_for(paths)
    .map_err(discovery_validation_error)?;
  Ok(metadata)
}

fn publish_with(
  paths: &RuntimePaths,
  metadata: &IpcEndpointMetadata,
  before_replace: impl FnOnce() -> Result<()>,
  after_replace: impl FnOnce() -> Result<()>,
) -> Result<()> {
  metadata.validate_for(paths)?;
  paths.ensure_dirs()?;
  let lock = open_publication_lock(paths)?;
  FileExt::lock(&lock)?;
  let result = publish_while_locked(paths, metadata, before_replace, after_replace);
  FileExt::unlock(&lock)?;
  result
}

fn publish_while_locked(
  paths: &RuntimePaths,
  metadata: &IpcEndpointMetadata,
  before_replace: impl FnOnce() -> Result<()>,
  after_replace: impl FnOnce() -> Result<()>,
) -> Result<()> {
  cleanup_stale_temporary_files(paths, metadata)?;
  let destination = paths.ipc_endpoint_path();
  let temporary = temporary_path(paths, &metadata.publication_generation);
  let bytes = serialize_metadata(metadata)?;
  let mut file = create_owner_only_runtime_file(paths, &temporary)
    .with_context(|| format!("create temporary IPC discovery {}", temporary.display()))?;
  let mut installed = false;
  let publication = (|| -> Result<()> {
    file.write_all(&bytes)?;
    flush_file(&file)?;
    drop(file);
    before_replace()?;
    install_discovery_file(&temporary, &destination)?;
    installed = true;
    finalize_discovery_file(&destination)?;
    after_replace()?;
    let published = read_metadata(&destination)?;
    if published != *metadata {
      bail!("published IPC discovery does not match its complete candidate");
    }
    Ok(())
  })();
  if publication.is_err() {
    let _ = fs::remove_file(&temporary);
    if installed {
      cleanup_matching_publication(
        paths,
        &metadata.runtime_id,
        &metadata.daemon_instance_id,
        &metadata.publication_generation,
      )
      .context("remove an installed discovery generation after publication failed")?;
    }
  }
  publication.with_context(|| format!("publish IPC discovery {}", destination.display()))
}

fn serialize_metadata(metadata: &IpcEndpointMetadata) -> Result<Vec<u8>> {
  let mut bytes = serde_json::to_vec_pretty(metadata)?;
  bytes.push(b'\n');
  Ok(bytes)
}

fn read_metadata(path: &Path) -> Result<IpcEndpointMetadata> {
  let bytes = fs::read(path)?;
  Ok(serde_json::from_slice(&bytes)?)
}

fn cleanup_matching_publication(
  paths: &RuntimePaths,
  runtime_id: &str,
  daemon_instance_id: &str,
  publication_generation: &str,
) -> Result<()> {
  cleanup_matching_publication_with(
    paths,
    runtime_id,
    daemon_instance_id,
    publication_generation,
    read_metadata,
  )
}

fn cleanup_matching_publication_with(
  paths: &RuntimePaths,
  runtime_id: &str,
  daemon_instance_id: &str,
  publication_generation: &str,
  read: impl FnOnce(&Path) -> Result<IpcEndpointMetadata>,
) -> Result<()> {
  let path = paths.ipc_endpoint_path();
  let metadata = match read(&path) {
    Ok(metadata) => metadata,
    Err(error) if is_not_found(&error) => return Ok(()),
    Err(error) if error.downcast_ref::<serde_json::Error>().is_some() => return Ok(()),
    Err(error) => return Err(error),
  };
  if metadata.schema_version == IPC_DISCOVERY_SCHEMA_VERSION
    && metadata.runtime_id == runtime_id
    && metadata.daemon_instance_id == daemon_instance_id
    && metadata.publication_generation == publication_generation
  {
    fs::remove_file(&path)?;
    sync_parent_directory(&path)?;
  }
  Ok(())
}

fn is_not_found(error: &anyhow::Error) -> bool {
  error
    .downcast_ref::<io::Error>()
    .is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
}

fn cleanup_stale_temporary_files(
  paths: &RuntimePaths,
  current: &IpcEndpointMetadata,
) -> Result<()> {
  for entry in fs::read_dir(paths.runtime_dir())? {
    let entry = entry?;
    let file_name = entry.file_name();
    let file_name = file_name.to_string_lossy();
    let Some(generation) = temporary_generation(&file_name) else {
      continue;
    };
    let path = entry.path();
    if validate_owner_only_runtime_file(&path).is_err() {
      continue;
    }
    let Ok(metadata) = read_metadata(&path) else {
      continue;
    };
    if metadata.schema_version == IPC_DISCOVERY_SCHEMA_VERSION
      && metadata.profile == current.profile
      && metadata.runtime_id == current.runtime_id
      && metadata.publication_generation == generation
    {
      fs::remove_file(path)?;
    }
  }
  Ok(())
}

fn temporary_path(paths: &RuntimePaths, generation: &str) -> PathBuf {
  paths.runtime_dir().join(format!(
    "{DISCOVERY_TEMP_PREFIX}{generation}{DISCOVERY_TEMP_SUFFIX}"
  ))
}

fn temporary_generation(name: &str) -> Option<&str> {
  name
    .strip_prefix(DISCOVERY_TEMP_PREFIX)?
    .strip_suffix(DISCOVERY_TEMP_SUFFIX)
    .filter(|generation| validate_random_id("temporary generation", generation).is_ok())
}

fn random_id() -> io::Result<String> {
  let mut bytes = [0_u8; RANDOM_ID_BYTES];
  getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
  Ok(hex::encode(bytes))
}

fn validate_random_id(label: &str, value: &str) -> io::Result<()> {
  if value.len() == RANDOM_ID_BYTES * 2
    && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    && value.bytes().all(|byte| !byte.is_ascii_uppercase())
  {
    return Ok(());
  }
  Err(io::Error::new(
    io::ErrorKind::InvalidData,
    format!("IPC discovery {label} ID is not 128-bit lowercase hexadecimal"),
  ))
}

#[cfg(unix)]
fn current_endpoint(paths: &RuntimePaths) -> IpcEndpoint {
  IpcEndpoint::UnixSocket {
    path: crate::ipc_unix_security::unix_socket_path(paths),
  }
}

#[cfg(windows)]
fn current_endpoint(paths: &RuntimePaths) -> IpcEndpoint {
  IpcEndpoint::WindowsNamedPipe {
    name: paths.socket_name().to_string(),
  }
}

#[cfg(not(any(unix, windows)))]
fn current_endpoint(paths: &RuntimePaths) -> IpcEndpoint {
  IpcEndpoint::WindowsNamedPipe {
    name: paths.socket_name().to_string(),
  }
}

fn open_publication_lock(paths: &RuntimePaths) -> io::Result<File> {
  ensure_runtime_dir(paths)?;
  open_platform_publication_lock(paths)
}

#[cfg(unix)]
fn ensure_runtime_dir(paths: &RuntimePaths) -> io::Result<()> {
  crate::ipc_unix_security::secure_runtime_paths(paths)
}

#[cfg(windows)]
fn ensure_runtime_dir(paths: &RuntimePaths) -> io::Result<()> {
  fs::create_dir_all(paths.runtime_dir())?;
  crate::ipc_windows_security::secure_owner_only_runtime_directory(paths.runtime_dir())
}

#[cfg(not(any(unix, windows)))]
fn ensure_runtime_dir(paths: &RuntimePaths) -> io::Result<()> {
  fs::create_dir_all(paths.runtime_dir())
}

#[cfg(unix)]
fn open_platform_publication_lock(paths: &RuntimePaths) -> io::Result<File> {
  crate::ipc_unix_security::open_owner_only_lock_file(paths, &paths.ipc_discovery_lock_path())
}

#[cfg(windows)]
fn open_platform_publication_lock(paths: &RuntimePaths) -> io::Result<File> {
  crate::ipc_windows_security::open_owner_only_lock_file(&paths.ipc_discovery_lock_path())
}

#[cfg(not(any(unix, windows)))]
fn open_platform_publication_lock(paths: &RuntimePaths) -> io::Result<File> {
  std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .open(paths.ipc_discovery_lock_path())
}

#[cfg(unix)]
fn create_owner_only_runtime_file(paths: &RuntimePaths, path: &Path) -> io::Result<File> {
  crate::ipc_unix_security::create_owner_only_runtime_file(paths, path)
}

#[cfg(windows)]
fn create_owner_only_runtime_file(_paths: &RuntimePaths, path: &Path) -> io::Result<File> {
  crate::ipc_windows_security::create_owner_only_runtime_file(path)
}

#[cfg(not(any(unix, windows)))]
fn create_owner_only_runtime_file(_paths: &RuntimePaths, path: &Path) -> io::Result<File> {
  std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(path)
}

#[cfg(unix)]
fn validate_owner_only_runtime_file(path: &Path) -> io::Result<()> {
  crate::ipc_unix_security::validate_owner_only_runtime_file(path)
}

#[cfg(windows)]
fn validate_owner_only_runtime_file(path: &Path) -> io::Result<()> {
  crate::ipc_windows_security::validate_owner_only_runtime_file(path)
}

#[cfg(not(any(unix, windows)))]
fn validate_owner_only_runtime_file(path: &Path) -> io::Result<()> {
  path.metadata().map(|_| ())
}

#[cfg(unix)]
fn flush_file(file: &File) -> io::Result<()> {
  file.sync_all()
}

#[cfg(windows)]
fn flush_file(file: &File) -> io::Result<()> {
  crate::ipc_windows_security::flush_file(file)
}

#[cfg(not(any(unix, windows)))]
fn flush_file(file: &File) -> io::Result<()> {
  file.sync_all()
}

#[cfg(unix)]
fn install_discovery_file(temporary: &Path, destination: &Path) -> io::Result<()> {
  fs::rename(temporary, destination)
}

#[cfg(unix)]
fn finalize_discovery_file(destination: &Path) -> io::Result<()> {
  crate::ipc_unix_security::sync_parent_directory(destination)
}

#[cfg(windows)]
fn install_discovery_file(temporary: &Path, destination: &Path) -> io::Result<()> {
  crate::ipc_windows_security::install_discovery_file(temporary, destination)
}

#[cfg(windows)]
fn finalize_discovery_file(destination: &Path) -> io::Result<()> {
  crate::ipc_windows_security::secure_owner_only_path(destination)
}

#[cfg(not(any(unix, windows)))]
fn install_discovery_file(temporary: &Path, destination: &Path) -> io::Result<()> {
  fs::rename(temporary, destination)
}

#[cfg(not(any(unix, windows)))]
fn finalize_discovery_file(_destination: &Path) -> io::Result<()> {
  Ok(())
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> io::Result<()> {
  crate::ipc_unix_security::sync_parent_directory(path)
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> io::Result<()> {
  Ok(())
}

fn discovery_read_error(error: io::Error) -> IpcClientError {
  let (code, message, guidance, retryable) = match error.kind() {
    io::ErrorKind::PermissionDenied => (
      LocalIpcErrorCode::PermissionDenied,
      "Cadder cannot read IPC discovery for this runtime; no request was sent.",
      "Use the account that owns this Cadder runtime or select an accessible profile.",
      false,
    ),
    io::ErrorKind::NotFound => (
      LocalIpcErrorCode::DiscoveryUnavailable,
      "Cadder IPC discovery is unavailable; no request was sent.",
      "Start the Cadder daemon for this runtime, then retry.",
      true,
    ),
    io::ErrorKind::Interrupted => (
      LocalIpcErrorCode::DiscoveryReadFailed,
      "Cadder could not finish reading IPC discovery; no request was sent.",
      "Retry once. If the error remains, inspect the runtime-directory diagnostics.",
      true,
    ),
    _ => (
      LocalIpcErrorCode::DiscoveryReadFailed,
      "Cadder could not read IPC discovery; no request was sent.",
      "Inspect the runtime directory and local filesystem diagnostics before retrying.",
      false,
    ),
  };
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Discovery,
    phase: IpcClientPhase::DiscoveryRead,
    code,
    message: message.into(),
    guidance: Some(guidance.into()),
    retryable,
    request_id: None,
    operation: Some("discover-ipc-endpoint".into()),
    source: Some(Box::new(error)),
  })
}

fn discovery_decode_error(error: serde_json::Error) -> IpcClientError {
  invalid_discovery_error(Box::new(error))
}

fn discovery_validation_error(error: io::Error) -> IpcClientError {
  invalid_discovery_error(Box::new(error))
}

fn invalid_discovery_error(source: Box<dyn std::error::Error + Send + Sync>) -> IpcClientError {
  IpcClientError::local(LocalIpcErrorContext {
    kind: LocalIpcErrorKind::Discovery,
    phase: IpcClientPhase::DiscoveryDecode,
    code: LocalIpcErrorCode::InvalidDiscovery,
    message: "Cadder IPC discovery is invalid; no request was sent.".into(),
    guidance: Some(
      "Restart the Cadder daemon for this runtime. If the error remains, inspect the discovery diagnostics."
        .into(),
    ),
    retryable: false,
    request_id: None,
    operation: Some("discover-ipc-endpoint".into()),
    source: Some(source),
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  };

  #[test]
  fn discovery_publication_roundtrips_complete_versioned_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let metadata = IpcEndpointMetadata::new(&paths).unwrap();
    let mut publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();

    assert_eq!(discover_ipc_endpoint(&paths).unwrap(), metadata);
    assert!(paths.ipc_discovery_lock_path().exists());

    publication.cleanup().unwrap();
    assert!(!paths.ipc_endpoint_path().exists());
    assert!(paths.ipc_discovery_lock_path().exists());
  }

  #[tokio::test]
  async fn shutdown_storage_cleanup_until_removes_the_owned_generation() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let metadata = IpcEndpointMetadata::new(&paths).unwrap();
    let mut publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();

    publication
      .cleanup_until(Instant::now() + tokio::time::Duration::from_secs(1))
      .await
      .unwrap();

    assert!(!paths.ipc_endpoint_path().exists());
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn shutdown_storage_cleanup_retains_ownership_after_the_normal_budget() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let metadata = IpcEndpointMetadata::new(&paths).unwrap();
    let publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();
    let lock = open_publication_lock(&paths).unwrap();
    FileExt::lock(&lock).unwrap();
    let cleanup = tokio::spawn(async move {
      let mut publication = publication;
      publication
        .cleanup_until(Instant::now() + tokio::time::Duration::from_millis(25))
        .await
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(75)).await;
    assert!(!cleanup.is_finished());
    assert!(paths.ipc_endpoint_path().exists());

    FileExt::unlock(&lock).unwrap();
    let error = cleanup.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("fail-stop containment"));
    assert!(!paths.ipc_endpoint_path().exists());
  }

  #[test]
  fn discovery_publication_failure_before_replace_preserves_previous_generation() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let previous = IpcEndpointMetadata::new(&paths).unwrap();
    let _publication = IpcEndpointPublication::publish(&paths, &previous).unwrap();
    let candidate = IpcEndpointMetadata::new(&paths).unwrap();

    let error = publish_with(
      &paths,
      &candidate,
      || bail!("injected before replace"),
      || Ok(()),
    )
    .unwrap_err();

    assert!(error.to_string().contains("publish IPC discovery"));
    assert_eq!(discover_ipc_endpoint(&paths).unwrap(), previous);
    assert!(!temporary_path(&paths, &candidate.publication_generation).exists());
  }

  #[test]
  fn discovery_publication_failure_after_replace_removes_the_installed_generation() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let candidate = IpcEndpointMetadata::new(&paths).unwrap();

    let error = publish_with(
      &paths,
      &candidate,
      || Ok(()),
      || bail!("injected after replace"),
    )
    .unwrap_err();

    assert!(error.to_string().contains("publish IPC discovery"));
    assert!(!paths.ipc_endpoint_path().exists());
    assert!(!temporary_path(&paths, &candidate.publication_generation).exists());
  }

  #[test]
  fn shutdown_storage_discovery_publication_old_guard_never_removes_a_new_generation() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let first = IpcEndpointMetadata::new(&paths).unwrap();
    let old_guard = IpcEndpointPublication::publish(&paths, &first).unwrap();
    let second = IpcEndpointMetadata::new(&paths).unwrap();
    let mut current_guard = IpcEndpointPublication::publish(&paths, &second).unwrap();

    drop(old_guard);

    assert_eq!(discover_ipc_endpoint(&paths).unwrap(), second);
    current_guard.cleanup().unwrap();
  }

  #[test]
  fn discovery_publication_failed_cleanup_remains_retryable() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let metadata = IpcEndpointMetadata::new(&paths).unwrap();
    let mut publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();

    let error = publication
      .cleanup_with(|_, _, _, _| bail!("injected cleanup failure"))
      .unwrap_err();

    assert!(error.to_string().contains("injected cleanup failure"));
    assert!(publication.active);
    assert!(paths.ipc_endpoint_path().exists());
    publication.cleanup().unwrap();
    assert!(!paths.ipc_endpoint_path().exists());
  }

  #[test]
  fn discovery_publication_operational_read_failure_keeps_cleanup_retryable() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let metadata = IpcEndpointMetadata::new(&paths).unwrap();
    let mut publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();

    let error = publication
      .cleanup_with(|paths, runtime_id, instance_id, generation| {
        cleanup_matching_publication_with(paths, runtime_id, instance_id, generation, |_| {
          Err(
            io::Error::new(
              io::ErrorKind::PermissionDenied,
              "injected discovery read denial",
            )
            .into(),
          )
        })
      })
      .unwrap_err();

    assert_eq!(
      error.downcast_ref::<io::Error>().unwrap().kind(),
      io::ErrorKind::PermissionDenied
    );
    assert!(publication.active);
    publication.cleanup().unwrap();
  }

  #[test]
  fn discovery_publication_cleanup_leaves_malformed_and_unknown_documents_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let metadata = IpcEndpointMetadata::new(&paths).unwrap();
    let mut publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();
    fs::write(paths.ipc_endpoint_path(), b"{not-json}\n").unwrap();

    publication.cleanup().unwrap();

    assert_eq!(
      fs::read(paths.ipc_endpoint_path()).unwrap(),
      b"{not-json}\n"
    );

    let replacement = IpcEndpointMetadata::new(&paths).unwrap();
    let mut replacement_publication =
      IpcEndpointPublication::publish(&paths, &replacement).unwrap();
    let mut unknown = serde_json::to_value(&replacement).unwrap();
    unknown["schemaVersion"] = serde_json::json!(IPC_DISCOVERY_SCHEMA_VERSION + 1);
    fs::write(
      paths.ipc_endpoint_path(),
      serde_json::to_vec_pretty(&unknown).unwrap(),
    )
    .unwrap();

    replacement_publication.cleanup().unwrap();

    assert!(paths.ipc_endpoint_path().exists());
    assert_eq!(
      read_metadata(&paths.ipc_endpoint_path())
        .unwrap()
        .schema_version,
      IPC_DISCOVERY_SCHEMA_VERSION + 1
    );
  }

  #[test]
  fn discovery_publication_removes_only_verified_stale_temporary_generations() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let stale = IpcEndpointMetadata::new(&paths).unwrap();
    let stale_path = temporary_path(&paths, &stale.publication_generation);
    let mut stale_file = create_owner_only_runtime_file(&paths, &stale_path).unwrap();
    stale_file
      .write_all(&serialize_metadata(&stale).unwrap())
      .unwrap();
    flush_file(&stale_file).unwrap();
    drop(stale_file);
    let unverified_path = temporary_path(&paths, &random_id().unwrap());
    let mut unverified_file = create_owner_only_runtime_file(&paths, &unverified_path).unwrap();
    unverified_file.write_all(b"{not-json}\n").unwrap();
    flush_file(&unverified_file).unwrap();
    drop(unverified_file);
    let current = IpcEndpointMetadata::new(&paths).unwrap();

    let mut publication = IpcEndpointPublication::publish(&paths, &current).unwrap();

    assert!(!stale_path.exists());
    assert!(unverified_path.exists());
    publication.cleanup().unwrap();
  }

  #[test]
  fn discovery_publication_readers_observe_only_complete_generations() {
    let temp = tempfile::tempdir().unwrap();
    let paths = Arc::new(RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap());
    let initial = IpcEndpointMetadata::new(&paths).unwrap();
    let mut guards = vec![IpcEndpointPublication::publish(&paths, &initial).unwrap()];
    let running = Arc::new(AtomicBool::new(true));
    let reader_paths = Arc::clone(&paths);
    let reader_running = Arc::clone(&running);
    let reader = std::thread::spawn(move || {
      while reader_running.load(Ordering::Relaxed) {
        discover_ipc_endpoint(&reader_paths).unwrap();
      }
    });

    for _ in 0..25 {
      let metadata = IpcEndpointMetadata::new(&paths).unwrap();
      guards.push(IpcEndpointPublication::publish(&paths, &metadata).unwrap());
    }
    running.store(false, Ordering::Relaxed);
    reader.join().unwrap();
    guards.last_mut().unwrap().cleanup().unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn discovery_publication_uses_owner_only_unix_modes() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let metadata = IpcEndpointMetadata::new(&paths).unwrap();
    let mut publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();

    assert_eq!(
      fs::metadata(paths.ipc_endpoint_path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777,
      0o600
    );
    assert_eq!(
      fs::metadata(paths.ipc_discovery_lock_path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777,
      0o600
    );
    publication.cleanup().unwrap();
  }

  #[test]
  fn discovery_publication_rejects_metadata_for_another_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let first = RuntimePaths::resolve(Some(temp.path().join("first"))).unwrap();
    let second = RuntimePaths::resolve(Some(temp.path().join("second"))).unwrap();
    let metadata = IpcEndpointMetadata::new(&first).unwrap();

    assert!(IpcEndpointPublication::publish(&second, &metadata).is_err());
  }

  #[test]
  fn discovery_publication_rejects_an_endpoint_for_another_runtime_or_platform() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let other = RuntimePaths::resolve(Some(temp.path().join("other"))).unwrap();
    let mut metadata = IpcEndpointMetadata::new(&paths).unwrap();
    metadata.endpoint = current_endpoint(&other);

    assert!(IpcEndpointPublication::publish(&paths, &metadata).is_err());

    metadata.endpoint = match current_endpoint(&paths) {
      IpcEndpoint::UnixSocket { .. } => IpcEndpoint::WindowsNamedPipe {
        name: paths.socket_name().to_string(),
      },
      IpcEndpoint::WindowsNamedPipe { .. } => IpcEndpoint::UnixSocket {
        path: paths.runtime_dir().join("cadder.sock"),
      },
    };
    assert!(IpcEndpointPublication::publish(&paths, &metadata).is_err());
  }

  #[test]
  fn discovery_rejected_runtime_path_preserves_permission_classification() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    let runtime = temp.path().join("runtime-link");
    fs::create_dir(&target).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &runtime).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&target, &runtime).unwrap();
    let paths = RuntimePaths::resolve(Some(runtime)).unwrap();

    let error = discover_ipc_endpoint(&paths).unwrap_err();

    assert_eq!(
      error.local_error().unwrap().code(),
      LocalIpcErrorCode::PermissionDenied
    );
  }

  #[cfg(windows)]
  #[test]
  fn discovery_publication_supports_long_windows_runtime_paths() {
    use std::os::windows::ffi::OsStrExt;

    let temp = tempfile::tempdir().unwrap();
    let mut runtime = temp.path().to_path_buf();
    for index in 0..12 {
      runtime.push(format!("runtime-segment-{index:02}-abcdefgh"));
    }
    assert!(runtime.as_os_str().encode_wide().count() > 260);
    let paths = RuntimePaths::resolve(Some(runtime)).unwrap();
    let metadata = IpcEndpointMetadata::new(&paths).unwrap();

    let mut publication = IpcEndpointPublication::publish(&paths, &metadata).unwrap();

    assert_eq!(discover_ipc_endpoint(&paths).unwrap(), metadata);
    publication.cleanup().unwrap();
  }

  #[test]
  fn discovery_publication_temporary_name_requires_exact_generation() {
    assert_eq!(
      temporary_generation(".cadder-ipc.00112233445566778899aabbccddeeff.tmp"),
      Some("00112233445566778899aabbccddeeff")
    );
    assert_eq!(temporary_generation(".cadder-ipc.not-random.tmp"), None);
    assert_eq!(
      temporary_generation("cadder-ipc.00112233445566778899aabbccddeeff.tmp"),
      None
    );
  }

  #[test]
  fn discovery_errors_preserve_read_and_decode_classification() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let missing = discover_ipc_endpoint(&paths).unwrap_err();
    assert_eq!(
      missing.local_error().unwrap().code(),
      LocalIpcErrorCode::DiscoveryUnavailable
    );

    paths.ensure_dirs().unwrap();
    fs::write(paths.ipc_endpoint_path(), b"{not-json}").unwrap();
    let malformed = discover_ipc_endpoint(&paths).unwrap_err();
    assert_eq!(
      malformed.local_error().unwrap().code(),
      LocalIpcErrorCode::InvalidDiscovery
    );
  }
}
