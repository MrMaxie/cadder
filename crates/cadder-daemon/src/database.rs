use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use cadder_ipc::{
  ActivationState, EntrypointRegistration, LogEntry, LogStreamStatus, RuntimeDiagnostic,
  StorageState,
};
use tokio::sync::Semaphore;
use tokio_rusqlite::Connection;
use tokio_rusqlite::rusqlite::{self, InterruptHandle, OptionalExtension};

use crate::logs::{LogQuery, LogQueryResult, stream_key};
use crate::paths::StoragePaths;

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
  match crate::ipc_windows_security::create_owner_only_runtime_directory(path) {
    Ok(()) => {}
    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
    Err(error) => return Err(error.into()),
  }
  crate::ipc_windows_security::secure_owner_only_runtime_directory(path)?;
  Ok(())
}

#[cfg(not(any(unix, windows)))]
fn secure_owner_only_directory(path: &Path) -> Result<()> {
  fs::create_dir_all(path).map_err(Into::into)
}

#[cfg(unix)]
fn create_owner_only_file(path: &Path) -> Result<fs::File> {
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
fn create_owner_only_file(path: &Path) -> Result<fs::File> {
  crate::ipc_windows_security::create_owner_only_runtime_file(path).map_err(Into::into)
}

#[cfg(not(any(unix, windows)))]
fn create_owner_only_file(path: &Path) -> Result<fs::File> {
  Ok(
    fs::OpenOptions::new()
      .read(true)
      .write(true)
      .create_new(true)
      .open(path)?,
  )
}

const SCHEMA_VERSION: i64 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_secs(2);
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const LEGACY_ARTIFACTS: [&str; 6] = [
  "manifest.json",
  "generations",
  "recovery",
  "plans",
  "secrets",
  "storage.lock",
];

#[derive(Clone)]
pub(crate) struct Database {
  connection: Connection,
  gate: Arc<Semaphore>,
  interrupt: Arc<InterruptHandle>,
  path: PathBuf,
  diagnostics: Arc<Vec<RuntimeDiagnostic>>,
}

impl std::fmt::Debug for Database {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("Database")
      .field("path", &self.path)
      .finish_non_exhaustive()
  }
}

impl Database {
  pub(crate) async fn open(paths: &StoragePaths) -> Result<Self> {
    let data_dir = paths.profile_dir();
    secure_owner_only_directory(data_dir)
      .with_context(|| format!("secure database directory {}", data_dir.display()))?;
    let path = data_dir.join("cadder.sqlite3");
    if !path.exists() {
      drop(
        create_owner_only_file(&path)
          .with_context(|| format!("create database file {}", path.display()))?,
      );
    }

    let connection = Connection::open(&path)
      .await
      .with_context(|| format!("open SQLite database {}", path.display()))?;
    let interrupt = connection
      .call(|connection| -> rusqlite::Result<_> { Ok(connection.get_interrupt_handle()) })
      .await
      .map_err(|error| anyhow!(error.to_string()))?;
    let database = Self {
      connection,
      gate: Arc::new(Semaphore::new(1)),
      interrupt: Arc::new(interrupt),
      path,
      diagnostics: Arc::new(Vec::new()),
    };
    database.initialize().await?;
    let diagnostics = database.cleanup_legacy_artifacts();
    let database = Self {
      diagnostics: Arc::new(diagnostics),
      ..database
    };
    Ok(database)
  }

  #[cfg(test)]
  pub(crate) fn path(&self) -> &Path {
    &self.path
  }

  pub(crate) fn state(&self) -> StorageState {
    StorageState {
      backend: "sqlite".to_string(),
      path: Some(self.path.display().to_string()),
      schema_version: SCHEMA_VERSION as u32,
      diagnostics: self.diagnostics.as_ref().clone(),
    }
  }

