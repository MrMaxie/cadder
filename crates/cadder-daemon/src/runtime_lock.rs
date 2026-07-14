use anyhow::{Context, Result, anyhow};
use cadder_protocol::{MIN_COMPATIBLE_PROTOCOL_VERSION, PROTOCOL_VERSION, ProtocolCapabilities};
use chrono::{DateTime, Utc};
use fs4::{FileExt, TryLockError};
use serde::{Deserialize, Serialize};
use std::{
  env,
  fs::{self, File, OpenOptions},
  io::Write,
  path::{Path, PathBuf},
  sync::{Arc, Mutex},
};

use crate::paths::RuntimePaths;
use crate::runtime_guard_record::{
  RuntimeGuardChildIdentity, RuntimeGuardReplacementBinding, RuntimeGuardReplacementProof,
};

const LOCK_METADATA_VERSION: u16 = 3;

#[derive(Debug)]
pub struct DaemonLock {
  _file: File,
  metadata_path: Option<PathBuf>,
  metadata_generation: Option<String>,
  retain_metadata: bool,
  recovery: Option<DaemonLockRecovery>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeContainmentMetadata {
  inner: Arc<Mutex<RuntimeContainmentMetadataInner>>,
}

#[derive(Debug)]
struct RuntimeContainmentMetadataInner {
  lock: DaemonLock,
  binding: RuntimeGuardReplacementBinding,
}

#[derive(Debug)]
pub(crate) struct DaemonLockCandidate {
  file: File,
  metadata_path: PathBuf,
  previous: Option<Box<DaemonLockMetadata>>,
}

impl DaemonLock {
  pub fn acquire(path: PathBuf) -> Result<Self> {
    Self::try_acquire(path.clone())?.ok_or_else(|| {
      anyhow!(
        "daemon lock {} is held by another cadderd owner",
        path.display()
      )
    })
  }

  pub fn try_acquire(path: PathBuf) -> Result<Option<Self>> {
    let file = match try_lock_file(&path)? {
      Some(file) => file,
      None => return Ok(None),
    };

    Ok(Some(Self {
      _file: file,
      metadata_path: None,
      metadata_generation: None,
      retain_metadata: false,
      recovery: None,
    }))
  }

  pub(crate) fn try_acquire_candidate(paths: &RuntimePaths) -> Result<Option<DaemonLockCandidate>> {
    let path = paths.lock_path();
    let metadata_path = paths.lock_metadata_path();
    let file = match try_lock_file(&path)? {
      Some(file) => file,
      None => return Ok(None),
    };

    let previous = match read_lock_metadata(&metadata_path)? {
      LockMetadataRead::Empty => None,
      LockMetadataRead::Valid(stale_owner) => Some(stale_owner),
      LockMetadataRead::Invalid { error } => {
        return Err(anyhow!(
          "daemon lock metadata {} is unreadable ({error}); Cadder cannot prove the previous runtime generation is stale. preserve its files for diagnosis",
          metadata_path.display()
        ));
      }
    };

    Ok(Some(DaemonLockCandidate {
      file,
      metadata_path,
      previous,
    }))
  }

  pub(crate) fn owner_generation(&self) -> Option<&str> {
    self.metadata_generation.as_deref()
  }

  pub(crate) fn attach_containment(
    &mut self,
    binding: RuntimeGuardReplacementBinding,
  ) -> Result<()> {
    let path = self
      .metadata_path
      .as_ref()
      .context("raw daemon locks cannot publish containment metadata")?;
    let generation = self
      .metadata_generation
      .as_deref()
      .context("daemon lock does not have an owner generation")?;
    let LockMetadataRead::Valid(mut metadata) = read_lock_metadata(path)? else {
      return Err(anyhow!(
        "daemon lock metadata changed before containment binding could be published"
      ));
    };
    if metadata.owner_generation != generation {
      return Err(anyhow!(
        "daemon lock generation changed before containment binding could be published"
      ));
    }
    if binding.generation.context.owner_generation != generation
      || binding.generation.context.profile != metadata.runtime_profile
      || binding.generation.context.runtime_id != metadata.instance_key
    {
      return Err(anyhow!(
        "runtime guard binding does not identify the active daemon lock generation"
      ));
    }
    metadata.containment = Some(binding);
    metadata.predecessor_containment = None;
    write_lock_metadata(path, &metadata)
      .with_context(|| format!("attach containment binding to {}", path.display()))?;
    self.retain_metadata = true;
    Ok(())
  }

