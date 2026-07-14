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
}

#[derive(Debug, Default)]
struct LogInner {
  next_sequence: u64,
  entries: VecDeque<LogEntry>,
  per_stream: HashMap<String, StreamRetention>,
}

#[derive(Debug, Default, Clone)]
struct StreamRetention {
  retained_count: usize,
  dropped_count: usize,
  first_sequence: Option<u64>,
  last_sequence: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct LogQuery {
  pub stream: LogStreamIdentity,
  pub limit: usize,
  pub after_sequence: Option<u64>,
  pub minimum_severity: Option<LogSeverity>,
}

#[derive(Debug, Clone)]
pub struct LogQueryResult {
  pub status: LogStreamStatus,
  pub entries: Vec<LogEntry>,
  pub next_cursor: Option<String>,
  pub has_gap: bool,
  pub has_more_before: bool,
  pub truncated_by_retention: bool,
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
    }
  }

  pub fn append(
    &self,
    stream: LogStreamIdentity,
    severity: LogSeverity,
    raw_message: impl AsRef<str>,
    attribution_kind: LogAttributionKind,
    operation: Option<String>,
  ) -> LogEntry {
    let mut inner = self.inner.lock().expect("log mutex poisoned");
    inner.next_sequence += 1;
    let entry = LogEntry {
      sequence_number: inner.next_sequence,
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
    let key = stream_key(&entry.stream);
    inner.entries.push_back(entry.clone());
    let stats = inner.per_stream.entry(key.clone()).or_default();
    stats.retained_count += 1;
    stats.first_sequence.get_or_insert(entry.sequence_number);
    stats.last_sequence = Some(entry.sequence_number);

    while inner.entries.len() > self.max_entries
      || inner
        .per_stream
        .get(&key)
        .map(|stats| stats.retained_count)
        .unwrap_or_default()
        > self.max_per_stream
    {
      let Some(removed) = inner.entries.pop_front() else {
        break;
      };
      let removed_key = stream_key(&removed.stream);
      let first_retained = first_sequence_for_key(&inner.entries, &removed_key);
      if let Some(stats) = inner.per_stream.get_mut(&removed_key) {
        stats.retained_count = stats.retained_count.saturating_sub(1);
        stats.dropped_count += 1;
        stats.first_sequence = first_retained;
      }
    }

    entry
  }

  pub fn query(&self, query: LogQuery, stream_is_active: bool) -> LogQueryResult {
    let inner = self.inner.lock().expect("log mutex poisoned");
    let key = stream_key(&query.stream);
    let retention = inner.per_stream.get(&key).cloned().unwrap_or_default();
    let matching_retained_count = inner
      .entries
      .iter()
      .filter(|entry| same_stream(&entry.stream, &query.stream))
      .filter(|entry| {
        query
          .minimum_severity
          .is_none_or(|min| entry.severity >= min)
      })
      .count();
    let mut entries: Vec<_> = inner
      .entries
      .iter()
      .filter(|entry| same_stream(&entry.stream, &query.stream))
      .filter(|entry| {
        query
          .after_sequence
          .is_none_or(|seq| entry.sequence_number > seq)
      })
      .filter(|entry| {
        query
          .minimum_severity
          .is_none_or(|min| entry.severity >= min)
      })
      .cloned()
      .collect();

    let matched_before_limit = entries.len();
    let has_more_before = entries.len() > query.limit;
    if entries.len() > query.limit {
      let split_at = entries.len() - query.limit;
      entries = entries.split_off(split_at);
    }

    let retention_gap = query.after_sequence.is_some_and(|after_sequence| {
      if let Some(first_sequence) = retention.first_sequence {
        after_sequence.saturating_add(1) < first_sequence
      } else {
        retention
          .last_sequence
          .is_some_and(|last_sequence| after_sequence < last_sequence)
      }
    });
    let tail_gap = query.after_sequence.is_some() && matched_before_limit > query.limit;
    let next_cursor = entries
      .last()
      .map(|entry| format!("seq:{}", entry.sequence_number));

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

    LogQueryResult {
      status,
      entries,
      next_cursor,
      has_gap: retention_gap || tail_gap,
      has_more_before: has_more_before
        || (query.after_sequence.is_none() && retention.dropped_count > 0),
      truncated_by_retention: retention.dropped_count > 0,
    }
  }
}