  pub(crate) async fn restore_desired_state(
    &self,
    registration: &mut EntrypointRegistration,
  ) -> Result<()> {
    let project_root = registration.source_working_directory.raw.clone();
    let desired = self
      .call(move |connection| {
        let entrypoint = connection
          .query_row(
            "SELECT desired_enabled FROM entrypoints WHERE entrypoint_key = ?1",
            [&project_root],
            |row| row.get::<_, bool>(0),
          )
          .optional()?;
        let mut domains = std::collections::HashMap::new();
        let mut statement = connection
          .prepare("SELECT domain_key, desired_enabled FROM domains WHERE entrypoint_key = ?1")?;
        for row in statement.query_map([&project_root], |row| {
          Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
        })? {
          let (domain, enabled) = row?;
          domains.insert(domain, enabled);
        }
        Ok((entrypoint, domains))
      })
      .await?;
    if let Some(enabled) = desired.0 {
      registration.activation_state = ActivationState::from_enabled(enabled);
    }
    for domain in &mut registration.registered_domains {
      if let Some(enabled) = desired.1.get(&domain.name.canonical) {
        domain.activation_state = ActivationState::from_enabled(*enabled);
      }
    }
    Ok(())
  }

  pub(crate) async fn persist_desired_state(
    &self,
    registrations: Vec<EntrypointRegistration>,
  ) -> Result<()> {
    self
      .call(move |connection| {
        let transaction = connection.transaction()?;
        for registration in registrations {
          let entrypoint_key = registration.source_working_directory.raw.clone();
          transaction.execute(
            "INSERT INTO entrypoints (
               entrypoint_key, registration_id, project_root, caddyfile_path, desired_enabled
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(entrypoint_key) DO UPDATE SET
               registration_id = excluded.registration_id,
               project_root = excluded.project_root,
               caddyfile_path = excluded.caddyfile_path,
               desired_enabled = excluded.desired_enabled",
            rusqlite::params![
              entrypoint_key,
              registration.registration_id,
              registration.source_working_directory.raw,
              registration.source_config_path.raw,
              registration.activation_state.is_enabled(),
            ],
          )?;
          transaction.execute(
            "DELETE FROM domains WHERE entrypoint_key = ?1",
            [&entrypoint_key],
          )?;
          for domain in registration.registered_domains {
            let canonical_host = domain.name.canonical;
            transaction.execute(
              "INSERT INTO domains (
                 entrypoint_key, domain_key, canonical_host, desired_enabled
               ) VALUES (?1, ?2, ?3, ?4)",
              rusqlite::params![
                entrypoint_key,
                canonical_host,
                canonical_host,
                domain.activation_state.is_enabled(),
              ],
            )?;
          }
        }
        transaction.commit()
      })
      .await
  }

  pub(crate) async fn append_log(&self, mut entry: LogEntry) -> Result<LogEntry> {
    let stream_key = stream_key(&entry.stream);
    let timestamp = entry.timestamp_utc.to_rfc3339();
    let source = serde_json::to_string(&entry.attribution_kind)?;
    let severity = serde_json::to_string(&entry.severity)?;
    let event_name = serde_json::to_string(&entry.entry_kind)?;
    let fields_json = serde_json::to_string(&entry)?;
    let domain_key = entry.domain_key.clone();
    let entrypoint_key = entry.source_registration_id.clone();
    let request_id = entry.operation.clone();
    let message = entry.raw_message.clone();
    let sequence = self
      .call(move |connection| {
        let transaction = connection.transaction()?;
        transaction.execute(
          "INSERT INTO log_events (
             stream_key, timestamp, source, severity, event_name, message,
             entrypoint_key, domain_key, request_id, fields_json, redaction_json, truncation_json
           ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, '{}', NULL)",
          rusqlite::params![
            stream_key,
            timestamp,
            source,
            severity,
            event_name,
            message,
            entrypoint_key,
            domain_key,
            request_id,
            fields_json,
          ],
        )?;
        let sequence = transaction.last_insert_rowid();
        transaction.execute(
          "DELETE FROM log_events
           WHERE stream_key = ?1
             AND sequence NOT IN (
               SELECT sequence FROM log_events
               WHERE stream_key = ?1
               ORDER BY sequence DESC
               LIMIT 1000
             )",
          [&stream_key],
        )?;
        transaction.execute(
          "DELETE FROM log_events
           WHERE sequence NOT IN (
             SELECT sequence FROM log_events ORDER BY sequence DESC LIMIT 5000
           )",
          [],
        )?;
        transaction.commit()?;
        Ok(sequence as u64)
      })
      .await?;
    entry.sequence_number = sequence;
    Ok(entry)
  }

  pub(crate) async fn query_logs(
    &self,
    query: LogQuery,
    stream_is_active: bool,
  ) -> Result<LogQueryResult> {
    let key = stream_key(&query.stream);
    let limit = query.limit.clamp(1, 200) as i64;
    let entries = self
      .call(move |connection| {
        let mut statement = connection.prepare(
          "SELECT sequence, fields_json FROM (
             SELECT sequence, fields_json FROM log_events
             WHERE stream_key = ?1
             ORDER BY sequence DESC
             LIMIT ?2
           ) ORDER BY sequence ASC",
        )?;
        statement
          .query_map(rusqlite::params![key, limit], |row| {
            Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?))
          })?
          .collect::<rusqlite::Result<Vec<_>>>()
      })
      .await?;
    let mut decoded = Vec::with_capacity(entries.len());
    for (sequence, json) in entries {
      let mut entry: LogEntry = serde_json::from_str(&json)
        .with_context(|| format!("decode persisted log event {sequence}"))?;
      entry.sequence_number = sequence;
      decoded.push(entry);
    }
    let status = if decoded.is_empty() {
      if stream_is_active {
        LogStreamStatus::Empty
      } else {
        LogStreamStatus::Removed
      }
    } else if stream_is_active {
      LogStreamStatus::Active
    } else {
      LogStreamStatus::Stale
    };
    Ok(LogQueryResult {
      status,
      entries: decoded,
    })
  }

  async fn initialize(&self) -> Result<()> {
    self
      .call(|connection| {
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.execute_batch(
          "PRAGMA journal_mode = DELETE;
           PRAGMA synchronous = FULL;
           PRAGMA foreign_keys = ON;",
        )?;
        let integrity: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if integrity != "ok" {
          return Err(rusqlite::Error::InvalidQuery);
        }

        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        match version {
          0 if application_tables(connection)?.is_empty() => create_schema(connection),
          0 => Err(rusqlite::Error::InvalidQuery),
          SCHEMA_VERSION => validate_schema(connection),
          _ => Err(rusqlite::Error::InvalidQuery),
        }
      })
      .await
      .with_context(|| format!("validate SQLite database {}", self.path.display()))
  }

  pub(crate) async fn call<R, F>(&self, operation: F) -> Result<R>
  where
    R: Send + 'static,
    F: FnOnce(&mut rusqlite::Connection) -> rusqlite::Result<R> + Send + 'static,
  {
    let _permit = self
      .gate
      .acquire()
      .await
      .map_err(|_| anyhow!("database admission is closed"))?;
    let call = self.connection.call(operation);
    tokio::pin!(call);
    tokio::select! {
      result = &mut call => map_call_result(result),
      _ = tokio::time::sleep(CALL_TIMEOUT) => {
        self.interrupt.interrupt();
        let terminal = call.await;
        let _ = map_call_result(terminal);
        bail!("SQLite operation exceeded its deadline and was rolled back")
      }
    }
  }

  pub(crate) async fn close(self) -> Result<()> {
    let _permit = self
      .gate
      .acquire()
      .await
      .map_err(|_| anyhow!("database admission is closed"))?;
    self.interrupt.interrupt();
    self
      .connection
      .close()
      .await
      .map_err(|error| anyhow!(error.to_string()))
  }

  fn cleanup_legacy_artifacts(&self) -> Vec<RuntimeDiagnostic> {
    let data_dir = self
      .path
      .parent()
      .expect("database path has a data directory");
    let mut diagnostics = Vec::new();
    for name in LEGACY_ARTIFACTS {
      if cleanup_legacy_artifact(data_dir, name).is_err() {
        diagnostics.push(RuntimeDiagnostic {
          code: "legacy-storage-cleanup-failed".to_string(),
          message: format!(
            "Cadder could not remove the legacy storage artifact `{name}`; it will retry on the next start."
          ),
          operation: Some("legacy-storage-cleanup".to_string()),
        });
      }
    }
    diagnostics
  }
}

