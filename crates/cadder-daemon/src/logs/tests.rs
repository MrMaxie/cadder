use super::*;

#[test]
fn redacts_token_like_values() {
  assert_eq!(
    Redactor::redact("ok Authorization: bearer token=abc password=def"),
    "ok [redacted] [redacted] [redacted] [redacted]"
  );
}

#[tokio::test]
async fn query_returns_only_the_newest_requested_entries_for_one_stream() {
  let store = CaddyLogStore::new(10, 10);
  let stream = LogStreamIdentity::domain("app.localhost");
  for message in ["first", "second", "third"] {
    store
      .append(
        stream.clone(),
        LogSeverity::Info,
        message,
        LogAttributionKind::Domain,
        None,
      )
      .await;
  }
  store
    .append(
      LogStreamIdentity::domain("other.localhost"),
      LogSeverity::Info,
      "ignored",
      LogAttributionKind::Domain,
      None,
    )
    .await;

  let result = store.query(LogQuery { stream, limit: 2 }, true).await;

  assert_eq!(result.status, LogStreamStatus::Active);
  assert_eq!(
    result
      .entries
      .iter()
      .map(|entry| entry.raw_message.as_str())
      .collect::<Vec<_>>(),
    ["second", "third"]
  );
}

#[tokio::test]
async fn retention_limits_are_enforced_without_exposing_paging_state() {
  let store = CaddyLogStore::new(2, 2);
  let stream = LogStreamIdentity::domain("app.localhost");
  for message in ["first", "second", "third"] {
    store
      .append(
        stream.clone(),
        LogSeverity::Info,
        message,
        LogAttributionKind::Domain,
        None,
      )
      .await;
  }

  let result = store.query(LogQuery { stream, limit: 10 }, true).await;
  assert_eq!(result.entries.len(), 2);
  assert_eq!(result.entries[0].raw_message, "second");
}