fn stream_key(stream: &LogStreamIdentity) -> String {
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

fn first_sequence_for_key(entries: &VecDeque<LogEntry>, key: &str) -> Option<u64> {
  entries
    .iter()
    .find(|entry| stream_key(&entry.stream) == key)
    .map(|entry| entry.sequence_number)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn redacts_token_like_values() {
    assert_eq!(
      Redactor::redact("ok Authorization: bearer token=abc password=def"),
      "ok [redacted] [redacted] [redacted] [redacted]"
    );
  }

  #[test]
  fn queries_by_stream_and_cursor() {
    let store = CaddyLogStore::new(10, 10);
    let stream = LogStreamIdentity::domain("app.localhost");
    let other = LogStreamIdentity::domain("api.localhost");
    store.append(
      stream.clone(),
      LogSeverity::Info,
      "first",
      LogAttributionKind::Domain,
      None,
    );
    let first = store.append(
      stream.clone(),
      LogSeverity::Error,
      "second",
      LogAttributionKind::Domain,
      None,
    );
    store.append(
      other,
      LogSeverity::Info,
      "ignored",
      LogAttributionKind::Domain,
      None,
    );

    let result = store.query(
      LogQuery {
        stream,
        limit: 10,
        after_sequence: Some(first.sequence_number - 1),
        minimum_severity: Some(LogSeverity::Error),
      },
      true,
    );

    assert_eq!(result.status, LogStreamStatus::Active);
    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.entries[0].raw_message, "second");
  }

  #[test]
  fn reports_tail_gap_when_more_new_entries_than_limit() {
    let store = CaddyLogStore::new(10, 10);
    let stream = LogStreamIdentity::domain("app.localhost");
    let first = store.append(
      stream.clone(),
      LogSeverity::Info,
      "first",
      LogAttributionKind::Domain,
      None,
    );
    store.append(
      stream.clone(),
      LogSeverity::Info,
      "second",
      LogAttributionKind::Domain,
      None,
    );
    store.append(
      stream.clone(),
      LogSeverity::Info,
      "third",
      LogAttributionKind::Domain,
      None,
    );

    let result = store.query(
      LogQuery {
        stream,
        limit: 1,
        after_sequence: Some(first.sequence_number),
        minimum_severity: None,
      },
      true,
    );

    assert!(result.has_gap);
    assert!(result.has_more_before);
    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.entries[0].raw_message, "third");
  }

  #[test]
  fn reports_retention_truncation_for_old_cursor() {
    let store = CaddyLogStore::new(2, 2);
    let stream = LogStreamIdentity::domain("app.localhost");
    let first = store.append(
      stream.clone(),
      LogSeverity::Info,
      "first",
      LogAttributionKind::Domain,
      None,
    );
    store.append(
      stream.clone(),
      LogSeverity::Info,
      "second",
      LogAttributionKind::Domain,
      None,
    );
    store.append(
      stream.clone(),
      LogSeverity::Info,
      "third",
      LogAttributionKind::Domain,
      None,
    );

    let result = store.query(
      LogQuery {
        stream,
        limit: 10,
        after_sequence: Some(first.sequence_number - 1),
        minimum_severity: None,
      },
      true,
    );

    assert!(result.has_gap);
    assert!(result.truncated_by_retention);
    assert_eq!(result.entries.len(), 2);
  }

  #[test]
  fn active_tail_query_stays_active_when_no_new_entries_arrive() {
    let store = CaddyLogStore::new(10, 10);
    let stream = LogStreamIdentity::domain("app.localhost");
    let first = store.append(
      stream.clone(),
      LogSeverity::Info,
      "first",
      LogAttributionKind::Domain,
      None,
    );

    let result = store.query(
      LogQuery {
        stream,
        limit: 10,
        after_sequence: Some(first.sequence_number),
        minimum_severity: None,
      },
      true,
    );

    assert_eq!(result.status, LogStreamStatus::Active);
    assert!(result.entries.is_empty());
  }
}