fn cleanup_legacy_artifact(data_dir: &Path, name: &str) -> Result<()> {
  let target = data_dir.join(name);
  let metadata = match fs::symlink_metadata(&target) {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
    Err(error) => return Err(error).with_context(|| format!("inspect legacy {name}")),
  };
  if metadata.file_type().is_symlink() {
    bail!("refusing to clean linked legacy artifact {name}");
  }
  if target.parent() != Some(data_dir) {
    bail!("refusing to clean a legacy artifact outside the data directory");
  }
  if metadata.is_dir() {
    fs::remove_dir_all(&target).with_context(|| format!("remove legacy {name}"))?;
  } else {
    fs::remove_file(&target).with_context(|| format!("remove legacy {name}"))?;
  }
  Ok(())
}

fn map_call_result<R, E>(result: std::result::Result<R, tokio_rusqlite::Error<E>>) -> Result<R>
where
  E: std::error::Error + Send + Sync + 'static,
{
  result.map_err(|error| anyhow!(error))
}

fn application_tables(connection: &rusqlite::Connection) -> rusqlite::Result<Vec<String>> {
  let mut statement = connection.prepare(
    "SELECT name FROM sqlite_schema
     WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
     ORDER BY name",
  )?;
  statement
    .query_map([], |row| row.get(0))?
    .collect::<rusqlite::Result<Vec<_>>>()
}