  pub(crate) fn recovery(&self) -> Option<&DaemonLockRecovery> {
    self.recovery.as_ref()
  }

  pub(crate) fn active_owner_diagnostic(paths: &RuntimePaths) -> String {
    let lock_path = paths.lock_path();
    let metadata_path = paths.lock_metadata_path();
    match read_lock_metadata(&metadata_path) {
      Ok(LockMetadataRead::Empty) => {
        active_owner_without_metadata(paths, &lock_path, &metadata_path)
      }
      Ok(LockMetadataRead::Valid(metadata)) => {
        active_owner_from_metadata(paths, &lock_path, &metadata)
      }
      Ok(LockMetadataRead::Invalid { error }) => {
        active_owner_with_unreadable_metadata(paths, &lock_path, &metadata_path, &error)
      }
      Err(error) => {
        active_owner_with_unreadable_metadata(paths, &lock_path, &metadata_path, &error.to_string())
      }
    }
  }
}

impl RuntimeContainmentMetadata {
  pub(crate) fn new(mut lock: DaemonLock, binding: RuntimeGuardReplacementBinding) -> Result<Self> {
    lock.attach_containment(binding.clone())?;
    Ok(Self {
      inner: Arc::new(Mutex::new(RuntimeContainmentMetadataInner {
        lock,
        binding,
      })),
    })
  }

  pub(crate) fn update_last_child(&self, child: RuntimeGuardChildIdentity) -> Result<()> {
    let mut inner = self
      .inner
      .lock()
      .map_err(|_| anyhow!("runtime containment metadata lock is poisoned"))?;
    let mut binding = inner.binding.clone();
    binding.last_child = Some(child);
    inner.lock.attach_containment(binding.clone())?;
    inner.binding = binding;
    Ok(())
  }
}

impl DaemonLockCandidate {
  pub(crate) fn expected_containment(&self) -> Option<&RuntimeGuardReplacementBinding> {
    self
      .previous
      .as_deref()
      .and_then(DaemonLockMetadata::expected_containment)
  }

