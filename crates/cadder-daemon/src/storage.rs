use crate::paths::StoragePaths;
use anyhow::{Context, Result, anyhow, bail};
use cadder_ipc::{HistoryKind, HistoryRecord, RuntimeDiagnostic, StorageState};
use chrono::Utc;
use fs4::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
  collections::VecDeque,
  fmt,
  fs::{self, File},
  io::{self, Read, Seek, SeekFrom, Write},
  panic::{AssertUnwindSafe, catch_unwind},
  path::{Path, PathBuf},
  sync::{Arc, Mutex},
  thread,
};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::time::{Instant, timeout_at};

const SCHEMA_VERSION: u32 = 1;
const HISTORY_RETENTION_LIMIT: usize = 100_000;
const STORAGE_QUEUE_CAPACITY: usize = 256;
const STORAGE_QUEUE_BYTE_CAPACITY: usize = 8 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 32 * 1024;
const RECORD_ENVELOPE_RESERVE_BYTES: usize = 1024;
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_SEGMENT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SEGMENT_RECORDS: usize = 10_000;
const MAX_TRANSACTION_SEGMENTS: usize = 16;
const MAX_RECOVERY_COPY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_RECOVERY_ENTRIES: usize = 1024;
const MAX_STORAGE_DIAGNOSTICS: usize = 32;
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone)]
pub struct RuntimeStore {
  sender: Option<mpsc::UnboundedSender<StorageCommand>>,
  shared: Arc<StoreShared>,
}

