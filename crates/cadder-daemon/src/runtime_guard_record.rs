//! Durable ownership evidence for one runtime-guard generation.

use crate::RuntimePaths;
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use fs4::{FileExt, TryLockError};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
  fmt,
  fs::{self, File},
  io::{self, Read, Write},
  path::{Path, PathBuf},
};

const RECORD_SCHEMA_VERSION: u16 = 1;
const GUARD_PROTOCOL_REVISION: u16 = 1;
const RANDOM_ID_BYTES: usize = 16;
const GENERATION_NONCE_BYTES: usize = 32;
const MAX_RECORD_BYTES: u64 = 64 * 1024;
const TEMPORARY_PREFIX: &str = ".cadder-containment.";
const TEMPORARY_SUFFIX: &str = ".tmp";
const NONCE_COMMITMENT_DOMAIN: &[u8] = b"cadder-runtime-guard-generation-v1\0";

/// A freshly generated secret and its durable one-way commitment.
pub(crate) struct RuntimeGuardGeneration {
  nonce: [u8; GENERATION_NONCE_BYTES],
  commitment: String,
}

impl RuntimeGuardGeneration {
  /// Creates an unpredictable generation secret using the operating-system RNG.
  pub(crate) fn random() -> Result<Self> {
    let mut nonce = [0_u8; GENERATION_NONCE_BYTES];
    getrandom::fill(&mut nonce).map_err(|error| io::Error::other(error.to_string()))?;
    let commitment = nonce_commitment(&nonce);
    Ok(Self { nonce, commitment })
  }

  /// Returns the lowercase hexadecimal secret for the authenticated bootstrap channel.
  pub(crate) fn nonce(&self) -> String {
    hex::encode(self.nonce)
  }

  /// Returns the commitment stored in owner-only runtime metadata.
  pub(crate) fn commitment(&self) -> &str {
    &self.commitment
  }

  /// Checks a bootstrap nonce against a stored commitment without short-circuiting on bytes.
  pub(crate) fn nonce_matches(nonce: &str, expected_commitment: &str) -> bool {
    let Ok(decoded) = hex::decode(nonce) else {
      return false;
    };
    if decoded.len() != GENERATION_NONCE_BYTES {
      return false;
    }
    let actual = nonce_commitment(&decoded);
    constant_time_eq(actual.as_bytes(), expected_commitment.as_bytes())
  }
}

impl fmt::Debug for RuntimeGuardGeneration {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("RuntimeGuardGeneration")
      .field("nonce", &"[REDACTED]")
      .field("commitment", &self.commitment)
      .finish()
  }
}

/// Stable operating-system identity for one process instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardProcessIdentity {
  pub process_id: u32,
  pub creation_identity: String,
}

/// Stable file identity and content digest for an executable image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardImageIdentity {
  pub path: PathBuf,
  pub file_identity: String,
  pub sha256: String,
}

/// Identity of the guard process and the exact executable image it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardIdentity {
  pub process: RuntimeGuardProcessIdentity,
  pub image: RuntimeGuardImageIdentity,
}

/// Compatibility evidence for the pinned Caddy image owned by the guard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardPinnedCaddyIdentity {
  pub image: RuntimeGuardImageIdentity,
  pub version: String,
  pub probe_revision: String,
}

/// Identity of the optional Caddy tree created by one guard generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardChildIdentity {
  pub child_generation: String,
  pub process: RuntimeGuardProcessIdentity,
  pub pinned_caddy: RuntimeGuardPinnedCaddyIdentity,
}

/// Lifecycle state written only by the process that owns the generation lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeGuardRecordState {
  Preparing,
  Ready,
  Terminal,
}

/// Why the guard published a terminal empty-tree proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeGuardTerminalReason {
  CleanFinalize,
  OwnerChannelClosed,
  GuardFailure,
}

/// Terminal details emitted after the exact owned tree has been joined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardTerminalOutcome {
  pub reason: RuntimeGuardTerminalReason,
  pub child_exit_code: Option<i32>,
  pub completed_at_utc: DateTime<Utc>,
}

