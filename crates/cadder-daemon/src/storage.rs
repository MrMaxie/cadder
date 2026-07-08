use anyhow::{Context, Result, bail};
use cadder_protocol::{HistoryKind, HistoryRecord, RuntimeDiagnostic, StorageState};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;
use std::{
  fmt,
  path::{Path, PathBuf},
  sync::{Arc, Mutex},
  thread,
};
use tokio::sync::{mpsc, oneshot};

const SCHEMA_VERSION: u32 = 1;
const HISTORY_RETENTION_LIMIT: i64 = 10_000;
const STORAGE_QUEUE_CAPACITY: usize = 256;
const MAX_STORAGE_DIAGNOSTICS: usize = 32;

#[derive(Clone)]
pub struct RuntimeStore {
  sender: Option<mpsc::Sender<StorageCommand>>,
  sidecar: Arc<Mutex<StorageSidecar>>,
}

struct StorageSidecar {
  backend: String,
  path: Option<PathBuf>,
  diagnostics: Vec<RuntimeDiagnostic>,
}

struct HistoryWrite {
  kind: HistoryKind,
  summary: String,
  registration_id: Option<String>,
  domain_key: Option<String>,
  payload: Value,
  retention_limit: i64,
}

enum StorageCommand {
  Record(HistoryWrite),
  Query {
    kind: Option<HistoryKind>,
    limit: i64,
    reply: oneshot::Sender<Vec<HistoryRecord>>,
  },
}

impl fmt::Debug for RuntimeStore {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("RuntimeStore")
      .field("state", &self.state())
      .finish()
  }
}

impl RuntimeStore {
  pub fn open(path: impl AsRef<Path>) -> Self {
    let path = path.as_ref();
    match Self::try_open(path) {
      Ok(store) => store,
      Err(error) => Self::unavailable(
        Some(path.to_path_buf()),
        format!("Could not open runtime storage: {error}"),
      ),
    }
  }

  pub fn memory() -> Self {
    match Self::try_memory() {
      Ok(store) => store,
      Err(error) => Self::unavailable(None, format!("Could not open memory storage: {error}")),
    }
  }

  pub fn state(&self) -> StorageState {
    let sidecar = self
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
    retention_limit: i64,
  ) {
    let payload = serde_json::to_value(payload).unwrap_or_else(|error| {
      serde_json::json!({
        "serializationError": error.to_string()
      })
    });
    let write = HistoryWrite {
      kind,
      summary: summary.into(),
      registration_id: registration_id.map(str::to_string),
      domain_key: domain_key.map(str::to_string),
      payload,
      retention_limit,
    };
    let Some(sender) = &self.sender else {
      return;
    };
    match sender.try_send(StorageCommand::Record(write)) {
      Ok(()) => {}
      Err(mpsc::error::TrySendError::Full(_)) => push_diagnostic(
        &self.sidecar,
        "storage-history-queue-full",
        "Runtime history queue is full; dropped history record.".to_string(),
      ),
      Err(mpsc::error::TrySendError::Closed(_)) => push_diagnostic(
        &self.sidecar,
        "storage-history-worker-unavailable",
        "Runtime history worker is unavailable; dropped history record.".to_string(),
      ),
    }
  }

  pub async fn query_history(&self, kind: Option<HistoryKind>, limit: usize) -> Vec<HistoryRecord> {
    let Some(sender) = self.sender.clone() else {
      return Vec::new();
    };
    let (reply, response) = oneshot::channel();
    let command = StorageCommand::Query {
      kind,
      limit: limit.clamp(1, 500) as i64,
      reply,
    };
    match sender.try_send(command) {
      Ok(()) => {}
      Err(mpsc::error::TrySendError::Full(_)) => {
        push_diagnostic(
          &self.sidecar,
          "storage-history-query-queue-full",
          "Runtime history queue is full; could not read history.".to_string(),
        );
        return Vec::new();
      }
      Err(mpsc::error::TrySendError::Closed(_)) => {
        push_diagnostic(
          &self.sidecar,
          "storage-history-worker-unavailable",
          "Runtime history worker is unavailable; could not read history.".to_string(),
        );
        return Vec::new();
      }
    }
    match response.await {
      Ok(records) => records,
      Err(_) => {
        push_diagnostic(
          &self.sidecar,
          "storage-history-worker-unavailable",
          "Runtime history worker closed without returning history.".to_string(),
        );
        Vec::new()
      }
    }
  }