struct StoreShared {
  admission: Arc<Mutex<StorageAdmission>>,
  lifecycle: Arc<WorkerLifecycle>,
  sidecar: Arc<Mutex<StorageSidecar>>,
  worker: Mutex<Option<thread::JoinHandle<()>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoragePhase {
  Accepting,
  Draining,
}

struct StorageAdmission {
  phase: StoragePhase,
  queued_commands: usize,
  queued_bytes: usize,
  command_capacity: usize,
  byte_capacity: usize,
}

#[derive(Default)]
struct WorkerLifecycle {
  outcome: Mutex<Option<std::result::Result<(), String>>>,
  finished: Notify,
}

struct StorageSidecar {
  backend: String,
  path: Option<PathBuf>,
  diagnostics: Vec<RuntimeDiagnostic>,
}

#[derive(Serialize)]
struct HistoryWrite {
  kind: HistoryKind,
  summary: String,
  registration_id: Option<String>,
  domain_key: Option<String>,
  payload: Value,
  retention_limit: usize,
}

enum StorageCommand {
  Record {
    write: HistoryWrite,
    admitted_bytes: usize,
    reply: Option<oneshot::Sender<std::result::Result<(), String>>>,
  },
  Query {
    kind: Option<HistoryKind>,
    limit: usize,
    reply: oneshot::Sender<Vec<HistoryRecord>>,
  },
  Shutdown,
}

enum StorageBackend {
  Memory(MemoryHistoryStore),
  Files(FileHistoryStore),
  #[cfg(test)]
  FailFlush(MemoryHistoryStore),
  #[cfg(test)]
  PanicOnFlush(MemoryHistoryStore),
}

#[derive(Default)]
struct MemoryHistoryStore {
  records: VecDeque<HistoryRecord>,
  next_sequence: i64,
}

struct FileHistoryStore {
  _lock: File,
  paths: StoragePaths,
  generation: String,
  segment: File,
  segment_path: PathBuf,
  records: VecDeque<HistoryRecord>,
  next_sequence: i64,
  previous_hash: String,
  recovery_notes: Vec<String>,
  segment_byte_limit: u64,
  segment_record_limit: usize,
  segment_records: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StorageManifest {
  schema_version: u32,
  active_generation: String,
  active_transaction_segment: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredHistoryBody {
  schema_version: u32,
  sequence_number: i64,
  timestamp_utc: chrono::DateTime<Utc>,
  kind: HistoryKind,
  summary: String,
  registration_id: Option<String>,
  domain_key: Option<String>,
  payload: Value,
  previous_record_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredHistoryRecord {
  #[serde(flatten)]
  body: StoredHistoryBody,
  checksum: String,
}

#[derive(Debug)]
enum AdmissionError {
  Draining,
  QueueFull,
  RecordTooLarge,
  WorkerUnavailable,
}

impl fmt::Debug for RuntimeStore {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("RuntimeStore")
      .field("state", &self.state())
      .finish()
  }
}

impl RuntimeStore {
  pub(crate) fn try_open(paths: &StoragePaths) -> Result<Self> {
    let backend = FileHistoryStore::open(paths)?;
    let recovery_notes = backend.recovery_notes.clone();
    let store = Self::spawn(
      StorageBackend::Files(backend),
      "files",
      Some(paths.profile_dir().to_path_buf()),
      STORAGE_QUEUE_CAPACITY,
      STORAGE_QUEUE_BYTE_CAPACITY,
      None,
    )?;
    for note in recovery_notes {
      push_diagnostic(
        &store.shared.sidecar,
        "storage-incomplete-tail-recovered",
        note,
      );
    }
    Ok(store)
  }

  pub fn open(paths: &StoragePaths) -> Self {
    match Self::try_open(paths) {
      Ok(store) => store,
      Err(error) => Self::unavailable(
        Some(paths.profile_dir().to_path_buf()),
        format!("Could not open runtime storage: {error:#}"),
      ),
    }
  }

  pub fn memory() -> Self {
    Self::spawn(
      StorageBackend::Memory(MemoryHistoryStore::default()),
      "memory",
      None,
      STORAGE_QUEUE_CAPACITY,
      STORAGE_QUEUE_BYTE_CAPACITY,
      None,
    )
    .expect("start in-memory runtime storage worker")
  }

  pub fn state(&self) -> StorageState {
    let sidecar = self
      .shared
      .sidecar
      .lock()
      .expect("runtime storage sidecar mutex poisoned");
    StorageState {
      backend: sidecar.backend.clone(),
      path: sidecar.path.as_ref().map(|path| path.display().to_string()),
      schema_version: SCHEMA_VERSION,
      diagnostics: sidecar.diagnostics.clone(),
    }
  }

  pub fn record_history<T: Serialize>(
    &self,
    kind: HistoryKind,
    summary: impl Into<String>,
    registration_id: Option<&str>,
    domain_key: Option<&str>,
    payload: &T,
  ) {
    self.record_history_with_retention(
      kind,
      summary,
      registration_id,
      domain_key,
      payload,
      HISTORY_RETENTION_LIMIT,
    );
  }

  fn record_history_with_retention<T: Serialize>(
    &self,
    kind: HistoryKind,
    summary: impl Into<String>,
    registration_id: Option<&str>,
    domain_key: Option<&str>,
    payload: &T,
    retention_limit: usize,
  ) {
    let (write, admitted_bytes) = match history_write(
      kind,
      summary,
      registration_id,
      domain_key,
      payload,
      retention_limit,
    ) {
      Ok(write) => write,
      Err(error) => {
        push_diagnostic(
          &self.shared.sidecar,
          "storage-history-serialization",
          format!("Could not serialize runtime history: {error:#}"),
        );
        return;
      }
    };
    let command = StorageCommand::Record {
      write,
      admitted_bytes,
      reply: None,
    };
    if let Err(error) = self.admit(command, admitted_bytes) {
      self.report_admission_error(error, "write runtime history");
    }
  }

  pub(crate) async fn commit_history<T: Serialize>(
    &self,
    kind: HistoryKind,
    summary: impl Into<String>,
    registration_id: Option<&str>,
    domain_key: Option<&str>,
    payload: &T,
  ) -> Result<()> {
    let (write, admitted_bytes) = history_write(
      kind,
      summary,
      registration_id,
      domain_key,
      payload,
      HISTORY_RETENTION_LIMIT,
    )?;
    let (reply, response) = oneshot::channel();
    self
      .admit(
        StorageCommand::Record {
          write,
          admitted_bytes,
          reply: Some(reply),
        },
        admitted_bytes,
      )
      .map_err(|error| anyhow!(admission_error_message(error, "commit runtime history").1))?;
    response
      .await
      .context("runtime storage worker closed before acknowledging history")?
      .map_err(anyhow::Error::msg)
  }

  pub(crate) fn enqueue_history_for_shutdown<T: Serialize>(
    &self,
    kind: HistoryKind,
    summary: impl Into<String>,
    payload: &T,
  ) -> Result<()> {
    let (write, admitted_bytes) =
      history_write(kind, summary, None, None, payload, HISTORY_RETENTION_LIMIT)?;
    self
      .admit(
        StorageCommand::Record {
          write,
          admitted_bytes,
          reply: None,
        },
        admitted_bytes,
      )
      .map_err(|error| anyhow!(admission_error_message(error, "queue shutdown history").1))
  }

  pub async fn query_history(&self, kind: Option<HistoryKind>, limit: usize) -> Vec<HistoryRecord> {
    let (reply, response) = oneshot::channel();
    let command = StorageCommand::Query {
      kind,
      limit: limit.clamp(1, 500),
      reply,
    };
    if let Err(error) = self.admit(command, 0) {
      self.report_admission_error(error, "read runtime history");
      return Vec::new();
    }
    match response.await {
      Ok(records) => records,
      Err(_) => {
        push_diagnostic(
          &self.shared.sidecar,
          "storage-history-worker-unavailable",
          "Runtime history worker closed without returning history.".to_string(),
        );
        Vec::new()
      }
    }
  }

  /// Closes admission, flushes all accepted commands, and joins the worker before the deadline.
  pub(crate) async fn shutdown_until(&self, deadline: Instant) -> Result<bool> {
    self.begin_shutdown();
    if timeout_at(deadline, self.wait_for_worker()).await.is_err() {
      push_diagnostic(
        &self.shared.sidecar,
        "storage-shutdown-contained",
        "Runtime storage exceeded its shutdown budget; Cadder retains ownership until the worker finishes."
          .to_string(),
      );
      return Ok(false);
    }
    self.join_finished_worker();
    self.worker_outcome()?;
    Ok(true)
  }

  /// Waits without detaching after the normal shutdown budget expires.
  pub(crate) async fn contain_shutdown(&self) -> Result<()> {
    self.begin_shutdown();
    self.wait_for_worker().await;
    self.join_finished_worker();
    self.worker_outcome()
  }

  fn admit(
    &self,
    command: StorageCommand,
    admitted_bytes: usize,
  ) -> std::result::Result<(), AdmissionError> {
    let Some(sender) = &self.sender else {
      return Err(AdmissionError::WorkerUnavailable);
    };
    if admitted_bytes > MAX_RECORD_BYTES {
      return Err(AdmissionError::RecordTooLarge);
    }
    let mut admission = self
      .shared
      .admission
      .lock()
      .expect("runtime storage admission mutex poisoned");
    if admission.phase != StoragePhase::Accepting {
      return Err(AdmissionError::Draining);
    }
    if admission.queued_commands >= admission.command_capacity
      || admission.queued_bytes.saturating_add(admitted_bytes) > admission.byte_capacity
    {
      return Err(AdmissionError::QueueFull);
    }
    admission.queued_commands += 1;
    admission.queued_bytes += admitted_bytes;
    if sender.send(command).is_err() {
      admission.queued_commands -= 1;
      admission.queued_bytes -= admitted_bytes;
      return Err(AdmissionError::WorkerUnavailable);
    }
    Ok(())
  }

  fn begin_shutdown(&self) {
    let Some(sender) = &self.sender else {
      return;
    };
    let mut admission = self
      .shared
      .admission
      .lock()
      .expect("runtime storage admission mutex poisoned");
    if admission.phase == StoragePhase::Draining {
      return;
    }
    admission.phase = StoragePhase::Draining;
    if sender.send(StorageCommand::Shutdown).is_err() {
      drop(admission);
      push_diagnostic(
        &self.shared.sidecar,
        "storage-history-worker-unavailable",
        "Runtime storage worker closed before accepting the shutdown barrier.".to_string(),
      );
    }
  }

  async fn wait_for_worker(&self) {
    loop {
      let notified = self.shared.lifecycle.finished.notified();
      if self
        .shared
        .lifecycle
        .outcome
        .lock()
        .expect("runtime storage lifecycle mutex poisoned")
        .is_some()
      {
        return;
      }
      notified.await;
    }
  }

  fn join_finished_worker(&self) {
    let worker = self
      .shared
      .worker
      .lock()
      .expect("runtime storage worker mutex poisoned")
      .take();
    if let Some(worker) = worker
      && worker.join().is_err()
    {
      push_diagnostic(
        &self.shared.sidecar,
        "storage-history-worker-panicked",
        "Runtime storage worker panicked while shutting down.".to_string(),
      );
    }
  }

  fn worker_outcome(&self) -> Result<()> {
    match self
      .shared
      .lifecycle
      .outcome
      .lock()
      .expect("runtime storage lifecycle mutex poisoned")
      .as_ref()
    {
      Some(Ok(())) => Ok(()),
      Some(Err(error)) => Err(anyhow!(error.clone())),
      None => bail!("runtime storage worker has not finished"),
    }
  }

  fn report_admission_error(&self, error: AdmissionError, operation: &str) {
    let (code, message) = admission_error_message(error, operation);
    push_diagnostic(&self.shared.sidecar, code, message);
  }

  fn spawn(
    backend: StorageBackend,
    backend_name: &str,
    path: Option<PathBuf>,
    command_capacity: usize,
    byte_capacity: usize,
    start_gate: Option<std::sync::mpsc::Receiver<()>>,
  ) -> Result<Self> {
    let sidecar = Arc::new(Mutex::new(StorageSidecar {
      backend: backend_name.to_string(),
      path: path.clone(),
      diagnostics: Vec::new(),
    }));
    let admission = Mutex::new(StorageAdmission {
      phase: StoragePhase::Accepting,
      queued_commands: 0,
      queued_bytes: 0,
      command_capacity,
      byte_capacity,
    });
    let admission = Arc::new(admission);
    let lifecycle = Arc::new(WorkerLifecycle::default());
    let (sender, receiver) = mpsc::unbounded_channel();
    let worker = match spawn_storage_worker(
      backend,
      receiver,
      admission.clone(),
      sidecar.clone(),
      lifecycle.clone(),
      start_gate,
    ) {
      Ok(worker) => worker,
      Err(error) => {
        return Err(error);
      }
    };
    let shared = Arc::new(StoreShared {
      admission,
      lifecycle,
      sidecar,
      worker: Mutex::new(Some(worker)),
    });

    Ok(Self {
      sender: Some(sender),
      shared,
    })
  }

  fn unavailable(path: Option<PathBuf>, message: String) -> Self {
    let lifecycle = Arc::new(WorkerLifecycle::default());
    *lifecycle
      .outcome
      .lock()
      .expect("runtime storage lifecycle mutex poisoned") = Some(Ok(()));
    Self {
      sender: None,
      shared: Arc::new(StoreShared {
        admission: Arc::new(Mutex::new(StorageAdmission {
          phase: StoragePhase::Draining,
          queued_commands: 0,
          queued_bytes: 0,
          command_capacity: 0,
          byte_capacity: 0,
        })),
        lifecycle,
        sidecar: Arc::new(Mutex::new(StorageSidecar {
          backend: "unavailable".to_string(),
          path,
          diagnostics: vec![RuntimeDiagnostic {
            code: "storage-unavailable".to_string(),
            message,
            operation: Some("open-storage".to_string()),
          }],
        })),
        worker: Mutex::new(None),
      }),
    }
  }

  #[cfg(test)]
  pub(crate) fn memory_stalled_for_test(
    queue_capacity: usize,
  ) -> (Self, std::sync::mpsc::Sender<()>) {
    let (release, gate) = std::sync::mpsc::channel();
    let store = Self::spawn(
      StorageBackend::Memory(MemoryHistoryStore::default()),
      "memory",
      None,
      queue_capacity,
      STORAGE_QUEUE_BYTE_CAPACITY,
      Some(gate),
    )
    .unwrap();
    (store, release)
  }

  #[cfg(test)]
  fn failing_flush_for_test(panic: bool) -> Self {
    let memory = MemoryHistoryStore::default();
    let backend = if panic {
      StorageBackend::PanicOnFlush(memory)
    } else {
      StorageBackend::FailFlush(memory)
    };
    Self::spawn(
      backend,
      "memory",
      None,
      STORAGE_QUEUE_CAPACITY,
      STORAGE_QUEUE_BYTE_CAPACITY,
      None,
    )
    .unwrap()
  }
}

fn history_write<T: Serialize>(
  kind: HistoryKind,
  summary: impl Into<String>,
  registration_id: Option<&str>,
  domain_key: Option<&str>,
  payload: &T,
  retention_limit: usize,
) -> Result<(HistoryWrite, usize)> {
  let payload = serde_json::to_value(payload).context("serialize runtime history payload")?;
  let write = HistoryWrite {
    kind,
    summary: summary.into(),
    registration_id: registration_id.map(str::to_string),
    domain_key: domain_key.map(str::to_string),
    payload,
    retention_limit,
  };
  let admitted_bytes = serde_json::to_vec(&write).map_or(MAX_RECORD_BYTES + 1, |bytes| {
    bytes.len().saturating_add(RECORD_ENVELOPE_RESERVE_BYTES)
  });
  Ok((write, admitted_bytes))
}

fn admission_error_message(error: AdmissionError, operation: &str) -> (&'static str, String) {
  match error {
    AdmissionError::Draining => (
      "storage-shutting-down",
      format!("Could not {operation}: runtime storage is shutting down."),
    ),
    AdmissionError::QueueFull => (
      if operation.starts_with("read") {
        "storage-history-query-queue-full"
      } else {
        "storage-history-queue-full"
      },
      format!("Could not {operation}: the bounded runtime storage queue is full."),
    ),
    AdmissionError::RecordTooLarge => (
      "storage-history-record-too-large",
      format!("Could not {operation}: the history record exceeds the 32 KiB storage limit."),
    ),
    AdmissionError::WorkerUnavailable => (
      "storage-history-worker-unavailable",
      format!("Could not {operation}: the runtime storage worker is unavailable."),
    ),
  }
}

fn spawn_storage_worker(
  mut backend: StorageBackend,
  mut receiver: mpsc::UnboundedReceiver<StorageCommand>,
  admission: Arc<Mutex<StorageAdmission>>,
  sidecar: Arc<Mutex<StorageSidecar>>,
  lifecycle: Arc<WorkerLifecycle>,
  start_gate: Option<std::sync::mpsc::Receiver<()>>,
) -> Result<thread::JoinHandle<()>> {
  thread::Builder::new()
    .name("cadder-runtime-storage".to_string())
    .spawn(move || {
      if let Some(gate) = start_gate {
        let _ = gate.recv();
      }
      let outcome = catch_unwind(AssertUnwindSafe(|| {
        run_storage_worker(&mut backend, &mut receiver, &admission, &sidecar)
      }))
      .unwrap_or_else(|payload| {
        Err(anyhow!(
          "runtime storage worker panicked: {}",
          panic_message(payload.as_ref())
        ))
      });
      if let Err(error) = &outcome {
        sidecar
          .lock()
          .expect("runtime storage sidecar mutex poisoned")
          .backend = "degraded".to_string();
        admission
          .lock()
          .expect("runtime storage admission mutex poisoned")
          .phase = StoragePhase::Draining;
        push_diagnostic(
          &sidecar,
          "storage-worker-failed",
          format!("Runtime storage worker stopped after an error: {error:#}"),
        );
      }
      *lifecycle
        .outcome
        .lock()
        .expect("runtime storage lifecycle mutex poisoned") =
        Some(outcome.map_err(|error| format!("{error:#}")));
      lifecycle.finished.notify_waiters();
    })
    .context("start runtime storage worker")
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
  payload
    .downcast_ref::<&'static str>()
    .copied()
    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
    .unwrap_or("non-string panic payload")
}

fn run_storage_worker(
  backend: &mut StorageBackend,
  receiver: &mut mpsc::UnboundedReceiver<StorageCommand>,
  admission: &Arc<Mutex<StorageAdmission>>,
  sidecar: &Arc<Mutex<StorageSidecar>>,
) -> Result<()> {
  while let Some(command) = receiver.blocking_recv() {
    match command {
      StorageCommand::Record {
        write,
        admitted_bytes,
        reply,
      } => {
        if let Err(error) = backend.record(write) {
          release_admission(admission, admitted_bytes);
          let message = format!("{error:#}");
          if let Some(reply) = reply {
            let _ = reply.send(Err(message.clone()));
          }
          push_diagnostic(
            sidecar,
            "storage-history-write",
            format!("Could not persist runtime history: {message}"),
          );
          return Err(error);
        }
        if let Some(reply) = reply {
          let _ = reply.send(Ok(()));
        }
        release_admission(admission, admitted_bytes);
      }
      StorageCommand::Query { kind, limit, reply } => {
        let _ = reply.send(backend.query(kind, limit));
        release_admission(admission, 0);
      }
      StorageCommand::Shutdown => {
        backend.flush()?;
        return Ok(());
      }
    }
  }
  backend.flush()
}

fn release_admission(admission: &Arc<Mutex<StorageAdmission>>, admitted_bytes: usize) {
  let mut admission = admission
    .lock()
    .expect("runtime storage admission mutex poisoned");
  admission.queued_commands = admission.queued_commands.saturating_sub(1);
  admission.queued_bytes = admission.queued_bytes.saturating_sub(admitted_bytes);
}

impl StorageBackend {
  fn record(&mut self, write: HistoryWrite) -> Result<()> {
    match self {
      Self::Memory(store) => store.record(write),
      Self::Files(store) => store.record(write),
      #[cfg(test)]
      Self::FailFlush(store) | Self::PanicOnFlush(store) => store.record(write),
    }
  }

  fn query(&self, kind: Option<HistoryKind>, limit: usize) -> Vec<HistoryRecord> {
    let records = match self {
      Self::Memory(store) => &store.records,
      Self::Files(store) => &store.records,
      #[cfg(test)]
      Self::FailFlush(store) | Self::PanicOnFlush(store) => &store.records,
    };
    records
      .iter()
      .rev()
      .filter(|record| kind.is_none_or(|kind| record.kind == kind))
      .take(limit)
      .cloned()
      .collect()
  }

  fn flush(&mut self) -> Result<()> {
    match self {
      Self::Memory(_) => Ok(()),
      Self::Files(store) => durable_sync(&store.segment)
        .with_context(|| format!("flush history segment {}", store.segment_path.display())),
      #[cfg(test)]
      Self::FailFlush(_) => bail!("injected storage flush failure"),
      #[cfg(test)]
      Self::PanicOnFlush(_) => panic!("injected storage flush panic"),
    }
  }
}

impl MemoryHistoryStore {
  fn record(&mut self, write: HistoryWrite) -> Result<()> {
    self.next_sequence += 1;
    self.records.push_back(HistoryRecord {
      sequence_number: self.next_sequence,
      timestamp_utc: Utc::now(),
      kind: write.kind,
      summary: write.summary,
      registration_id: write.registration_id,
      domain_key: write.domain_key,
      payload: write.payload,
    });
    prune_records(&mut self.records, write.retention_limit);
    Ok(())
  }
}

impl FileHistoryStore {
  fn open(paths: &StoragePaths) -> Result<Self> {
    secure_storage_directories(paths)?;
    let lock = open_or_create_owner_only_file(&paths.lock_path())?;
    FileExt::try_lock(&lock)
      .map_err(|error| anyhow!("runtime storage is already owned: {error}"))?;

    let manifest = load_or_create_manifest(paths)?;
    validate_generation(&manifest.active_generation)?;
    let transaction_dir = paths
      .generations_dir()
      .join(&manifest.active_generation)
      .join("transactions");
    secure_owner_only_directory(&transaction_dir)?;
    let segment_names = transaction_segment_names(
      &transaction_dir,
      &manifest.active_generation,
      &manifest.active_transaction_segment,
    )?;
    let mut records = VecDeque::new();
    let mut next_sequence = 1_i64;
    let mut previous_hash = GENESIS_HASH.to_string();
    let mut recovery_notes = Vec::new();
    let mut active = None;
    for name in segment_names {
      let segment_path = transaction_dir.join(&name);
      let mut segment = open_owner_only_file(&segment_path)?;
      let is_active = name == manifest.active_transaction_segment;
      let replayed = replay_segment(
        &mut segment,
        &segment_path,
        &paths.recovery_dir(),
        next_sequence,
        &previous_hash,
        is_active,
      );
      let (segment_records, following_sequence, following_hash, recovery_note) = match replayed {
        Ok(replayed) => replayed,
        Err(error) => {
          let backup = preserve_corrupt_generation(paths, &manifest.active_generation)
            .context("preserve corrupt runtime storage generation")?;
          return Err(error.context(format!(
            "runtime storage generation is corrupt; evidence was preserved as {}",
            backup.file_name().unwrap_or_default().to_string_lossy()
          )));
        }
      };
      let segment_record_count = segment_records.len();
      records.extend(segment_records);
      prune_records(&mut records, HISTORY_RETENTION_LIMIT);
      next_sequence = following_sequence;
      previous_hash = following_hash;
      if let Some(note) = recovery_note {
        recovery_notes.push(note);
      }
      if is_active {
        active = Some((segment, segment_path, segment_record_count));
        break;
      }
    }
    let (mut segment, segment_path, segment_records) = active
      .ok_or_else(|| anyhow!("runtime storage manifest selects a missing transaction segment"))?;
    segment.seek(SeekFrom::End(0))?;

    Ok(Self {
      _lock: lock,
      paths: paths.clone(),
      generation: manifest.active_generation,
      segment,
      segment_path,
      records,
      next_sequence,
      previous_hash,
      recovery_notes,
      segment_byte_limit: MAX_SEGMENT_BYTES,
      segment_record_limit: MAX_SEGMENT_RECORDS,
      segment_records,
    })
  }

  fn record(&mut self, write: HistoryWrite) -> Result<()> {
    let body = StoredHistoryBody {
      schema_version: SCHEMA_VERSION,
      sequence_number: self.next_sequence,
      timestamp_utc: Utc::now(),
      kind: write.kind,
      summary: write.summary,
      registration_id: write.registration_id,
      domain_key: write.domain_key,
      payload: write.payload,
      previous_record_hash: self.previous_hash.clone(),
    };
    let checksum = checksum_body(&body)?;
    let stored = StoredHistoryRecord { body, checksum };
    let mut bytes = serde_json::to_vec(&stored).context("serialize runtime history record")?;
    if bytes.len() + 1 > MAX_RECORD_BYTES {
      bail!("runtime history record exceeds the 32 KiB storage limit");
    }
    let record_hash = hash_bytes(&bytes);
    bytes.push(b'\n');
    let current_len = self.segment.metadata()?.len();
    if current_len.saturating_add(bytes.len() as u64) > self.segment_byte_limit
      || self.segment_records >= self.segment_record_limit
    {
      self.rotate()?;
    }
    self
      .segment
      .write_all(&bytes)
      .with_context(|| format!("append history segment {}", self.segment_path.display()))?;
    durable_sync(&self.segment)
      .with_context(|| format!("flush history segment {}", self.segment_path.display()))?;

    let record = history_from_stored(&stored);
    self.records.push_back(record);
    prune_records(&mut self.records, write.retention_limit);
    self.next_sequence += 1;
    self.previous_hash = record_hash;
    self.segment_records += 1;
    Ok(())
  }

  fn rotate(&mut self) -> Result<()> {
    durable_sync(&self.segment)?;
    let transaction_dir = self
      .paths
      .generations_dir()
      .join(&self.generation)
      .join("transactions");
    if fs::read_dir(&transaction_dir)?.count() >= MAX_TRANSACTION_SEGMENTS {
      bail!(
        "runtime history reached its bounded segment capacity; run `cadder logs export`, preserve the profile data, and restart after storage maintenance"
      );
    }
    let name = format!("{:020}-{}.jsonl", self.next_sequence, self.generation);
    let path = transaction_dir.join(&name);
    let candidate = create_owner_only_file(&path)
      .with_context(|| format!("create history segment {}", path.display()))?;
    durable_sync(&candidate)?;
    sync_parent_directory(&path)?;
    let manifest = StorageManifest {
      schema_version: SCHEMA_VERSION,
      active_generation: self.generation.clone(),
      active_transaction_segment: name,
    };
    if let Err(error) = publish_manifest(&self.paths.manifest_path(), &manifest) {
      drop(candidate);
      return Err(error);
    }
    self.segment = candidate;
    self.segment_path = path;
    self.segment_records = 0;
    Ok(())
  }
}

fn transaction_segment_names(
  directory: &Path,
  generation: &str,
  active_name: &str,
) -> Result<Vec<String>> {
  let suffix = format!("-{generation}.jsonl");
  let mut segments = Vec::new();
  for entry in fs::read_dir(directory)? {
    let entry = entry?;
    let file_type = entry.file_type()?;
    if !file_type.is_file() {
      bail!(
        "runtime storage transaction directory contains a non-file entry {}",
        entry.path().display()
      );
    }
    let name = entry
      .file_name()
      .into_string()
      .map_err(|_| anyhow!("runtime storage transaction filename is not UTF-8"))?;
    let Some(sequence) = name.strip_suffix(&suffix) else {
      bail!("runtime storage transaction filename `{name}` is invalid");
    };
    if sequence.len() != 20 || !sequence.bytes().all(|byte| byte.is_ascii_digit()) {
      bail!("runtime storage transaction filename `{name}` is invalid");
    }
    segments.push((sequence.parse::<i64>()?, name));
  }
  segments.sort_by_key(|(sequence, _)| *sequence);
  if segments.len() > MAX_TRANSACTION_SEGMENTS {
    bail!(
      "runtime storage contains more than the supported {MAX_TRANSACTION_SEGMENTS} transaction segments"
    );
  }
  let Some(active_index) = segments.iter().position(|(_, name)| name == active_name) else {
    bail!("runtime storage manifest selects a missing transaction segment");
  };
  if segments.windows(2).any(|window| window[0].0 == window[1].0) {
    bail!("runtime storage contains duplicate transaction segment sequences");
  }
  for (_, orphan) in &segments[active_index + 1..] {
    let path = directory.join(orphan);
    let file = open_owner_only_file(&path)?;
    if file.metadata()?.len() != 0 {
      bail!("unselected transaction segment `{orphan}` is not empty");
    }
    drop(file);
    fs::remove_file(&path)?;
    sync_parent_directory(&path)?;
  }
  Ok(
    segments
      .into_iter()
      .take(active_index + 1)
      .map(|(_, name)| name)
      .collect(),
  )
}

fn load_or_create_manifest(paths: &StoragePaths) -> Result<StorageManifest> {
  let path = paths.manifest_path();
  if path.exists() {
    let mut file = open_owner_only_file(&path)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
      .take(MAX_MANIFEST_BYTES as u64 + 1)
      .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
      bail!("runtime storage manifest exceeds {MAX_MANIFEST_BYTES} bytes");
    }
    let manifest: StorageManifest =
      serde_json::from_slice(&bytes).context("parse runtime storage manifest")?;
    if manifest.schema_version > SCHEMA_VERSION {
      bail!(
        "runtime storage schema version {} is newer than supported version {SCHEMA_VERSION}",
        manifest.schema_version
      );
    }
    if manifest.schema_version != SCHEMA_VERSION {
      bail!("runtime storage manifest requires a migration");
    }
    return Ok(manifest);
  }

  if fs::read_dir(paths.generations_dir())?.next().is_some() {
    bail!(
      "runtime storage manifest is missing while stored generations still exist; preserve the profile data and restore a verified manifest before restarting"
    );
  }

  let generation = new_generation()?;
  let generation_dir = paths.generations_dir().join(&generation);
  let transaction_dir = generation_dir.join("transactions");
  let logs_dir = generation_dir.join("logs");
  secure_owner_only_directory(&generation_dir)?;
  secure_owner_only_directory(&transaction_dir)?;
  secure_owner_only_directory(&logs_dir)?;
  let segment = format!("00000000000000000001-{generation}.jsonl");
  let manifest = StorageManifest {
    schema_version: SCHEMA_VERSION,
    active_generation: generation,
    active_transaction_segment: segment,
  };
  let segment_path = transaction_dir.join(&manifest.active_transaction_segment);
  let segment_file = create_owner_only_file(&segment_path)?;
  durable_sync(&segment_file)?;
  drop(segment_file);
  sync_parent_directory(&segment_path)?;
  publish_manifest(&path, &manifest)?;
  Ok(manifest)
}

fn publish_manifest(path: &Path, manifest: &StorageManifest) -> Result<()> {
  let parent = path
    .parent()
    .ok_or_else(|| anyhow!("runtime storage manifest has no parent directory"))?;
  let candidate = parent.join(format!(".manifest-{}.tmp", new_generation()?));
  let result = (|| -> Result<()> {
    let mut file = create_owner_only_file(&candidate)?;
    serde_json::to_writer(&mut file, manifest).context("serialize runtime storage manifest")?;
    file.write_all(b"\n")?;
    durable_sync(&file)?;
    drop(file);
    install_owner_only_file(&candidate, path)?;
    sync_parent_directory(path)
  })();
  if result.is_err() {
    let _ = fs::remove_file(&candidate);
  }
  result
}

fn replay_segment(
  segment: &mut File,
  path: &Path,
  recovery_dir: &Path,
  mut expected_sequence: i64,
  initial_previous_hash: &str,
  allow_incomplete_tail: bool,
) -> Result<(VecDeque<HistoryRecord>, i64, String, Option<String>)> {
  segment.seek(SeekFrom::Start(0))?;
  let mut bytes = Vec::new();
  Read::by_ref(segment)
    .take(MAX_SEGMENT_BYTES + 1)
    .read_to_end(&mut bytes)?;
  if bytes.len() as u64 > MAX_SEGMENT_BYTES {
    bail!("active history segment exceeds the 8 MiB boundary");
  }
  let mut recovery_note = None;
  if !bytes.is_empty() && !bytes.ends_with(b"\n") {
    if !allow_incomplete_tail {
      bail!("sealed history segment has an incomplete final record");
    }
    let valid_len = bytes
      .iter()
      .rposition(|byte| *byte == b'\n')
      .map_or(0, |position| position + 1);
    let tail = &bytes[valid_len..];
    if tail.len() > MAX_RECORD_BYTES {
      bail!("incomplete history tail exceeds the 32 KiB record limit");
    }
    preserve_incomplete_tail(recovery_dir, tail)?;
    recovery_note = Some(
      "Cadder recovered an incomplete final history record and preserved it in the profile recovery directory."
        .to_string(),
    );
    segment.set_len(valid_len as u64)?;
    durable_sync(segment)?;
    bytes.truncate(valid_len);
    sync_parent_directory(path)?;
  }

  let mut records = VecDeque::new();
  let mut previous_hash = initial_previous_hash.to_string();
  let committed = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
  if committed.is_empty() {
    return Ok((records, expected_sequence, previous_hash, recovery_note));
  }
  for (index, line) in committed.split(|byte| *byte == b'\n').enumerate() {
    if line.is_empty() {
      bail!("complete history record {} is empty", index + 1);
    }
    if line.len() > MAX_RECORD_BYTES {
      bail!("history record {} exceeds the 32 KiB limit", index + 1);
    }
    let stored: StoredHistoryRecord = serde_json::from_slice(line)
      .with_context(|| format!("parse complete history record {}", index + 1))?;
    validate_stored_record(&stored, expected_sequence, &previous_hash)
      .with_context(|| format!("validate complete history record {}", index + 1))?;
    previous_hash = hash_bytes(line);
    records.push_back(history_from_stored(&stored));
    expected_sequence += 1;
  }
  Ok((records, expected_sequence, previous_hash, recovery_note))
}

fn preserve_incomplete_tail(recovery_dir: &Path, tail: &[u8]) -> Result<PathBuf> {
  secure_owner_only_directory(recovery_dir)?;
  let path = recovery_dir.join(format!("incomplete-tail-{}.jsonl.part", hash_bytes(tail)));
  match create_owner_only_file(&path) {
    Ok(mut file) => {
      file.write_all(tail)?;
      durable_sync(&file)?;
      sync_parent_directory(&path)?;
      Ok(path)
    }
    Err(error)
      if error
        .downcast_ref::<io::Error>()
        .is_some_and(|error| error.kind() == io::ErrorKind::AlreadyExists) =>
    {
      Ok(path)
    }
    Err(error) => Err(error),
  }
}

fn preserve_corrupt_generation(paths: &StoragePaths, generation: &str) -> Result<PathBuf> {
  let destination = paths
    .recovery_dir()
    .join(format!("corrupt-{generation}-{}", new_generation()?));
  secure_owner_only_directory(&destination)?;
  let mut budget = RecoveryCopyBudget {
    entries: 0,
    bytes: 0,
  };
  copy_recovery_tree(
    &paths.generations_dir().join(generation),
    &destination.join("generation"),
    &mut budget,
  )?;
  copy_recovery_file(
    &paths.manifest_path(),
    &destination.join("manifest.json"),
    &mut budget,
  )?;
  sync_parent_directory(&destination)?;
  Ok(destination)
}

struct RecoveryCopyBudget {
  entries: usize,
  bytes: u64,
}

fn copy_recovery_tree(
  source: &Path,
  destination: &Path,
  budget: &mut RecoveryCopyBudget,
) -> Result<()> {
  secure_owner_only_directory(destination)?;
  for entry in fs::read_dir(source)? {
    budget.entries += 1;
    if budget.entries > MAX_RECOVERY_ENTRIES {
      bail!("corrupt storage recovery exceeds the entry limit");
    }
    let entry = entry?;
    let file_type = entry.file_type()?;
    let target = destination.join(entry.file_name());
    if file_type.is_dir() {
      copy_recovery_tree(&entry.path(), &target, budget)?;
    } else if file_type.is_file() {
      copy_recovery_file(&entry.path(), &target, budget)?;
    } else {
      bail!("corrupt storage recovery contains a link or special file");
    }
  }
  sync_parent_directory(destination)
}

fn copy_recovery_file(
  source: &Path,
  destination: &Path,
  budget: &mut RecoveryCopyBudget,
) -> Result<()> {
  copy_recovery_file_with_limit(source, destination, budget, MAX_RECOVERY_COPY_BYTES)
}

fn copy_recovery_file_with_limit(
  source: &Path,
  destination: &Path,
  budget: &mut RecoveryCopyBudget,
  byte_limit: u64,
) -> Result<()> {
  let mut source = open_owner_only_file(source)?;
  let length = source.metadata()?.len();
  let remaining = byte_limit.saturating_sub(budget.bytes);
  if length > remaining {
    bail!("corrupt storage recovery exceeds the 128 MiB byte limit");
  }
  let mut destination_file = create_owner_only_file(destination)?;
  let copied = io::copy(
    &mut Read::by_ref(&mut source).take(remaining + 1),
    &mut destination_file,
  );
  let copied = match copied {
    Ok(copied) if copied <= remaining => copied,
    Ok(_) => {
      drop(destination_file);
      let _ = fs::remove_file(destination);
      bail!("corrupt storage recovery exceeds the 128 MiB byte limit");
    }
    Err(error) => {
      drop(destination_file);
      let _ = fs::remove_file(destination);
      return Err(error.into());
    }
  };
  if copied != length {
    drop(destination_file);
    let _ = fs::remove_file(destination);
    bail!("corrupt storage recovery copy length changed during backup");
  }
  budget.bytes += copied;
  durable_sync(&destination_file)?;
  sync_parent_directory(destination)
}

fn validate_stored_record(
  record: &StoredHistoryRecord,
  expected_sequence: i64,
  expected_previous_hash: &str,
) -> Result<()> {
  if record.body.schema_version > SCHEMA_VERSION {
    bail!(
      "history schema version {} is newer than supported version {SCHEMA_VERSION}",
      record.body.schema_version
    );
  }
  if record.body.schema_version != SCHEMA_VERSION {
    bail!("history record requires a schema migration");
  }
  if record.body.sequence_number != expected_sequence {
    bail!(
      "history sequence is {}, expected {expected_sequence}",
      record.body.sequence_number
    );
  }
  if record.body.previous_record_hash != expected_previous_hash {
    bail!("history hash chain does not match the previous record");
  }
  if record.checksum != checksum_body(&record.body)? {
    bail!("history record checksum does not match its content");
  }
  Ok(())
}

fn checksum_body(body: &StoredHistoryBody) -> Result<String> {
  Ok(hash_bytes(
    &serde_json::to_vec(body).context("serialize history checksum input")?,
  ))
}

fn hash_bytes(bytes: &[u8]) -> String {
  hex::encode(Sha256::digest(bytes))
}

fn history_from_stored(stored: &StoredHistoryRecord) -> HistoryRecord {
  HistoryRecord {
    sequence_number: stored.body.sequence_number,
    timestamp_utc: stored.body.timestamp_utc,
    kind: stored.body.kind,
    summary: stored.body.summary.clone(),
    registration_id: stored.body.registration_id.clone(),
    domain_key: stored.body.domain_key.clone(),
    payload: stored.body.payload.clone(),
  }
}

fn prune_records(records: &mut VecDeque<HistoryRecord>, retention_limit: usize) {
  while records.len() > retention_limit {
    records.pop_front();
  }
}

fn secure_storage_directories(paths: &StoragePaths) -> Result<()> {
  for path in [
    paths.profile_dir().to_path_buf(),
    paths.generations_dir(),
    paths.plans_dir(),
    paths.secrets_dir(),
    paths.recovery_dir(),
  ] {
    secure_owner_only_directory(&path)
      .with_context(|| format!("secure runtime storage directory {}", path.display()))?;
  }
  Ok(())
}

fn new_generation() -> Result<String> {
  let mut bytes = [0_u8; 16];
  getrandom::fill(&mut bytes).map_err(|error| anyhow!(error.to_string()))?;
  Ok(hex::encode(bytes))
}

fn validate_generation(generation: &str) -> Result<()> {
  if generation.len() == 32
    && generation
      .bytes()
      .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
  {
    Ok(())
  } else {
    bail!("runtime storage manifest contains an invalid generation identifier")
  }
}

fn open_or_create_owner_only_file(path: &Path) -> Result<File> {
  match create_owner_only_file(path) {
    Ok(file) => Ok(file),
    Err(error)
      if error
        .downcast_ref::<io::Error>()
        .is_some_and(|error| error.kind() == io::ErrorKind::AlreadyExists) =>
    {
      open_owner_only_file(path)
    }
    Err(error) => Err(error),
  }
}

#[cfg(unix)]
fn secure_owner_only_directory(path: &Path) -> Result<()> {
  use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
  let mut builder = fs::DirBuilder::new();
  builder.recursive(true).mode(0o700).create(path)?;
  crate::ipc_unix_security::secure_owner_only_directory(path)?;
  fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
  Ok(())
}

#[cfg(windows)]
fn secure_owner_only_directory(path: &Path) -> Result<()> {
  fs::create_dir_all(path)?;
  crate::ipc_windows_security::secure_owner_only_runtime_directory(path)?;
  Ok(())
}

#[cfg(not(any(unix, windows)))]
fn secure_owner_only_directory(path: &Path) -> Result<()> {
  fs::create_dir_all(path).map_err(Into::into)
}

#[cfg(unix)]
fn create_owner_only_file(path: &Path) -> Result<File> {
  use std::os::unix::fs::OpenOptionsExt;
  Ok(
    fs::OpenOptions::new()
      .read(true)
      .write(true)
      .create_new(true)
      .mode(0o600)
      .open(path)?,
  )
}

#[cfg(windows)]
fn create_owner_only_file(path: &Path) -> Result<File> {
  crate::ipc_windows_security::create_owner_only_runtime_file(path).map_err(Into::into)
}

#[cfg(not(any(unix, windows)))]
fn create_owner_only_file(path: &Path) -> Result<File> {
  Ok(
    fs::OpenOptions::new()
      .read(true)
      .write(true)
      .create_new(true)
      .open(path)?,
  )
}

#[cfg(unix)]
fn open_owner_only_file(path: &Path) -> Result<File> {
  crate::ipc_unix_security::open_owner_only_runtime_file(path).map_err(Into::into)
}

#[cfg(windows)]
fn open_owner_only_file(path: &Path) -> Result<File> {
  crate::ipc_windows_security::open_owner_only_runtime_file(path).map_err(Into::into)
}

#[cfg(not(any(unix, windows)))]
fn open_owner_only_file(path: &Path) -> Result<File> {
  Ok(fs::OpenOptions::new().read(true).write(true).open(path)?)
}

#[cfg(target_os = "macos")]
fn durable_sync(file: &File) -> Result<()> {
  use std::os::fd::AsRawFd;
  file.sync_all()?;
  // SAFETY: the descriptor is borrowed from a live file for the duration of fcntl.
  if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } == -1 {
    return Err(io::Error::last_os_error().into());
  }
  Ok(())
}

#[cfg(not(target_os = "macos"))]
fn durable_sync(file: &File) -> Result<()> {
  file.sync_all().map_err(Into::into)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<()> {
  crate::ipc_unix_security::sync_parent_directory(path).map_err(Into::into)
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<()> {
  Ok(())
}

#[cfg(unix)]
fn install_owner_only_file(candidate: &Path, destination: &Path) -> Result<()> {
  fs::rename(candidate, destination).map_err(Into::into)
}

#[cfg(windows)]
fn install_owner_only_file(candidate: &Path, destination: &Path) -> Result<()> {
  crate::ipc_windows_security::install_discovery_file(candidate, destination).map_err(Into::into)
}

#[cfg(not(any(unix, windows)))]
fn install_owner_only_file(candidate: &Path, destination: &Path) -> Result<()> {
  fs::rename(candidate, destination).map_err(Into::into)
}

fn push_diagnostic(sidecar: &Arc<Mutex<StorageSidecar>>, code: &str, message: String) {
  let mut sidecar = sidecar
    .lock()
    .expect("runtime storage sidecar mutex poisoned");
  if sidecar
    .diagnostics
    .last()
    .is_some_and(|diagnostic| diagnostic.code == code && diagnostic.message == message)
  {
    return;
  }
  sidecar.diagnostics.push(RuntimeDiagnostic {
    code: code.to_string(),
    message,
    operation: Some("runtime-storage".to_string()),
  });
  if sidecar.diagnostics.len() > MAX_STORAGE_DIAGNOSTICS {
    sidecar.diagnostics.remove(0);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::time::Duration;

  fn storage_paths(root: &Path) -> StoragePaths {
    crate::RuntimePaths::resolve(Some(root.to_path_buf()))
      .unwrap()
      .storage_paths()
      .clone()
  }

  fn active_segment_path(paths: &StoragePaths) -> PathBuf {
    let manifest: StorageManifest =
      serde_json::from_slice(&fs::read(paths.manifest_path()).unwrap()).unwrap();
    paths
      .generations_dir()
      .join(manifest.active_generation)
      .join("transactions")
      .join(manifest.active_transaction_segment)
  }

  #[tokio::test]
  async fn records_and_filters_history_preserving_worker_order() {
    let store = RuntimeStore::memory();
    store.record_history(
      HistoryKind::Registration,
      "Registered shim-1.",
      Some("shim-1"),
      Some("app.localhost"),
      &serde_json::json!({ "accepted": true }),
    );
    store.record_history(
      HistoryKind::Runtime,
      "Runtime started.",
      None,
      None,
      &serde_json::json!({ "status": "running" }),
    );

    let records = store
      .query_history(Some(HistoryKind::Registration), 10)
      .await;

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].summary, "Registered shim-1.");
    assert_eq!(records[0].registration_id.as_deref(), Some("shim-1"));
  }

  #[tokio::test]
  async fn file_store_replays_checksums_and_hash_chain_after_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let paths = storage_paths(dir.path());
    let store = RuntimeStore::open(&paths);
    assert_eq!(store.state().backend, "files");
    store.record_history(
      HistoryKind::Runtime,
      "Runtime started.",
      None,
      None,
      &serde_json::json!({ "status": "running" }),
    );
    assert!(
      store
        .shutdown_until(Instant::now() + Duration::from_secs(1))
        .await
        .unwrap()
    );

    let reopened = RuntimeStore::open(&paths);
    let records = reopened.query_history(None, 10).await;

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].summary, "Runtime started.");
    reopened.contain_shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn file_store_fails_closed_when_the_manifest_is_missing_from_existing_data() {
    let dir = tempfile::tempdir().unwrap();
    let paths = storage_paths(dir.path());
    let store = RuntimeStore::open(&paths);
    store.record_history(
      HistoryKind::Runtime,
      "Committed.",
      None,
      None,
      &serde_json::json!({}),
    );
    store.contain_shutdown().await.unwrap();
    let generation_entries = fs::read_dir(paths.generations_dir()).unwrap().count();
    fs::remove_file(paths.manifest_path()).unwrap();

    let reopened = RuntimeStore::open(&paths);

    assert_eq!(reopened.state().backend, "unavailable");
    assert!(
      reopened.state().diagnostics[0]
        .message
        .contains("manifest is missing")
    );
    assert_eq!(
      fs::read_dir(paths.generations_dir()).unwrap().count(),
      generation_entries
    );
    assert!(!paths.manifest_path().exists());
  }