fn create_schema(connection: &mut rusqlite::Connection) -> rusqlite::Result<()> {
  let transaction = connection.transaction()?;
  transaction.execute_batch(
    "CREATE TABLE entrypoints (
       entrypoint_key TEXT PRIMARY KEY NOT NULL,
       registration_id TEXT NOT NULL UNIQUE,
       project_root TEXT NOT NULL,
       caddyfile_path TEXT NOT NULL,
       desired_enabled INTEGER NOT NULL CHECK (desired_enabled IN (0, 1))
     );
     CREATE TABLE domains (
       entrypoint_key TEXT NOT NULL REFERENCES entrypoints(entrypoint_key) ON DELETE CASCADE,
       domain_key TEXT NOT NULL,
       canonical_host TEXT NOT NULL,
       desired_enabled INTEGER NOT NULL CHECK (desired_enabled IN (0, 1)),
       PRIMARY KEY (entrypoint_key, domain_key)
     );
     CREATE TABLE log_events (
       sequence INTEGER PRIMARY KEY AUTOINCREMENT,
       stream_key TEXT NOT NULL,
       timestamp TEXT NOT NULL,
       source TEXT NOT NULL,
       severity TEXT NOT NULL,
       event_name TEXT NOT NULL,
       message TEXT NOT NULL,
       entrypoint_key TEXT,
       domain_key TEXT,
       request_id TEXT,
       fields_json TEXT NOT NULL,
       redaction_json TEXT NOT NULL,
       truncation_json TEXT
     );
     CREATE INDEX log_events_stream_sequence_idx
       ON log_events (stream_key, sequence);
     PRAGMA user_version = 1;",
  )?;
  transaction.commit()
}