  pub(crate) fn publish_after_proof(
    self,
    paths: &RuntimePaths,
    proof: RuntimeGuardReplacementProof,
  ) -> Result<DaemonLock> {
    let replacement_is_proven = match (&self.previous, &proof) {
      (None, RuntimeGuardReplacementProof::FirstStart) => true,
      (
        Some(previous),
        RuntimeGuardReplacementProof::PreviousGenerationTerminated { binding, .. },
      ) => previous.expected_containment() == Some(binding.as_ref()),
      _ => false,
    };
    if !replacement_is_proven {
      return Err(anyhow!(
        "daemon lock metadata and runtime containment proof do not identify the same previous generation; preserve the runtime files for diagnosis"
      ));
    }

    let predecessor_containment = self
      .previous
      .as_deref()
      .and_then(DaemonLockMetadata::expected_containment)
      .cloned();
    let mut metadata = DaemonLockMetadata::new(paths)?;
    metadata.predecessor_containment = predecessor_containment.clone();
    let recovery = self
      .previous
      .map(|stale_owner| DaemonLockRecovery::ReplacedStaleOwner {
        stale_owner,
        recovered_by: Box::new(metadata.clone()),
      });
    write_lock_metadata(&self.metadata_path, &metadata).with_context(|| {
      format!(
        "write daemon lock metadata {} after containment proof",
        self.metadata_path.display()
      )
    })?;

    Ok(DaemonLock {
      _file: self.file,
      metadata_path: Some(self.metadata_path),
      metadata_generation: Some(metadata.owner_generation.clone()),
      retain_metadata: predecessor_containment.is_some(),
      recovery,
    })
  }
}

impl Drop for DaemonLock {
  fn drop(&mut self) {
    if self.retain_metadata {
      return;
    }
    let (Some(path), Some(generation)) = (&self.metadata_path, &self.metadata_generation) else {
      return;
    };
    let Ok(LockMetadataRead::Valid(current)) = read_lock_metadata(path) else {
      return;
    };
    if current.owner_generation == *generation {
      let _ = fs::remove_file(path);
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DaemonLockMetadata {
  pub metadata_version: u16,
  pub cadder_version: String,
  pub protocol_version: u16,
  pub minimum_compatible_protocol_version: u16,
  pub capabilities: ProtocolCapabilities,
  pub runtime_profile: String,
  pub runtime_dir: String,
  pub instance_key: String,
  pub socket_name: String,
  pub process_id: u32,
  pub owner_generation: String,
  pub acquired_at_utc: DateTime<Utc>,
  pub executable_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub containment: Option<RuntimeGuardReplacementBinding>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub predecessor_containment: Option<RuntimeGuardReplacementBinding>,
}

impl DaemonLockMetadata {
  fn new(paths: &RuntimePaths) -> Result<Self> {
    Ok(Self {
      metadata_version: LOCK_METADATA_VERSION,
      cadder_version: env!("CARGO_PKG_VERSION").to_string(),
      protocol_version: PROTOCOL_VERSION,
      minimum_compatible_protocol_version: MIN_COMPATIBLE_PROTOCOL_VERSION,
      capabilities: ProtocolCapabilities::current(),
      runtime_profile: paths.runtime_profile().to_string(),
      runtime_dir: paths.runtime_dir().display().to_string(),
      instance_key: paths.instance_key().to_string(),
      socket_name: paths.socket_name().to_string(),
      process_id: std::process::id(),
      owner_generation: new_owner_generation()?,
      acquired_at_utc: Utc::now(),
      executable_path: env::current_exe()
        .ok()
        .map(|path| path.display().to_string()),
      containment: None,
      predecessor_containment: None,
    })
  }

  fn owner_summary(&self) -> String {
    let executable = self
      .executable_path
      .as_deref()
      .unwrap_or("unknown executable");
    format!(
      "owner pid {}, Cadder {}, protocol {} (compatible {}..={}), profile `{}`, runtime `{}`, socket `{}`, executable `{}`",
      self.process_id,
      self.cadder_version,
      self.protocol_version,
      self.minimum_compatible_protocol_version,
      self.protocol_version,
      self.runtime_profile,
      self.runtime_dir,
      self.socket_name,
      executable
    )
  }

  fn expected_containment(&self) -> Option<&RuntimeGuardReplacementBinding> {
    self
      .containment
      .as_ref()
      .or(self.predecessor_containment.as_ref())
  }

  fn compatibility(&self) -> LockCompatibility {
    if self.metadata_version > LOCK_METADATA_VERSION {
      return LockCompatibility::Incompatible(format!(
        "lock metadata version {} is newer than supported version {}; update this cadderd before attaching",
        self.metadata_version, LOCK_METADATA_VERSION
      ));
    }

    if self.minimum_compatible_protocol_version > PROTOCOL_VERSION {
      return LockCompatibility::Incompatible(format!(
        "owner requires Cadder IPC protocol {} or newer while this cadderd supports up to {}; update this cadderd or stop the newer runtime",
        self.minimum_compatible_protocol_version, PROTOCOL_VERSION
      ));
    }

    if self.protocol_version < MIN_COMPATIBLE_PROTOCOL_VERSION {
      return LockCompatibility::Incompatible(format!(
        "owner uses Cadder IPC protocol {} which is older than this cadderd supports; update the older runtime before reuse",
        self.protocol_version
      ));
    }

    if self.protocol_version < PROTOCOL_VERSION {
      return LockCompatibility::Compatible(Some(format!(
        "owner uses older Cadder IPC protocol {}; update that older node when practical, but do not remove the lock while the process is active",
        self.protocol_version
      )));
    }

    if self.protocol_version > PROTOCOL_VERSION {
      return LockCompatibility::Compatible(Some(format!(
        "owner uses newer Cadder IPC protocol {}; update this cadderd if capability negotiation later fails",
        self.protocol_version
      )));
    }

    LockCompatibility::Compatible(None)
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DaemonLockRecovery {
  ReplacedStaleOwner {
    stale_owner: Box<DaemonLockMetadata>,
    recovered_by: Box<DaemonLockMetadata>,
  },
}

impl DaemonLockRecovery {
  pub(crate) fn log_message(&self) -> String {
    match self {
      Self::ReplacedStaleOwner {
        stale_owner,
        recovered_by,
      } => format!(
        "Recovered stale daemon lock metadata: previous {} no longer held the OS lock; new {}",
        stale_owner.owner_summary(),
        recovered_by.owner_summary()
      ),
    }
  }
}

fn new_owner_generation() -> Result<String> {
  let mut bytes = [0_u8; 16];
  getrandom::fill(&mut bytes).map_err(|error| anyhow!(error.to_string()))?;
  Ok(hex::encode(bytes))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LockCompatibility {
  Compatible(Option<String>),
  Incompatible(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LockMetadataRead {
  Empty,
  Valid(Box<DaemonLockMetadata>),
  Invalid { error: String },
}

fn try_lock_file(path: &Path) -> Result<Option<File>> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)
      .with_context(|| format!("create lock directory {}", parent.display()))?;
  }

  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .truncate(false)
    .open(path)
    .with_context(|| format!("open daemon lock {}", path.display()))?;
  match FileExt::try_lock(&file) {
    Ok(()) => Ok(Some(file)),
    Err(TryLockError::WouldBlock) => Ok(None),
    Err(TryLockError::Error(error)) => {
      Err(error).with_context(|| format!("acquire daemon lock {}", path.display()))
    }
  }
}

fn read_lock_metadata(path: &Path) -> Result<LockMetadataRead> {
  match fs::read_to_string(path) {
    Ok(content) if content.trim().is_empty() => Ok(LockMetadataRead::Empty),
    Ok(content) => match serde_json::from_str(content.trim()) {
      Ok(metadata) => Ok(LockMetadataRead::Valid(Box::new(metadata))),
      Err(error) => Ok(LockMetadataRead::Invalid {
        error: error.to_string(),
      }),
    },
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(LockMetadataRead::Empty),
    Err(error) => {
      Err(error).with_context(|| format!("read daemon lock metadata {}", path.display()))
    }
  }
}

fn write_lock_metadata(path: &Path, metadata: &DaemonLockMetadata) -> Result<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)
      .with_context(|| format!("create daemon lock metadata directory {}", parent.display()))?;
  }
  let mut file = OpenOptions::new()
    .write(true)
    .create(true)
    .truncate(true)
    .open(path)
    .with_context(|| format!("open daemon lock metadata {}", path.display()))?;
  serde_json::to_writer_pretty(&mut file, metadata)?;
  file.write_all(b"\n")?;
  file.sync_data()?;
  Ok(())
}

fn active_owner_from_metadata(
  paths: &RuntimePaths,
  lock_path: &Path,
  metadata: &DaemonLockMetadata,
) -> String {
  let recovery = recovery_options(paths);
  match metadata.compatibility() {
    LockCompatibility::Compatible(Some(note)) => format!(
      "active Cadder runtime owns daemon lock {}; {}; {}; {}",
      lock_path.display(),
      metadata.owner_summary(),
      note,
      recovery
    ),
    LockCompatibility::Compatible(None) => format!(
      "active Cadder runtime owns daemon lock {}; {}; {}",
      lock_path.display(),
      metadata.owner_summary(),
      recovery
    ),
    LockCompatibility::Incompatible(note) => format!(
      "active incompatible Cadder runtime owns daemon lock {}; {}; {}; {}",
      lock_path.display(),
      metadata.owner_summary(),
      note,
      recovery
    ),
  }
}

fn active_owner_without_metadata(
  paths: &RuntimePaths,
  lock_path: &Path,
  metadata_path: &Path,
) -> String {
  format!(
    "active Cadder runtime owns daemon lock {} but has not published owner metadata at {} yet; {}",
    lock_path.display(),
    metadata_path.display(),
    recovery_options(paths)
  )
}

fn active_owner_with_unreadable_metadata(
  paths: &RuntimePaths,
  lock_path: &Path,
  metadata_path: &Path,
  error: &str,
) -> String {
  format!(
    "active incompatible Cadder runtime owns daemon lock {} but owner metadata at {} could not be decoded ({error}); {}",
    lock_path.display(),
    metadata_path.display(),
    recovery_options(paths)
  )
}

fn recovery_options(_paths: &RuntimePaths) -> String {
  "recovery options: wait for that process to finish, stop the active owner if it is stale, then retry; do not delete runtime files while the owner process is active".to_string()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::runtime_guard_record::RuntimeGuardGenerationLock;

  #[test]
  fn raw_lock_rejects_second_owner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.lock");
    let _first = DaemonLock::acquire(path.clone()).unwrap();

    assert!(DaemonLock::acquire(path).is_err());
  }

  #[test]
  fn runtime_lock_writes_owner_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();

    let _lock = acquire_first_runtime_lock(&paths);
    let metadata = read_metadata_file(&paths);

    assert_eq!(
      metadata.runtime_dir,
      paths.runtime_dir().display().to_string()
    );
    assert_eq!(metadata.instance_key, paths.instance_key());
    assert_eq!(metadata.socket_name, paths.socket_name());
    assert_eq!(metadata.protocol_version, PROTOCOL_VERSION);
  }

