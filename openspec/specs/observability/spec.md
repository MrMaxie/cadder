# Observability

## Purpose
Define bounded redacted logs for the retained operator journey.

## Requirements

### Requirement: OBS-001: Redaction precedes persistence and response
Cadder MUST redact credentials and sensitive values before a log reaches SQLite or an IPC response.

#### Scenario: Token-like value appears
- **WHEN** a log contains a token, password, cookie, authorization header, or private key
- **THEN** the retained value contains a stable redaction marker

### Requirement: OBS-002: Log retention is deterministic
The runtime SHALL retain at most 1,000 events per canonical stream and 5,000 events globally, deleting the oldest excess within the insertion transaction.

#### Scenario: Stream limit is exceeded
- **WHEN** a stream receives its 1,001st retained event
- **THEN** its oldest event is removed before commit

### Requirement: OBS-003: Queries are bounded and ordered
A query MUST select one runtime, entrypoint, or domain stream and return at most 200 newest matching rows in ascending sequence order without cursors, paging, tailing, subscriptions, history, or export.

#### Scenario: TUI refreshes Logs
- **WHEN** the TUI requests a limit within 1 through 200
- **THEN** the daemon returns the newest bounded rows in stable ascending order
