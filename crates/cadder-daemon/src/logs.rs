use crate::database::Database;
use cadder_ipc::{
  LogAttributionKind, LogEntry, LogEntryKind, LogSeverity, LogStreamIdentity, LogStreamStatus,
};
use chrono::Utc;
use std::{
  collections::{HashMap, VecDeque},
  sync::{Arc, Mutex},
};

#[derive(Debug, Clone)]
pub struct Redactor;

impl Redactor {
  pub fn redact(input: &str) -> String {
    let mut output = Vec::new();
    for token in input.split_whitespace() {
      let lower = token.to_ascii_lowercase();
      if lower.contains("authorization:")
        || lower.starts_with("bearer")
        || lower.contains("token=")
        || lower.contains("password=")
        || lower.contains("secret=")
      {
        output.push("[redacted]");
      } else {
        output.push(token);
      }
    }
    output.join(" ")
  }
}

#[derive(Debug, Clone)]
pub struct CaddyLogStore {
  inner: Arc<Mutex<LogInner>>,
  max_entries: usize,
  max_per_stream: usize,
  database: Option<Database>,
  durability_error: Arc<Mutex<Option<String>>>,
}

#[derive(Debug, Default)]
struct LogInner {
  next_sequence: u64,
  entries: VecDeque<LogEntry>,
  per_stream: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct LogQuery {
  pub stream: LogStreamIdentity,
  pub limit: usize,
}

#[derive(Debug, Clone)]
pub struct LogQueryResult {
  pub status: LogStreamStatus,
  pub entries: Vec<LogEntry>,
}

impl Default for CaddyLogStore {
  fn default() -> Self {
    Self::new(5_000, 1_000)
  }
}

impl CaddyLogStore {
  pub fn new(max_entries: usize, max_per_stream: usize) -> Self {
    Self {
      inner: Arc::new(Mutex::new(LogInner::default())),
      max_entries,
      max_per_stream,
      database: None,
      durability_error: Arc::new(Mutex::new(None)),
    }
  }

  pub(crate) fn with_database(database: Database) -> Self {
    Self {
      database: Some(database),
      ..Self::default()
    }
  }

  pub async fn append(
    &self,
    stream: LogStreamIdentity,
    severity: LogSeverity,
    raw_message: impl AsRef<str>,
    attribution_kind: LogAttributionKind,
    operation: Option<String>,
  ) -> LogEntry {
    let entry = LogEntry {
      sequence_number: 0,
      timestamp_utc: Utc::now(),
      severity,
      domain_key: stream.domain_key.clone(),
      stream,
      attribution_kind,
      entry_kind: LogEntryKind::Normal,
      raw_message: Redactor::redact(raw_message.as_ref()),
      source_registration_id: None,
      source_instance_id: None,
      operation,
    };
    if let Some(database) = &self.database {
      return match database.append_log(entry.clone()).await {
        Ok(entry) => {
          *self.durability_error.lock().expect("log mutex poisoned") = None;
          entry
        }
        Err(_) => {
          *self.durability_error.lock().expect("log mutex poisoned") = Some(
            "Cadder could not persist a log event; active traffic was left unchanged.".to_string(),
          );
          entry
        }
      };
    }
    let mut inner = self.inner.lock().expect("log mutex poisoned");
    inner.next_sequence += 1;
    let mut entry = entry;
    entry.sequence_number = inner.next_sequence;
    let key = stream_key(&entry.stream);
    inner.entries.push_back(entry.clone());
    *inner.per_stream.entry(key.clone()).or_default() += 1;

    while inner.entries.len() > self.max_entries
      || inner.per_stream.get(&key).copied().unwrap_or_default() > self.max_per_stream
    {
      let Some(removed) = inner.entries.pop_front() else {
        break;
      };
      let removed_key = stream_key(&removed.stream);
      if let Some(count) = inner.per_stream.get_mut(&removed_key) {
        *count = count.saturating_sub(1);
      }
    }

    entry
  }

  pub async fn query(&self, query: LogQuery, stream_is_active: bool) -> LogQueryResult {
    if let Some(database) = &self.database {
      return database
        .query_logs(query, stream_is_active)
        .await
        .unwrap_or_else(|_| LogQueryResult {
          status: LogStreamStatus::ReadError,
          entries: Vec::new(),
        });
    }
    let inner = self.inner.lock().expect("log mutex poisoned");
    let matching_retained_count = inner
      .entries
      .iter()
      .filter(|entry| same_stream(&entry.stream, &query.stream))
      .count();
    let mut entries: Vec<_> = inner
      .entries
      .iter()
      .filter(|entry| same_stream(&entry.stream, &query.stream))
      .cloned()
      .collect();

    if entries.len() > query.limit {
      let split_at = entries.len() - query.limit;
      entries = entries.split_off(split_at);
    }

    let status = if matching_retained_count == 0 {
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

    LogQueryResult { status, entries }
  }

  pub(crate) fn durability_diagnostic(&self) -> Option<String> {
    self
      .durability_error
      .lock()
      .expect("log mutex poisoned")
      .clone()
  }
}

pub(crate) fn stream_key(stream: &LogStreamIdentity) -> String {
  format!(
    "{}|{}|{}",
    stream.stream_id,
    stream.channel,
    stream.domain_key.as_deref().unwrap_or_default()
  )
}

fn same_stream(left: &LogStreamIdentity, right: &LogStreamIdentity) -> bool {
  left.stream_id == right.stream_id
    && left.channel == right.channel
    && left.domain_key == right.domain_key
}

#[cfg(test)]
mod tests;