  fn try_memory() -> Result<Self> {
    let connection = Connection::open_in_memory()?;
    initialize_schema(&connection)?;
    Self::with_connection(None, connection, STORAGE_QUEUE_CAPACITY, None)
  }

  fn try_open(path: &Path) -> Result<Self> {
    if let Some(parent) = path.parent() {
      std::fs::create_dir_all(parent)
        .with_context(|| format!("create runtime storage directory {}", parent.display()))?;
    }
    let connection =
      Connection::open(path).with_context(|| format!("open runtime storage {}", path.display()))?;
    initialize_schema(&connection)?;
    Self::with_connection(
      Some(path.to_path_buf()),
      connection,
      STORAGE_QUEUE_CAPACITY,
      None,
    )
  }

  fn with_connection(
    path: Option<PathBuf>,
    connection: Connection,
    queue_capacity: usize,
    start_gate: Option<std::sync::mpsc::Receiver<()>>,
  ) -> Result<Self> {
    let sidecar = Arc::new(Mutex::new(StorageSidecar {
      backend: "sqlite".to_string(),
      path,
      diagnostics: Vec::new(),
    }));
    let sender = spawn_storage_worker(connection, sidecar.clone(), queue_capacity, start_gate)?;
    Ok(Self {
      sender: Some(sender),
      sidecar,
    })
  }

  fn unavailable(path: Option<PathBuf>, message: String) -> Self {
    let sidecar = Arc::new(Mutex::new(StorageSidecar {
      backend: "unavailable".to_string(),
      path,
      diagnostics: vec![RuntimeDiagnostic {
        code: "storage-unavailable".to_string(),
        message,
        operation: Some("open-storage".to_string()),
      }],
    }));
    Self {
      sender: None,
      sidecar,
    }
  }

  #[cfg(test)]
  pub(crate) fn memory_stalled_for_test(
    queue_capacity: usize,
  ) -> (Self, std::sync::mpsc::Sender<()>) {
    let connection = Connection::open_in_memory().unwrap();
    initialize_schema(&connection).unwrap();
    let (release, gate) = std::sync::mpsc::channel();
    let store = Self::with_connection(None, connection, queue_capacity, Some(gate)).unwrap();
    (store, release)
  }
}

fn spawn_storage_worker(
  connection: Connection,
  sidecar: Arc<Mutex<StorageSidecar>>,
  queue_capacity: usize,
  start_gate: Option<std::sync::mpsc::Receiver<()>>,
) -> Result<mpsc::Sender<StorageCommand>> {
  let (sender, mut receiver) = mpsc::channel(queue_capacity);
  thread::Builder::new()
    .name("cadder-runtime-storage".to_string())
    .spawn(move || {
      if let Some(gate) = start_gate {
        let _ = gate.recv();
      }
      while let Some(command) = receiver.blocking_recv() {
        match command {
          StorageCommand::Record(write) => record_history_sync(&connection, &sidecar, write),
          StorageCommand::Query { kind, limit, reply } => {
            let records = query_history_sync(&connection, &sidecar, kind, limit);
            let _ = reply.send(records);
          }
        }
      }
    })
    .context("start runtime storage worker")?;
  Ok(sender)
}

fn record_history_sync(
  connection: &Connection,
  sidecar: &Arc<Mutex<StorageSidecar>>,
  write: HistoryWrite,
) {
  let result = connection.execute(
    "INSERT INTO history (timestamp_utc, kind, summary, registration_id, domain_key, payload_json)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    params![
      Utc::now().to_rfc3339(),
      history_kind_name(write.kind),
      write.summary,
      write.registration_id,
      write.domain_key,
      write.payload.to_string(),
    ],
  );
  if let Err(error) = result {
    push_diagnostic(
      sidecar,
      "storage-history-write",
      format!("Could not persist runtime history: {error}"),
    );
    return;
  }
  if let Err(error) = prune_history(connection, write.retention_limit) {
    push_diagnostic(
      sidecar,
      "storage-history-prune",
      format!("Could not prune old runtime history: {error}"),
    );
  }
}