/// Context shared by every record transition for one daemon generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGuardGenerationContext {
  pub profile: String,
  pub runtime_id: String,
  pub daemon_instance_id: String,
  pub owner_generation: String,
  pub nonce_commitment: String,
}

/// Owner-only durable evidence for one runtime-guard generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RuntimeGuardRecord {
  pub(crate) schema_version: u16,
  pub(crate) protocol_revision: u16,
  #[serde(flatten)]
  pub(crate) context: RuntimeGuardGenerationContext,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub(crate) guard: Option<RuntimeGuardIdentity>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub(crate) child: Option<RuntimeGuardChildIdentity>,
  pub(crate) state: RuntimeGuardRecordState,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub(crate) tree_empty: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub(crate) terminal: Option<RuntimeGuardTerminalOutcome>,
}

impl RuntimeGuardRecord {
  /// Creates the candidate-owned record written before guard bootstrap.
  pub(crate) fn preparing(context: RuntimeGuardGenerationContext) -> Self {
    Self {
      schema_version: RECORD_SCHEMA_VERSION,
      protocol_revision: GUARD_PROTOCOL_REVISION,
      context,
      guard: None,
      child: None,
      state: RuntimeGuardRecordState::Preparing,
      tree_empty: None,
      terminal: None,
    }
  }

  /// Creates the guard-owned record that permits daemon readiness.
  pub(crate) fn ready(
    context: RuntimeGuardGenerationContext,
    guard: RuntimeGuardIdentity,
    child: Option<RuntimeGuardChildIdentity>,
  ) -> Self {
    Self {
      schema_version: RECORD_SCHEMA_VERSION,
      protocol_revision: GUARD_PROTOCOL_REVISION,
      context,
      guard: Some(guard),
      child,
      state: RuntimeGuardRecordState::Ready,
      tree_empty: None,
      terminal: None,
    }
  }

  /// Creates the terminal proof written only after joining the owned process tree.
  pub(crate) fn terminal(
    context: RuntimeGuardGenerationContext,
    guard: RuntimeGuardIdentity,
    child: Option<RuntimeGuardChildIdentity>,
    terminal: RuntimeGuardTerminalOutcome,
  ) -> Self {
    Self {
      schema_version: RECORD_SCHEMA_VERSION,
      protocol_revision: GUARD_PROTOCOL_REVISION,
      context,
      guard: Some(guard),
      child,
      state: RuntimeGuardRecordState::Terminal,
      tree_empty: Some(true),
      terminal: Some(terminal),
    }
  }

  /// Returns the immutable identities that stale daemon metadata must retain.
  pub(crate) fn replacement_binding(&self) -> Result<RuntimeGuardReplacementBinding> {
    let guard = self
      .guard
      .clone()
      .ok_or_else(|| anyhow::anyhow!("runtime containment record has no guard identity"))?;
    Ok(RuntimeGuardReplacementBinding {
      context: self.context.clone(),
      guard,
      child: self.child.clone(),
    })
  }

