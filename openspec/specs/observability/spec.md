# Observability

## Purpose
Define bounded, redacted diagnostic logs exposed through the operator CLI.

## Requirements

### Requirement: OBS-001: Redaction precedes persistence and response
Cadder MUST redact authorization and cookie header values, token and password values in supported text, JSON, and URL query forms, and private-key blocks before a log reaches SQLite or an IPC response. Retained data SHALL use one stable redaction marker and MUST NOT preserve any part of the matched credential value.

#### Scenario: Documented credential form appears
- **WHEN** a log contains an authorization header, cookie, token, password, or private key
- **THEN** the retained value contains the stable redaction marker instead of the credential value

### Requirement: OBS-002: Log retention is deterministic
The runtime SHALL retain at most 1,000 events per canonical stream and 5,000 events globally, deleting the oldest excess within the insertion transaction.

#### Scenario: Stream limit is exceeded
- **WHEN** a stream receives its 1,001st retained event
- **THEN** its oldest event is removed before commit

### Requirement: OBS-003: Queries are bounded and ordered
A query MUST select one runtime, entrypoint, or domain stream and return at most 200 newest matching rows in ascending sequence order without cursors, paging, tailing, subscriptions, history, or export.

#### Scenario: A diagnostic client requests logs
- **WHEN** a client requests a limit within 1 through 200 for one canonical stream
- **THEN** the daemon returns the newest redacted rows in stable ascending order

### Requirement: OBS-004: Terminal rendering is control-safe
The operator CLI MUST neutralize ANSI escape sequences and C0 or C1 control characters in diagnostic and log messages before writing them to an interactive terminal. Redacted retained log content SHALL otherwise remain unchanged.

#### Scenario: Diagnostic contains terminal controls
- **WHEN** a retained log or diagnostic message contains an ANSI sequence or another C0 or C1 control character
- **THEN** the CLI renders the message without executing or forwarding that control sequence