fn query_history_sync(
  connection: &Connection,
  sidecar: &Arc<Mutex<StorageSidecar>>,
  kind: Option<HistoryKind>,
  limit: i64,
) -> Vec<HistoryRecord> {
  let result = match kind {
    Some(kind) => query_history_by_kind(connection, history_kind_name(kind), limit),
    None => query_history_all(connection, limit),
  };
  match result {
    Ok(records) => records,
    Err(error) => {
      push_diagnostic(
        sidecar,
        "storage-history-read",
        format!("Could not read runtime history: {error}"),
      );
      Vec::new()
    }
  }
}

fn initialize_schema(connection: &Connection) -> Result<()> {
  connection.execute_batch(
    "
    PRAGMA journal_mode = WAL;
    PRAGMA foreign_keys = ON;
    CREATE TABLE IF NOT EXISTS metadata (
      key TEXT PRIMARY KEY NOT NULL,
      value TEXT NOT NULL
    );
    ",
  )?;
  if let Some(version) = existing_schema_version(connection)?
    && version > SCHEMA_VERSION
  {
    bail!(
      "runtime storage schema version {version} is newer than supported version {SCHEMA_VERSION}"
    );
  }
  connection.execute_batch(
    "
    CREATE TABLE IF NOT EXISTS history (
      sequence_number INTEGER PRIMARY KEY AUTOINCREMENT,
      timestamp_utc TEXT NOT NULL,
      kind TEXT NOT NULL,
      summary TEXT NOT NULL,
      registration_id TEXT,
      domain_key TEXT,
      payload_json TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS history_kind_sequence_idx
      ON history(kind, sequence_number DESC);
    CREATE INDEX IF NOT EXISTS history_sequence_idx
      ON history(sequence_number DESC);
    ",
  )?;
  connection.execute(
    "INSERT INTO metadata (key, value) VALUES ('schema_version', ?1)
     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    params![SCHEMA_VERSION.to_string()],
  )?;
  Ok(())
}

fn existing_schema_version(connection: &Connection) -> Result<Option<u32>> {
  let raw = connection
    .query_row(
      "SELECT value FROM metadata WHERE key = 'schema_version'",
      [],
      |row| row.get::<_, String>(0),
    )
    .optional()?;
  raw
    .map(|value| {
      value
        .parse::<u32>()
        .with_context(|| format!("parse runtime storage schema version `{value}`"))
    })
    .transpose()
}

fn prune_history(connection: &Connection, retention_limit: i64) -> rusqlite::Result<usize> {
  connection.execute(
    "DELETE FROM history
     WHERE sequence_number NOT IN (
       SELECT sequence_number FROM history
       ORDER BY sequence_number DESC
       LIMIT ?1
     )",
    params![retention_limit],
  )
}

fn query_history_all(connection: &Connection, limit: i64) -> rusqlite::Result<Vec<HistoryRecord>> {
  let mut statement = connection.prepare(
    "SELECT sequence_number, timestamp_utc, kind, summary, registration_id, domain_key, payload_json
     FROM history
     ORDER BY sequence_number DESC
     LIMIT ?1",
  )?;
  read_history_rows(statement.query(params![limit])?)
}

fn query_history_by_kind(
  connection: &Connection,
  kind: &str,
  limit: i64,
) -> rusqlite::Result<Vec<HistoryRecord>> {
  let mut statement = connection.prepare(
    "SELECT sequence_number, timestamp_utc, kind, summary, registration_id, domain_key, payload_json
     FROM history
     WHERE kind = ?1
     ORDER BY sequence_number DESC
     LIMIT ?2",
  )?;
  read_history_rows(statement.query(params![kind, limit])?)
}

fn read_history_rows(mut rows: rusqlite::Rows<'_>) -> rusqlite::Result<Vec<HistoryRecord>> {
  let mut records = Vec::new();
  while let Some(row) = rows.next()? {
    let kind_name: String = row.get(2)?;
    let timestamp: String = row.get(1)?;
    let payload_json: String = row.get(6)?;
    records.push(HistoryRecord {
      sequence_number: row.get(0)?,
      timestamp_utc: parse_timestamp(&timestamp).unwrap_or_else(Utc::now),
      kind: history_kind_from_name(&kind_name).unwrap_or(HistoryKind::Runtime),
      summary: row.get(3)?,
      registration_id: row.get(4)?,
      domain_key: row.get(5)?,
      payload: serde_json::from_str(&payload_json).unwrap_or(Value::Null),
    });
  }
  Ok(records)
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
  DateTime::parse_from_rfc3339(raw)
    .map(|timestamp| timestamp.with_timezone(&Utc))
    .ok()
}

fn history_kind_name(kind: HistoryKind) -> &'static str {
  match kind {
    HistoryKind::Registration => "registration",
    HistoryKind::Runtime => "runtime",
    HistoryKind::Config => "config",
    HistoryKind::Iis => "iis",
    HistoryKind::Autostart => "autostart",
    HistoryKind::Log => "log",
  }
}