  fn validate_for(&self, paths: &RuntimePaths) -> Result<()> {
    ensure!(
      self.schema_version == RECORD_SCHEMA_VERSION,
      "unsupported runtime containment schema {}; expected {}",
      self.schema_version,
      RECORD_SCHEMA_VERSION
    );
    ensure!(
      self.protocol_revision == GUARD_PROTOCOL_REVISION,
      "unsupported runtime guard protocol revision {}; expected {}",
      self.protocol_revision,
      GUARD_PROTOCOL_REVISION
    );
    ensure!(
      self.context.profile == paths.runtime_profile().as_str()
        && self.context.runtime_id == paths.instance_key(),
      "runtime containment record does not identify the selected runtime"
    );
    validate_random_id("daemon instance", &self.context.daemon_instance_id)?;
    validate_random_id("daemon owner generation", &self.context.owner_generation)?;
    validate_sha256(
      "generation nonce commitment",
      &self.context.nonce_commitment,
    )?;

    match self.state {
      RuntimeGuardRecordState::Preparing => {
        ensure!(
          self.guard.is_none()
            && self.child.is_none()
            && self.tree_empty.is_none()
            && self.terminal.is_none(),
          "preparing runtime containment record contains unearned ownership evidence"
        );
      }
      RuntimeGuardRecordState::Ready => {
        ensure!(
          self.guard.is_some() && self.tree_empty.is_none() && self.terminal.is_none(),
          "ready runtime containment record has invalid terminal evidence"
        );
      }
      RuntimeGuardRecordState::Terminal => {
        ensure!(
          self.guard.is_some() && self.tree_empty == Some(true) && self.terminal.is_some(),
          "terminal runtime containment record does not prove an empty joined tree"
        );
      }
    }

    if let Some(guard) = &self.guard {
      validate_guard_identity(guard)?;
    }
    if let Some(child) = &self.child {
      validate_random_id("Caddy child generation", &child.child_generation)?;
      validate_process_identity(&child.process)?;
      validate_image_identity(&child.pinned_caddy.image)?;
      Version::parse(&child.pinned_caddy.version)
        .context("runtime containment record contains an invalid Caddy semantic version")?;
      validate_bounded_text("Caddy probe revision", &child.pinned_caddy.probe_revision)?;
    }
    Ok(())
  }
}

/// Exact stale-generation identities required for replacement proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RuntimeGuardReplacementBinding {
  pub(crate) context: RuntimeGuardGenerationContext,
  pub(crate) guard: RuntimeGuardIdentity,
  pub(crate) child: Option<RuntimeGuardChildIdentity>,
}

/// Result of validating prior containment while holding the generation lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeGuardReplacementProof {
  FirstStart,
  PreviousGenerationTerminated {
    binding: Box<RuntimeGuardReplacementBinding>,
    completed_at_utc: DateTime<Utc>,
  },
}

/// Exclusive generation lock used by both candidate daemon and live guard.
#[derive(Debug)]
pub(crate) struct RuntimeGuardGenerationLock {
  paths: RuntimePaths,
  _file: File,
}

impl RuntimeGuardGenerationLock {
  /// Attempts to acquire the owner-only containment lock without waiting.
  pub(crate) fn try_acquire(paths: &RuntimePaths) -> Result<Option<Self>> {
    paths.ensure_dirs()?;
    let file = open_owner_only_lock_file(paths)?;
    match FileExt::try_lock(&file) {
      Ok(()) => Ok(Some(Self {
        paths: paths.clone(),
        _file: file,
      })),
      Err(TryLockError::WouldBlock) => Ok(None),
      Err(TryLockError::Error(error)) => Err(error).with_context(|| {
        format!(
          "acquire runtime containment lock {}",
          paths.containment_lock_path().display()
        )
      }),
    }
  }

  /// Atomically publishes a validated record while this generation owns the lock.
  pub(crate) fn publish(&self, record: &RuntimeGuardRecord) -> Result<()> {
    record.validate_for(&self.paths)?;
    publish_record(&self.paths, record)
  }