fn validate_schema(connection: &mut rusqlite::Connection) -> rusqlite::Result<()> {
  let tables = application_tables(connection)?;
  if tables != ["domains", "entrypoints", "log_events"] {
    return Err(rusqlite::Error::InvalidQuery);
  }
  validate_columns(
    connection,
    "entrypoints",
    &[
      ("entrypoint_key", "TEXT", 1, 1),
      ("registration_id", "TEXT", 1, 0),
      ("project_root", "TEXT", 1, 0),
      ("caddyfile_path", "TEXT", 1, 0),
      ("desired_enabled", "INTEGER", 1, 0),
    ],
  )?;
  validate_columns(
    connection,
    "domains",
    &[
      ("entrypoint_key", "TEXT", 1, 1),
      ("domain_key", "TEXT", 1, 2),
      ("canonical_host", "TEXT", 1, 0),
      ("desired_enabled", "INTEGER", 1, 0),
    ],
  )?;
  validate_columns(
    connection,
    "log_events",
    &[
      ("sequence", "INTEGER", 0, 1),
      ("stream_key", "TEXT", 1, 0),
      ("timestamp", "TEXT", 1, 0),
      ("source", "TEXT", 1, 0),
      ("severity", "TEXT", 1, 0),
      ("event_name", "TEXT", 1, 0),
      ("message", "TEXT", 1, 0),
      ("entrypoint_key", "TEXT", 0, 0),
      ("domain_key", "TEXT", 0, 0),
      ("request_id", "TEXT", 0, 0),
      ("fields_json", "TEXT", 1, 0),
      ("redaction_json", "TEXT", 1, 0),
      ("truncation_json", "TEXT", 0, 0),
    ],
  )?;
  let index: Option<String> = connection
    .query_row(
      "SELECT name FROM sqlite_schema WHERE type = 'index' AND name = ?1",
      ["log_events_stream_sequence_idx"],
      |row| row.get(0),
    )
    .optional()?;
  if index.is_none() {
    return Err(rusqlite::Error::InvalidQuery);
  }
  let mut index_columns =
    connection.prepare("PRAGMA index_info(log_events_stream_sequence_idx)")?;
  let index_columns = index_columns
    .query_map([], |row| row.get::<_, String>(2))?
    .collect::<rusqlite::Result<Vec<_>>>()?;
  if index_columns != ["stream_key", "sequence"] {
    return Err(rusqlite::Error::InvalidQuery);
  }
  let mut foreign_keys = connection.prepare("PRAGMA foreign_key_list(domains)")?;
  let foreign_keys = foreign_keys
    .query_map([], |row| {
      Ok((
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        row.get::<_, String>(6)?,
      ))
    })?
    .collect::<rusqlite::Result<Vec<_>>>()?;
  if foreign_keys
    != [(
      "entrypoints".to_string(),
      "entrypoint_key".to_string(),
      "entrypoint_key".to_string(),
      "CASCADE".to_string(),
    )]
  {
    return Err(rusqlite::Error::InvalidQuery);
  }
  Ok(())
}