  #[test]
  fn runtime_lock_candidate_preserves_stale_owner_metadata_until_proof() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let stale = stale_metadata(&paths, 1);
    fs::write(
      paths.lock_metadata_path(),
      serde_json::to_string(&stale).unwrap(),
    )
    .unwrap();

    let candidate = DaemonLock::try_acquire_candidate(&paths).unwrap().unwrap();
    assert!(candidate.expected_containment().is_none());
    let error = candidate
      .publish_after_proof(&paths, RuntimeGuardReplacementProof::FirstStart)
      .unwrap_err();

    assert!(error.to_string().contains("do not identify the same"));
    assert_eq!(read_metadata_file(&paths), stale);
  }

  #[test]
  fn runtime_lock_rejects_unreadable_previous_generation() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    fs::write(paths.lock_metadata_path(), "{not-json").unwrap();

    let error = DaemonLock::try_acquire_candidate(&paths).unwrap_err();

    assert!(error.to_string().contains("cannot prove"));
    assert!(error.to_string().contains("preserve its files"));
    assert_eq!(
      fs::read_to_string(paths.lock_metadata_path()).unwrap(),
      "{not-json"
    );
  }

  #[test]
  fn active_owner_diagnostic_reports_owner_identity_and_recovery_options() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let metadata = stale_metadata(&paths, 77);
    fs::write(
      paths.lock_metadata_path(),
      serde_json::to_string(&metadata).unwrap(),
    )
    .unwrap();
    let _lock = DaemonLock::acquire(paths.lock_path()).unwrap();

    let diagnostic = DaemonLock::active_owner_diagnostic(&paths);

    assert!(diagnostic.contains("active Cadder runtime owns daemon lock"));
    assert!(diagnostic.contains("owner pid 77"));
    assert!(diagnostic.contains("recovery options"));
  }

  #[test]
  fn active_owner_diagnostic_reports_incompatible_newer_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let mut metadata = stale_metadata(&paths, 77);
    metadata.metadata_version = LOCK_METADATA_VERSION + 1;
    fs::write(
      paths.lock_metadata_path(),
      serde_json::to_string(&metadata).unwrap(),
    )
    .unwrap();
    let _lock = DaemonLock::acquire(paths.lock_path()).unwrap();

    let diagnostic = DaemonLock::active_owner_diagnostic(&paths);

    assert!(diagnostic.contains("active incompatible Cadder runtime"));
    assert!(diagnostic.contains("update this cadderd"));
  }

  fn stale_metadata(paths: &RuntimePaths, process_id: u32) -> DaemonLockMetadata {
    DaemonLockMetadata {
      metadata_version: LOCK_METADATA_VERSION,
      cadder_version: "0.7.0".to_string(),
      protocol_version: PROTOCOL_VERSION,
      minimum_compatible_protocol_version: MIN_COMPATIBLE_PROTOCOL_VERSION,
      capabilities: ProtocolCapabilities::current(),
      runtime_profile: paths.runtime_profile().to_string(),
      runtime_dir: paths.runtime_dir().display().to_string(),
      instance_key: paths.instance_key().to_string(),
      socket_name: paths.socket_name().to_string(),
      process_id,
      owner_generation: "00112233445566778899aabbccddeeff".to_string(),
      acquired_at_utc: Utc::now(),
      executable_path: Some("old-cadderd".to_string()),
      containment: None,
      predecessor_containment: None,
    }
  }

  #[test]
  fn shutdown_storage_stale_lock_guard_does_not_remove_replacement_generation() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let lock = acquire_first_runtime_lock(&paths);
    let mut replacement = read_metadata_file(&paths);
    replacement.owner_generation = "ffeeddccbbaa99887766554433221100".to_string();
    write_lock_metadata(&paths.lock_metadata_path(), &replacement).unwrap();

    drop(lock);

    assert!(paths.lock_metadata_path().exists());
    assert_eq!(
      read_metadata_file(&paths).owner_generation,
      replacement.owner_generation
    );
  }

  fn read_metadata_file(paths: &RuntimePaths) -> DaemonLockMetadata {
    serde_json::from_str(&fs::read_to_string(paths.lock_metadata_path()).unwrap()).unwrap()
  }

  fn acquire_first_runtime_lock(paths: &RuntimePaths) -> DaemonLock {
    let candidate = DaemonLock::try_acquire_candidate(paths).unwrap().unwrap();
    let containment = RuntimeGuardGenerationLock::try_acquire(paths)
      .unwrap()
      .unwrap();
    let proof = containment.prove_replacement(None).unwrap();
    candidate.publish_after_proof(paths, proof).unwrap()
  }
}