  /// Proves first start or exact previous-generation termination without signalling any process.
  pub(crate) fn prove_replacement(
    &self,
    expected: Option<&RuntimeGuardReplacementBinding>,
  ) -> Result<RuntimeGuardReplacementProof> {
    let record = match read_record(&self.paths) {
      Ok(Some(record)) => record,
      Ok(None) if expected.is_none() => return Ok(RuntimeGuardReplacementProof::FirstStart),
      Ok(None) => bail!(
        "runtime containment proof is missing for stale daemon metadata; leave recorded processes untouched and inspect the runtime profile before retrying"
      ),
      Err(error) => {
        return Err(error).context(
          "runtime containment proof is unreadable or untrusted; leave recorded processes untouched and inspect the runtime profile before retrying",
        );
      }
    };
    record.validate_for(&self.paths)?;
    let Some(expected) = expected else {
      bail!(
        "runtime containment proof exists without matching stale daemon metadata; leave recorded processes untouched and inspect the runtime profile before retrying"
      );
    };
    ensure!(
      record.state == RuntimeGuardRecordState::Terminal
        && record.tree_empty == Some(true)
        && record.terminal.is_some(),
      "previous runtime containment generation has not published a terminal empty-tree proof; leave recorded processes untouched and retry after the guard completes"
    );
    let actual = record.replacement_binding()?;
    ensure!(
      actual == *expected,
      "runtime containment proof does not match stale daemon metadata; leave recorded processes untouched and inspect the runtime profile before retrying"
    );
    let completed_at_utc = record
      .terminal
      .as_ref()
      .expect("terminal record was checked")
      .completed_at_utc;
    Ok(RuntimeGuardReplacementProof::PreviousGenerationTerminated {
      binding: Box::new(actual),
      completed_at_utc,
    })
  }
}

fn publish_record(paths: &RuntimePaths, record: &RuntimeGuardRecord) -> Result<()> {
  let mut bytes = serde_json::to_vec_pretty(record)?;
  bytes.push(b'\n');
  ensure!(
    bytes.len() as u64 <= MAX_RECORD_BYTES,
    "runtime containment record exceeds the {MAX_RECORD_BYTES}-byte limit"
  );

  let destination = paths.containment_record_path();
  let temporary = allocate_temporary_path(paths)?;
  let mut file = create_owner_only_runtime_file(paths, &temporary).with_context(|| {
    format!(
      "create runtime containment candidate {}",
      temporary.display()
    )
  })?;
  let result = (|| -> Result<()> {
    file.write_all(&bytes)?;
    flush_file(&file)?;
    drop(file);
    install_owner_only_file(&temporary, &destination)?;
    finalize_owner_only_file(&destination)?;
    let published = read_record(paths)?.ok_or_else(|| {
      anyhow::anyhow!("published runtime containment record disappeared after replacement")
    })?;
    ensure!(
      published == *record,
      "published runtime containment record does not match its complete candidate"
    );
    Ok(())
  })();
  if result.is_err() {
    let _ = fs::remove_file(&temporary);
  }
  result.with_context(|| {
    format!(
      "publish runtime containment record {}",
      destination.display()
    )
  })
}

fn read_record(paths: &RuntimePaths) -> Result<Option<RuntimeGuardRecord>> {
  let path = paths.containment_record_path();
  let file = match open_owner_only_runtime_file(&path) {
    Ok(file) => file,
    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error).with_context(|| format!("open {}", path.display())),
  };
  let length = file.metadata()?.len();
  ensure!(
    length <= MAX_RECORD_BYTES,
    "runtime containment record exceeds the {MAX_RECORD_BYTES}-byte limit"
  );
  let mut bytes = Vec::with_capacity(length as usize);
  file
    .take(MAX_RECORD_BYTES + 1)
    .read_to_end(&mut bytes)
    .with_context(|| format!("read {}", path.display()))?;
  ensure!(
    bytes.len() as u64 <= MAX_RECORD_BYTES,
    "runtime containment record exceeds the {MAX_RECORD_BYTES}-byte limit"
  );
  serde_json::from_slice(&bytes)
    .map(Some)
    .with_context(|| format!("decode runtime containment record {}", path.display()))
}

fn allocate_temporary_path(paths: &RuntimePaths) -> Result<PathBuf> {
  for _ in 0..8 {
    let mut random = [0_u8; RANDOM_ID_BYTES];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    let path = paths.runtime_dir().join(format!(
      "{TEMPORARY_PREFIX}{}{TEMPORARY_SUFFIX}",
      hex::encode(random)
    ));
    if !path.exists() {
      return Ok(path);
    }
  }
  bail!("could not allocate a unique runtime containment candidate after 8 attempts")
}