fn validate_columns(
  connection: &rusqlite::Connection,
  table: &str,
  expected: &[(&str, &str, i64, i64)],
) -> rusqlite::Result<()> {
  let pragma = match table {
    "entrypoints" => "PRAGMA table_info(entrypoints)",
    "domains" => "PRAGMA table_info(domains)",
    "log_events" => "PRAGMA table_info(log_events)",
    _ => return Err(rusqlite::Error::InvalidQuery),
  };
  let mut statement = connection.prepare(pragma)?;
  let actual = statement
    .query_map([], |row| {
      Ok((
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, i64>(3)?,
        row.get::<_, i64>(5)?,
      ))
    })?
    .collect::<rusqlite::Result<Vec<_>>>()?;
  let expected = expected
    .iter()
    .map(|(name, kind, not_null, primary_key)| {
      (
        (*name).to_string(),
        (*kind).to_string(),
        *not_null,
        *primary_key,
      )
    })
    .collect::<Vec<_>>();
  if actual != expected {
    return Err(rusqlite::Error::InvalidQuery);
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_ipc::{LogAttributionKind, LogSeverity, LogStreamIdentity};

  #[tokio::test]
  async fn creates_and_reopens_the_exact_schema() {
    let temp = tempfile::tempdir().unwrap();
    let paths = StoragePaths::new_for_test(temp.path().join("data"));

    let database = Database::open(&paths).await.unwrap();
    assert_eq!(database.path(), temp.path().join("data/cadder.sqlite3"));
    database.close().await.unwrap();

    let reopened = Database::open(&paths).await.unwrap();
    let version: i64 = reopened
      .call(|connection| connection.query_row("PRAGMA user_version", [], |row| row.get(0)))
      .await
      .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
  }

  #[tokio::test]
  async fn rejects_a_non_empty_version_zero_database() {
    let temp = tempfile::tempdir().unwrap();
    let paths = StoragePaths::new_for_test(temp.path().join("data"));
    secure_owner_only_directory(paths.profile_dir()).unwrap();
    let path = paths.profile_dir().join("cadder.sqlite3");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
      .execute("CREATE TABLE unexpected (id INTEGER)", [])
      .unwrap();
    drop(connection);

    assert!(Database::open(&paths).await.is_err());
  }

  #[tokio::test]
  async fn cleanup_preserves_unknown_data_entries() {
    let temp = tempfile::tempdir().unwrap();
    let paths = StoragePaths::new_for_test(temp.path().join("data"));
    secure_owner_only_directory(paths.profile_dir()).unwrap();
    fs::write(paths.profile_dir().join("keep.txt"), "keep").unwrap();
    fs::write(paths.profile_dir().join("manifest.json"), "legacy").unwrap();

    let database = Database::open(&paths).await.unwrap();

    assert!(paths.profile_dir().join("keep.txt").exists());
    assert!(!paths.profile_dir().join("manifest.json").exists());
    database.close().await.unwrap();
  }

  #[tokio::test]
  async fn rejects_partial_newer_and_corrupt_databases_before_cleanup() {
    for setup in ["partial", "newer", "corrupt"] {
      let temp = tempfile::tempdir().unwrap();
      let paths = StoragePaths::new_for_test(temp.path().join("data"));
      secure_owner_only_directory(paths.profile_dir()).unwrap();
      let database_path = paths.profile_dir().join("cadder.sqlite3");
      match setup {
        "partial" => {
          let connection = rusqlite::Connection::open(&database_path).unwrap();
          connection
            .execute_batch("CREATE TABLE entrypoints (entrypoint_key TEXT); PRAGMA user_version=1;")
            .unwrap();
        }
        "newer" => {
          let mut connection = rusqlite::Connection::open(&database_path).unwrap();
          create_schema(&mut connection).unwrap();
          drop(connection);
          let connection = rusqlite::Connection::open(&database_path).unwrap();
          connection.pragma_update(None, "user_version", 2).unwrap();
        }
        "corrupt" => fs::write(&database_path, b"not a sqlite database").unwrap(),
        _ => unreachable!(),
      }
      fs::write(paths.profile_dir().join("manifest.json"), "legacy").unwrap();

      assert!(Database::open(&paths).await.is_err(), "{setup}");
      assert!(
        paths.profile_dir().join("manifest.json").exists(),
        "{setup}"
      );
    }
  }

  #[tokio::test]
  async fn persists_redacted_logs_across_restarts() {
    let temp = tempfile::tempdir().unwrap();
    let paths = StoragePaths::new_for_test(temp.path().join("data"));
    let database = Database::open(&paths).await.unwrap();
    let logs = crate::logs::CaddyLogStore::with_database(database.clone());
    let stream = LogStreamIdentity::domain("app.localhost");

    logs
      .append(
        stream.clone(),
        LogSeverity::Error,
        "reload failed token=private",
        LogAttributionKind::Domain,
        Some("reload".to_string()),
      )
      .await;
    database.close().await.unwrap();

    let reopened = Database::open(&paths).await.unwrap();
    let result = reopened
      .query_logs(LogQuery { stream, limit: 200 }, true)
      .await
      .unwrap();
    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.entries[0].raw_message, "reload failed [redacted]");
    reopened.close().await.unwrap();
  }

  #[tokio::test]
  async fn log_insert_enforces_stream_and_global_retention() {
    let temp = tempfile::tempdir().unwrap();
    let paths = StoragePaths::new_for_test(temp.path().join("data"));
    let database = Database::open(&paths).await.unwrap();
    database
      .call(|connection| {
        let transaction = connection.transaction()?;
        for sequence in 0..5_100 {
          let stream = if sequence < 1_100 {
            "busy||"
          } else {
            "other||"
          };
          transaction.execute(
            "INSERT INTO log_events (
               stream_key, timestamp, source, severity, event_name, message,
               fields_json, redaction_json
             ) VALUES (?1, 'now', 'runtime', 'info', 'normal', 'seed', '{}', '{}')",
            [stream],
          )?;
        }
        transaction.commit()
      })
      .await
      .unwrap();
    let logs = crate::logs::CaddyLogStore::with_database(database.clone());
    logs
      .append(
        LogStreamIdentity {
          stream_id: "busy".to_string(),
          domain_key: None,
          channel: String::new(),
        },
        LogSeverity::Info,
        "retention trigger",
        LogAttributionKind::Runtime,
        None,
      )
      .await;
    let (total, busy): (i64, i64) = database
      .call(|connection| {
        Ok((
          connection.query_row("SELECT COUNT(*) FROM log_events", [], |row| row.get(0))?,
          connection.query_row(
            "SELECT COUNT(*) FROM log_events WHERE stream_key = 'busy||'",
            [],
            |row| row.get(0),
          )?,
        ))
      })
      .await
      .unwrap();
    assert!(total <= 5_000);
    assert!(busy <= 1_000);
    database.close().await.unwrap();
  }
}