  #[test]
  fn file_store_rotates_segments_and_replays_the_selected_chain() {
    let dir = tempfile::tempdir().unwrap();
    let paths = storage_paths(dir.path());
    let mut store = FileHistoryStore::open(&paths).unwrap();
    store.segment_byte_limit = 700;
    for index in 0..6 {
      store
        .record(HistoryWrite {
          kind: HistoryKind::Runtime,
          summary: format!("Rotated record {index}."),
          registration_id: None,
          domain_key: None,
          payload: serde_json::json!({ "index": index }),
          retention_limit: HISTORY_RETENTION_LIMIT,
        })
        .unwrap();
    }
    drop(store);

    let reopened = FileHistoryStore::open(&paths).unwrap();
    let transaction_dir = paths
      .generations_dir()
      .join(&reopened.generation)
      .join("transactions");

    assert!(fs::read_dir(transaction_dir).unwrap().count() > 1);
    assert_eq!(reopened.records.len(), 6);
    assert_eq!(
      reopened.records.back().unwrap().summary,
      "Rotated record 5."
    );
  }

  #[test]
  fn file_store_rotates_after_the_record_count_limit() {
    let dir = tempfile::tempdir().unwrap();
    let paths = storage_paths(dir.path());
    let mut store = FileHistoryStore::open(&paths).unwrap();
    store.segment_record_limit = 2;
    for index in 0..3 {
      store
        .record(HistoryWrite {
          kind: HistoryKind::Runtime,
          summary: format!("Record {index}."),
          registration_id: None,
          domain_key: None,
          payload: serde_json::json!({ "index": index }),
          retention_limit: HISTORY_RETENTION_LIMIT,
        })
        .unwrap();
    }
    let transaction_dir = paths
      .generations_dir()
      .join(&store.generation)
      .join("transactions");

    assert_eq!(fs::read_dir(transaction_dir).unwrap().count(), 2);
    assert_eq!(store.segment_records, 1);
  }