fn nonce_commitment(nonce: &[u8]) -> String {
  let mut hasher = Sha256::new();
  hasher.update(NONCE_COMMITMENT_DOMAIN);
  hasher.update(nonce);
  hex::encode(hasher.finalize())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
  if left.len() != right.len() {
    return false;
  }
  left
    .iter()
    .zip(right)
    .fold(0_u8, |difference, (left, right)| {
      difference | (left ^ right)
    })
    == 0
}

fn validate_guard_identity(identity: &RuntimeGuardIdentity) -> Result<()> {
  validate_process_identity(&identity.process)?;
  validate_image_identity(&identity.image)
}

fn validate_process_identity(identity: &RuntimeGuardProcessIdentity) -> Result<()> {
  ensure!(identity.process_id != 0, "recorded process ID is zero");
  validate_bounded_text("process creation identity", &identity.creation_identity)
}

fn validate_image_identity(identity: &RuntimeGuardImageIdentity) -> Result<()> {
  ensure!(
    identity.path.is_absolute(),
    "recorded executable path is not absolute"
  );
  validate_bounded_text("executable file identity", &identity.file_identity)?;
  validate_sha256("executable digest", &identity.sha256)
}

fn validate_bounded_text(label: &str, value: &str) -> Result<()> {
  ensure!(
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control),
    "runtime containment {label} is empty, oversized, or contains control characters"
  );
  Ok(())
}

fn validate_random_id(label: &str, value: &str) -> Result<()> {
  ensure!(
    value.len() == RANDOM_ID_BYTES * 2
      && value
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
    "runtime containment {label} is not 128-bit lowercase hexadecimal"
  );
  Ok(())
}

fn validate_sha256(label: &str, value: &str) -> Result<()> {
  ensure!(
    value.len() == 64
      && value
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
    "runtime containment {label} is not a lowercase SHA-256 digest"
  );
  Ok(())
}

#[cfg(unix)]
fn open_owner_only_lock_file(paths: &RuntimePaths) -> io::Result<File> {
  crate::ipc_unix_security::open_owner_only_lock_file(paths, &paths.containment_lock_path())
}

#[cfg(windows)]
fn open_owner_only_lock_file(paths: &RuntimePaths) -> io::Result<File> {
  crate::ipc_windows_security::open_owner_only_lock_file(&paths.containment_lock_path())
}