fn history_kind_from_name(raw: &str) -> Option<HistoryKind> {
  match raw {
    "registration" => Some(HistoryKind::Registration),
    "runtime" => Some(HistoryKind::Runtime),
    "config" => Some(HistoryKind::Config),
    "iis" => Some(HistoryKind::Iis),
    "autostart" => Some(HistoryKind::Autostart),
    "log" => Some(HistoryKind::Log),
    _ => None,
  }
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
  use std::time::Duration;

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

    let registrations = store
      .query_history(Some(HistoryKind::Registration), 10)
      .await;

    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0].summary, "Registered shim-1.");
    assert_eq!(registrations[0].registration_id.as_deref(), Some("shim-1"));
  }

  #[test]
  fn opens_file_backed_storage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite3");

    let store = RuntimeStore::open(&path);

    assert_eq!(store.state().backend, "sqlite");
    assert_eq!(
      store.state().path.as_deref(),
      Some(path.display().to_string().as_str())
    );
  }

  #[test]
  fn rejects_newer_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection
      .execute_batch(
        "
        CREATE TABLE metadata (
          key TEXT PRIMARY KEY NOT NULL,
          value TEXT NOT NULL
        );
        INSERT INTO metadata (key, value) VALUES ('schema_version', '999');
        ",
      )
      .unwrap();
    drop(connection);

    let store = RuntimeStore::open(&path);

    let state = store.state();
    assert_eq!(state.backend, "unavailable");
    assert!(
      state.diagnostics[0]
        .message
        .contains("newer than supported version")
    );
  }

  #[tokio::test]
  async fn unavailable_store_keeps_state_diagnostics_and_ignores_history() {
    let path = PathBuf::from("D:/missing/runtime.sqlite3");
    let store = RuntimeStore::unavailable(Some(path.clone()), "storage unavailable".to_string());

    store.record_history(
      HistoryKind::Runtime,
      "Runtime event.",
      None,
      None,
      &serde_json::json!({ "ignored": true }),
    );

    let state = store.state();
    assert_eq!(state.backend, "unavailable");
    assert_eq!(
      state.path.as_deref(),
      Some(path.display().to_string().as_str())
    );
    assert_eq!(state.diagnostics[0].code, "storage-unavailable");
    assert_eq!(
      state.diagnostics[0].operation.as_deref(),
      Some("open-storage")
    );
    assert!(store.query_history(None, 10).await.is_empty());
    assert!(format!("{store:?}").contains("unavailable"));
  }

  #[tokio::test]
  async fn prunes_history_to_retention_limit() {
    let store = RuntimeStore::memory();

    let retention_limit = 3;
    for index in 0..8 {
      store.record_history_with_retention(
        HistoryKind::Runtime,
        format!("Runtime event {index}."),
        None,
        None,
        &serde_json::json!({ "index": index }),
        retention_limit,
      );
    }

    let records = store.query_history(None, 500).await;

    assert_eq!(records.len(), retention_limit as usize);
    assert!(records[0].summary.contains("7"));
    assert!(records[2].summary.contains("5"));
  }

  #[tokio::test]
  async fn records_serialization_errors_and_tolerates_corrupt_history_rows() {
    struct FailingPayload;

    impl Serialize for FailingPayload {
      fn serialize<S>(&self, _serializer: S) -> std::result::Result<S::Ok, S::Error>
      where
        S: serde::Serializer,
      {
        Err(serde::ser::Error::custom("payload failed"))
      }
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite3");
    let store = RuntimeStore::open(&path);
    store.record_history(
      HistoryKind::Log,
      "Could not serialize payload.",
      None,
      None,
      &FailingPayload,
    );
    assert_eq!(
      store.query_history(Some(HistoryKind::Log), 10).await.len(),
      1
    );

    {
      let connection = Connection::open(&path).unwrap();
      connection
        .execute(
          "INSERT INTO history (timestamp_utc, kind, summary, registration_id, domain_key, payload_json)
           VALUES (?1, ?2, ?3, NULL, NULL, ?4)",
          params![
            "not-a-timestamp",
            "unknown-kind",
            "Corrupt row.",
            "{not-json"
          ],
        )
        .unwrap();
    }

    let records = store.query_history(None, 10).await;

    assert_eq!(records.len(), 2);
    let corrupt = records
      .iter()
      .find(|record| record.summary == "Corrupt row.")
      .unwrap();
    assert_eq!(corrupt.kind, HistoryKind::Runtime);
    assert_eq!(corrupt.payload, Value::Null);
    let serialized = records
      .iter()
      .find(|record| record.summary == "Could not serialize payload.")
      .unwrap();
    assert_eq!(serialized.kind, HistoryKind::Log);
    assert_eq!(serialized.payload["serializationError"], "payload failed");
  }

  #[tokio::test]
  async fn storage_query_and_write_failures_are_reported_as_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite3");
    let store = RuntimeStore::open(&path);

    {
      let connection = Connection::open(&path).unwrap();
      connection.execute("DROP TABLE history", []).unwrap();
    }

    store.record_history(
      HistoryKind::Runtime,
      "Write after schema removal.",
      None,
      None,
      &serde_json::json!({ "event": "write" }),
    );
    assert!(store.query_history(None, 10).await.is_empty());

    let diagnostics = store.state().diagnostics;
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "storage-history-write"),
      "{diagnostics:?}"
    );
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "storage-history-read"),
      "{diagnostics:?}"
    );
  }

  #[test]
  fn record_history_reports_queue_full_without_worker_progress() {
    let (store, release) = RuntimeStore::memory_stalled_for_test(1);

    store.record_history(
      HistoryKind::Runtime,
      "First queued event.",
      None,
      None,
      &serde_json::json!({ "index": 1 }),
    );
    store.record_history(
      HistoryKind::Runtime,
      "Dropped event.",
      None,
      None,
      &serde_json::json!({ "index": 2 }),
    );

    let diagnostics = store.state().diagnostics;
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "storage-history-queue-full"),
      "{diagnostics:?}"
    );
    release.send(()).unwrap();
  }

  #[tokio::test]
  async fn query_history_reports_queue_full_without_worker_progress() {
    let (store, release) = RuntimeStore::memory_stalled_for_test(1);
    store.record_history(
      HistoryKind::Runtime,
      "Queued event.",
      None,
      None,
      &serde_json::json!({ "index": 1 }),
    );

    let records = tokio::time::timeout(Duration::from_millis(50), store.query_history(None, 10))
      .await
      .unwrap();
    let diagnostics = store.state().diagnostics;

    assert!(records.is_empty());
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "storage-history-query-queue-full"),
      "{diagnostics:?}"
    );
    release.send(()).unwrap();
  }

  #[tokio::test]
  async fn state_is_available_while_worker_is_stalled() {
    let (store, release) = RuntimeStore::memory_stalled_for_test(1);
    store.record_history(
      HistoryKind::Runtime,
      "Queued event.",
      None,
      None,
      &serde_json::json!({ "queued": true }),
    );

    let state = tokio::time::timeout(Duration::from_millis(50), async { store.state() })
      .await
      .unwrap();

    assert_eq!(state.backend, "sqlite");
    assert!(state.diagnostics.is_empty());
    release.send(()).unwrap();
  }

  #[test]
  fn history_kind_names_roundtrip_all_variants() {
    let cases = [
      (HistoryKind::Registration, "registration"),
      (HistoryKind::Runtime, "runtime"),
      (HistoryKind::Config, "config"),
      (HistoryKind::Iis, "iis"),
      (HistoryKind::Autostart, "autostart"),
      (HistoryKind::Log, "log"),
    ];

    for (kind, name) in cases {
      assert_eq!(history_kind_name(kind), name);
      assert_eq!(history_kind_from_name(name), Some(kind));
    }
    assert_eq!(history_kind_from_name("unknown"), None);
  }
}