  #[test]
  fn file_store_removes_an_empty_segment_left_before_manifest_publication() {
    let dir = tempfile::tempdir().unwrap();
    let paths = storage_paths(dir.path());
    let store = FileHistoryStore::open(&paths).unwrap();
    let orphan = paths
      .generations_dir()
      .join(&store.generation)
      .join("transactions")
      .join(format!(
        "{:020}-{}.jsonl",
        store.next_sequence + 1,
        store.generation
      ));
    let orphan_file = create_owner_only_file(&orphan).unwrap();
    durable_sync(&orphan_file).unwrap();
    drop(orphan_file);
    drop(store);

    let reopened = FileHistoryStore::open(&paths).unwrap();

    assert!(!orphan.exists());
    drop(reopened);
  }

  #[tokio::test]
  async fn shutdown_storage_recovers_only_an_incomplete_final_jsonl_tail() {
    let dir = tempfile::tempdir().unwrap();
    let paths = storage_paths(dir.path());
    let store = RuntimeStore::open(&paths);
    store.record_history(
      HistoryKind::Runtime,
      "Committed.",
      None,
      None,
      &serde_json::json!({}),
    );
    store.contain_shutdown().await.unwrap();
    let segment_path = active_segment_path(&paths);
    let committed_len = fs::metadata(&segment_path).unwrap().len();
    fs::OpenOptions::new()
      .append(true)
      .open(&segment_path)
      .unwrap()
      .write_all(br#"{"schemaVersion":1"#)
      .unwrap();

    let reopened = RuntimeStore::open(&paths);
    let records = reopened.query_history(None, 10).await;

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].summary, "Committed.");
    assert_eq!(fs::metadata(&segment_path).unwrap().len(), committed_len);
    assert_eq!(fs::read_dir(paths.recovery_dir()).unwrap().count(), 1);
    assert!(
      reopened
        .state()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "storage-incomplete-tail-recovered")
    );
    reopened.contain_shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn shutdown_storage_rejects_an_empty_complete_interior_record() {
    let dir = tempfile::tempdir().unwrap();
    let paths = storage_paths(dir.path());
    let store = RuntimeStore::open(&paths);
    store.record_history(
      HistoryKind::Runtime,
      "Committed.",
      None,
      None,
      &serde_json::json!({}),
    );
    store.contain_shutdown().await.unwrap();
    let segment_path = active_segment_path(&paths);
    let mut bytes = fs::read(&segment_path).unwrap();
    bytes.extend_from_slice(b"\n");
    fs::write(&segment_path, bytes).unwrap();

    let reopened = RuntimeStore::open(&paths);

    assert_eq!(reopened.state().backend, "unavailable");
    assert!(
      reopened.state().diagnostics[0]
        .message
        .contains("complete history record 2 is empty")
    );
  }

  #[tokio::test]
  async fn shutdown_storage_degrades_on_a_corrupt_complete_jsonl_record() {
    let dir = tempfile::tempdir().unwrap();
    let paths = storage_paths(dir.path());
    let store = RuntimeStore::open(&paths);
    store.record_history(
      HistoryKind::Runtime,
      "Committed.",
      None,
      None,
      &serde_json::json!({}),
    );
    store.contain_shutdown().await.unwrap();
    let segment_path = active_segment_path(&paths);
    let original = fs::read(&segment_path).unwrap();
    let mut corrupt = original.clone();
    let position = corrupt
      .windows(b"Committed".len())
      .position(|window| window == b"Committed")
      .unwrap();
    corrupt[position] = b'X';
    fs::write(&segment_path, &corrupt).unwrap();

    let reopened = RuntimeStore::open(&paths);

    assert_eq!(reopened.state().backend, "unavailable");
    assert!(
      reopened.state().diagnostics[0]
        .message
        .contains("checksum does not match")
    );
    assert!(
      reopened.state().diagnostics[0]
        .message
        .contains("evidence was preserved")
    );
    let recovery = fs::read_dir(paths.recovery_dir())
      .unwrap()
      .next()
      .unwrap()
      .unwrap()
      .path();
    assert!(recovery.join("manifest.json").is_file());
    assert!(recovery.join("generation").is_dir());
    assert_eq!(fs::read(&segment_path).unwrap(), corrupt);
  }

  #[test]
  fn recovery_copy_stops_before_exceeding_its_byte_budget() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.jsonl");
    let destination = dir.path().join("backup.jsonl");
    let mut source_file = create_owner_only_file(&source).unwrap();
    source_file.write_all(b"12345").unwrap();
    drop(source_file);
    let mut budget = RecoveryCopyBudget {
      entries: 0,
      bytes: 0,
    };

    let error = copy_recovery_file_with_limit(&source, &destination, &mut budget, 4).unwrap_err();

    assert!(error.to_string().contains("128 MiB byte limit"));
    assert!(!destination.exists());
    assert_eq!(budget.bytes, 0);
  }

  #[tokio::test]
  async fn shutdown_storage_rejects_new_work_after_ingress_close() {
    let store = RuntimeStore::memory();
    assert!(
      store
        .shutdown_until(Instant::now() + Duration::from_secs(1))
        .await
        .unwrap()
    );

    store.record_history(
      HistoryKind::Runtime,
      "Too late.",
      None,
      None,
      &serde_json::json!({}),
    );

    assert!(
      store
        .state()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "storage-shutting-down")
    );
  }

  #[tokio::test]
  async fn shutdown_storage_timeout_retains_worker_for_containment() {
    let (store, release) = RuntimeStore::memory_stalled_for_test(1);
    store.record_history(
      HistoryKind::Runtime,
      "Queued.",
      None,
      None,
      &serde_json::json!({}),
    );

    assert!(!store.shutdown_until(Instant::now()).await.unwrap());
    release.send(()).unwrap();
    store.contain_shutdown().await.unwrap();

    assert!(
      store
        .state()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "storage-shutdown-contained")
    );
  }

  #[tokio::test]
  async fn shutdown_storage_propagates_worker_flush_failure() {
    let store = RuntimeStore::failing_flush_for_test(false);

    let error = store
      .shutdown_until(Instant::now() + Duration::from_secs(1))
      .await
      .unwrap_err();

    assert!(error.to_string().contains("injected storage flush failure"));
    assert_eq!(store.state().backend, "degraded");
  }

  #[tokio::test]
  async fn shutdown_storage_worker_panic_still_notifies_and_joins() {
    let store = RuntimeStore::failing_flush_for_test(true);

    let error = store
      .shutdown_until(Instant::now() + Duration::from_secs(1))
      .await
      .unwrap_err();

    assert!(error.to_string().contains("injected storage flush panic"));
    assert_eq!(store.state().backend, "degraded");
  }

  #[tokio::test]
  async fn queue_limits_commands_without_unbounded_channel_growth() {
    let (store, release) = RuntimeStore::memory_stalled_for_test(1);
    store.record_history(
      HistoryKind::Runtime,
      "First.",
      None,
      None,
      &serde_json::json!({}),
    );
    store.record_history(
      HistoryKind::Runtime,
      "Dropped.",
      None,
      None,
      &serde_json::json!({}),
    );

    assert!(
      store
        .state()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "storage-history-queue-full")
    );
    release.send(()).unwrap();
    store.contain_shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn prunes_in_memory_query_window_to_retention_limit() {
    let store = RuntimeStore::memory();
    for index in 0..8 {
      store.record_history_with_retention(
        HistoryKind::Runtime,
        format!("Runtime event {index}."),
        None,
        None,
        &serde_json::json!({ "index": index }),
        3,
      );
    }

    let records = store.query_history(None, 500).await;

    assert_eq!(records.len(), 3);
    assert!(records[0].summary.contains('7'));
    assert!(records[2].summary.contains('5'));
  }

  #[test]
  fn unavailable_store_keeps_typed_diagnostic() {
    let path = PathBuf::from("D:/missing/runtime-data");
    let store = RuntimeStore::unavailable(Some(path.clone()), "storage unavailable".to_string());

    let state = store.state();

    assert_eq!(state.backend, "unavailable");
    assert_eq!(
      state.path.as_deref(),
      Some(path.display().to_string().as_str())
    );
    assert_eq!(state.diagnostics[0].code, "storage-unavailable");
  }
}