#[cfg(not(any(unix, windows)))]
fn open_owner_only_lock_file(paths: &RuntimePaths) -> io::Result<File> {
  std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .truncate(false)
    .open(paths.containment_lock_path())
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
fn open_owner_only_runtime_file(path: &Path) -> io::Result<File> {
  crate::ipc_unix_security::open_owner_only_runtime_file(path)
}

#[cfg(windows)]
fn open_owner_only_runtime_file(path: &Path) -> io::Result<File> {
  crate::ipc_windows_security::open_owner_only_runtime_file(path)
}

#[cfg(not(any(unix, windows)))]
fn open_owner_only_runtime_file(path: &Path) -> io::Result<File> {
  File::open(path)
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
fn install_owner_only_file(temporary: &Path, destination: &Path) -> io::Result<()> {
  if destination.exists() {
    crate::ipc_unix_security::secure_owner_only_runtime_file(destination)?;
  }
  fs::rename(temporary, destination)
}

#[cfg(windows)]
fn install_owner_only_file(temporary: &Path, destination: &Path) -> io::Result<()> {
  crate::ipc_windows_security::install_discovery_file(temporary, destination)
}

#[cfg(not(any(unix, windows)))]
fn install_owner_only_file(temporary: &Path, destination: &Path) -> io::Result<()> {
  fs::rename(temporary, destination)
}

#[cfg(unix)]
fn finalize_owner_only_file(destination: &Path) -> io::Result<()> {
  crate::ipc_unix_security::sync_parent_directory(destination)
}

#[cfg(windows)]
fn finalize_owner_only_file(destination: &Path) -> io::Result<()> {
  crate::ipc_windows_security::secure_owner_only_path(destination)
}

#[cfg(not(any(unix, windows)))]
fn finalize_owner_only_file(_destination: &Path) -> io::Result<()> {
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn runtime_guard_generation_uses_a_256_bit_nonce_and_redacts_debug_output() {
    let generation = RuntimeGuardGeneration::random().unwrap();
    let nonce = generation.nonce();

    assert_eq!(nonce.len(), GENERATION_NONCE_BYTES * 2);
    assert!(RuntimeGuardGeneration::nonce_matches(
      &nonce,
      generation.commitment()
    ));
    assert!(!format!("{generation:?}").contains(&nonce));
  }

  #[test]
  fn runtime_guard_record_rejects_tree_empty_before_terminal() {
    let (_temp, paths) = fixture_paths();
    let mut record = RuntimeGuardRecord::ready(context(&paths), guard_identity(), None);
    record.tree_empty = Some(true);

    let error = record.validate_for(&paths).unwrap_err();

    assert!(error.to_string().contains("invalid terminal evidence"));
  }

  #[test]
  fn runtime_guard_record_serializes_tree_empty_only_for_terminal_state() {
    let (_temp, paths) = fixture_paths();
    let ready = RuntimeGuardRecord::ready(context(&paths), guard_identity(), None);
    let terminal = terminal_record(&paths);

    let ready_json = serde_json::to_value(ready).unwrap();
    let terminal_json = serde_json::to_value(terminal).unwrap();

    assert!(ready_json.get("treeEmpty").is_none());
    assert_eq!(
      terminal_json.get("treeEmpty"),
      Some(&serde_json::json!(true))
    );
  }

  #[test]
  fn runtime_guard_record_is_atomic_owner_only_json_on_windows() {
    let (_temp, paths) = fixture_paths();
    let lock = RuntimeGuardGenerationLock::try_acquire(&paths)
      .unwrap()
      .unwrap();
    let record = RuntimeGuardRecord::ready(context(&paths), guard_identity(), None);

    lock.publish(&record).unwrap();

    assert_eq!(read_record(&paths).unwrap(), Some(record));
    #[cfg(windows)]
    crate::ipc_windows_security::validate_owner_only_runtime_file(&paths.containment_record_path())
      .unwrap();
  }

  #[test]
  fn runtime_guard_generation_lock_rejects_a_concurrent_owner() {
    let (_temp, paths) = fixture_paths();
    let _first = RuntimeGuardGenerationLock::try_acquire(&paths)
      .unwrap()
      .unwrap();

    assert!(
      RuntimeGuardGenerationLock::try_acquire(&paths)
        .unwrap()
        .is_none()
    );
  }

  #[test]
  fn runtime_guard_replacement_allows_absent_record_only_without_stale_metadata() {
    let (_temp, paths) = fixture_paths();
    let lock = RuntimeGuardGenerationLock::try_acquire(&paths)
      .unwrap()
      .unwrap();
    let expected = RuntimeGuardRecord::ready(context(&paths), guard_identity(), None)
      .replacement_binding()
      .unwrap();

    assert_eq!(
      lock.prove_replacement(None).unwrap(),
      RuntimeGuardReplacementProof::FirstStart
    );
    let error = lock.prove_replacement(Some(&expected)).unwrap_err();
    assert!(error.to_string().contains("proof is missing"));
  }

  #[test]
  fn runtime_guard_replacement_requires_exact_terminal_empty_tree_binding() {
    let (_temp, paths) = fixture_paths();
    let lock = RuntimeGuardGenerationLock::try_acquire(&paths)
      .unwrap()
      .unwrap();
    let record = terminal_record(&paths);
    let expected = record.replacement_binding().unwrap();
    lock.publish(&record).unwrap();

    let proof = lock.prove_replacement(Some(&expected)).unwrap();

    assert!(matches!(
      proof,
      RuntimeGuardReplacementProof::PreviousGenerationTerminated { .. }
    ));
  }

  #[test]
  fn runtime_guard_replacement_fails_closed_for_mismatch_and_nonterminal_state() {
    let (_temp, paths) = fixture_paths();
    let lock = RuntimeGuardGenerationLock::try_acquire(&paths)
      .unwrap()
      .unwrap();
    let terminal = terminal_record(&paths);
    let mut mismatched = terminal.replacement_binding().unwrap();
    mismatched.child.as_mut().unwrap().pinned_caddy.image.sha256 = "bb".repeat(32);
    lock.publish(&terminal).unwrap();

    let mismatch_error = lock.prove_replacement(Some(&mismatched)).unwrap_err();
    assert!(mismatch_error.to_string().contains("does not match"));

    let ready = RuntimeGuardRecord::ready(
      terminal.context.clone(),
      terminal.guard.clone().unwrap(),
      terminal.child.clone(),
    );
    lock.publish(&ready).unwrap();
    let nonterminal_error = lock.prove_replacement(Some(&mismatched)).unwrap_err();
    assert!(
      nonterminal_error
        .to_string()
        .contains("has not published a terminal empty-tree proof")
    );
  }

  #[test]
  fn runtime_guard_replacement_fails_closed_for_unreadable_record() {
    let (_temp, paths) = fixture_paths();
    let lock = RuntimeGuardGenerationLock::try_acquire(&paths)
      .unwrap()
      .unwrap();
    let expected = terminal_record(&paths).replacement_binding().unwrap();
    let mut file =
      create_owner_only_runtime_file(&paths, &paths.containment_record_path()).unwrap();
    file.write_all(b"{not-json").unwrap();
    flush_file(&file).unwrap();

    let error = lock.prove_replacement(Some(&expected)).unwrap_err();

    assert!(error.to_string().contains("unreadable or untrusted"));
  }

  fn fixture_paths() -> (tempfile::TempDir, RuntimePaths) {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    (temp, paths)
  }

  fn context(paths: &RuntimePaths) -> RuntimeGuardGenerationContext {
    RuntimeGuardGenerationContext {
      profile: paths.runtime_profile().to_string(),
      runtime_id: paths.instance_key().to_string(),
      daemon_instance_id: "00112233445566778899aabbccddeeff".to_string(),
      owner_generation: "ffeeddccbbaa99887766554433221100".to_string(),
      nonce_commitment: "11".repeat(32),
    }
  }

  fn guard_identity() -> RuntimeGuardIdentity {
    RuntimeGuardIdentity {
      process: RuntimeGuardProcessIdentity {
        process_id: std::process::id(),
        creation_identity: "windows-creation-time-123".to_string(),
      },
      image: RuntimeGuardImageIdentity {
        path: std::env::current_exe().unwrap(),
        file_identity: "volume-1-file-2".to_string(),
        sha256: "22".repeat(32),
      },
    }
  }

  fn child_identity() -> RuntimeGuardChildIdentity {
    RuntimeGuardChildIdentity {
      child_generation: "1234567890abcdef1234567890abcdef".to_string(),
      process: RuntimeGuardProcessIdentity {
        process_id: 42,
        creation_identity: "windows-creation-time-456".to_string(),
      },
      pinned_caddy: RuntimeGuardPinnedCaddyIdentity {
        image: RuntimeGuardImageIdentity {
          path: std::env::current_exe().unwrap(),
          file_identity: "volume-1-file-3".to_string(),
          sha256: "33".repeat(32),
        },
        version: "2.11.3".to_string(),
        probe_revision: "cadder-v1-probe-1".to_string(),
      },
    }
  }

  fn terminal_record(paths: &RuntimePaths) -> RuntimeGuardRecord {
    RuntimeGuardRecord::terminal(
      context(paths),
      guard_identity(),
      Some(child_identity()),
      RuntimeGuardTerminalOutcome {
        reason: RuntimeGuardTerminalReason::OwnerChannelClosed,
        child_exit_code: Some(0),
        completed_at_utc: Utc::now(),
      },
    )
  }
}
