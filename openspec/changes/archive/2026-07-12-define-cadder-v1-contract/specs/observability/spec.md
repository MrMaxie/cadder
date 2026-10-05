## ADDED Requirements

### Requirement: OBS-001: Runtime logs use one structured schema
The daemon SHALL represent every persisted and streamed operational log as a versioned `LogEvent` with log sequence, timestamp, source, severity, event name, message, runtime profile, optional entrypoint, domain and request identifiers, structured fields, and redaction metadata. `HistoryEvent` SHALL remain a separate state-change audit contract defined by `OBS-007`; both contracts SHALL use the same identifier, timestamp, actor, and redaction conventions where those fields apply.

#### Scenario: Daemon emits an event
- **WHEN** a daemon subsystem records an operational event
- **THEN** the event has a monotonically ordered profile sequence and a valid source, severity, event name, and timestamp

#### Scenario: Event lacks an optional dimension
- **WHEN** an event does not belong to a project, entrypoint, domain, or request
- **THEN** the corresponding identifier is absent rather than populated with a placeholder

### Requirement: OBS-002: Redaction occurs before persistence or fan-out
Cadder MUST redact forbidden credentials and configured sensitive values before a log, history, diagnostic, or export record reaches durable storage or any subscriber.

#### Scenario: Sensitive structured field
- **WHEN** an event contains an authorization header, cookie, password, token, private key, or configured sensitive field
- **THEN** the stored and streamed event contains a stable redaction marker
- **AND** redaction metadata identifies the affected field without exposing its value

#### Scenario: Sensitive value appears in free text
- **WHEN** a known sensitive value appears in an event message
- **THEN** the value is replaced before storage and subscriber delivery

#### Scenario: Redaction cannot classify input safely
- **WHEN** Cadder cannot determine whether a field allowed by a source contains forbidden data
- **THEN** it omits or redacts the field instead of persisting it verbatim

### Requirement: OBS-003: Log queries use canonical dimensions
The daemon SHALL query logs by cursor or time range and any combination of profile, entrypoint, domain, source, severity, event name, and request identifier with the same semantics for every client.

#### Scenario: Combined filter
- **WHEN** a client requests warning-or-higher Caddy events for one domain after a cursor
- **THEN** the daemon returns only matching events in ascending sequence order

#### Scenario: Multi-domain entrypoint
- **WHEN** one entrypoint serves several domains
- **THEN** a domain filter returns events attributed to that exact domain rather than every event for the entrypoint

#### Scenario: Unknown filter target
- **WHEN** a client names an entrypoint or domain that does not exist
- **THEN** the daemon returns a typed not-found outcome instead of an unfiltered result

#### Scenario: Forgotten entrypoint retains records
- **WHEN** a client filters by a tombstoned registration ID that still has retained logs or history
- **THEN** the daemon returns only records attributed to that historical identity
- **AND** it marks the target as forgotten rather than active

### Requirement: OBS-004: Log tails are ordered and resumable
The daemon SHALL expose a cursor-based tail stream that delivers existing matches followed by new matches without duplicates and lets a client resume after disconnection.

#### Scenario: Tail starts after a cursor
- **WHEN** a client starts a tail with the last sequence it processed
- **THEN** the first returned event has a greater sequence and no later matching event is skipped

#### Scenario: Client reconnects
- **WHEN** a tail connection drops and reconnects with its last processed cursor
- **THEN** the daemon resumes with the next available matching event

#### Scenario: Requested cursor expired
- **WHEN** retention removed events after the requested cursor
- **THEN** the stream starts with a typed gap record that states the first available sequence

### Requirement: OBS-005: Slow subscribers cannot exhaust the daemon
Log streaming MUST use bounded subscriber buffers and explicit gap or termination outcomes instead of unbounded memory growth or silent event loss.

#### Scenario: Subscriber falls behind
- **WHEN** a subscriber cannot consume events within its bounded buffer
- **THEN** the daemon reports the lost sequence range through a gap outcome
- **AND** the subscriber can request a stored replay from the next available cursor

#### Scenario: Subscriber remains stalled
- **WHEN** a subscriber continues to make no progress after the documented deadline
- **THEN** the daemon closes that stream without affecting other clients

### Requirement: OBS-006: Log and history retention have deterministic limits
By default, each profile SHALL retain a `LogEvent` for at most seven days and SHALL retain at most 100,000 log events. It SHALL retain a `HistoryEvent` for at most 90 days and SHALL retain at most 100,000 history events. Trusted configuration MAY select log age from 1 through 365 days, history age from 1 through 3,650 days, and each count limit from 100 through 1,000,000 records. Values outside those closed ranges MUST fail trusted-configuration validation. Maintenance MUST remove the oldest records from each stream when either that stream's age or count limit is exceeded and MUST preserve cursor-gap evidence.

#### Scenario: Count limit is reached first
- **WHEN** a profile stores more than 100,000 events younger than seven days
- **THEN** maintenance removes the oldest excess events

#### Scenario: Age limit is reached first
- **WHEN** an event becomes older than seven days while the count remains below 100,000
- **THEN** maintenance removes that expired event

#### Scenario: User changes retention
- **WHEN** the runtime owner configures supported retention limits in trusted per-user configuration
- **THEN** new cleanup cycles use those limits and report them through diagnostics

#### Scenario: History reaches its count limit
- **WHEN** a profile contains more than 100,000 history events younger than 90 days
- **THEN** maintenance removes the oldest excess history events
- **AND** a query before the retained history cursor receives a typed gap outcome

### Requirement: OBS-007: History records state-changing outcomes
Cadder SHALL maintain a versioned, append-only-within-retention, redacted `HistoryEvent` for every accepted or rejected state-changing request. Each event MUST contain a monotonically ordered history sequence, request ID, actor class, operation, target, timestamp, and outcome. History queries SHALL support an opaque cursor, an inclusive time range, kind, and bounded limit. Log and history cursors MUST remain distinct and MUST NOT be accepted by the other query family.

#### Scenario: Mutation succeeds
- **WHEN** a state-changing request commits
- **THEN** history records the operation and success outcome in the same durable transition

#### Scenario: Mutation is denied
- **WHEN** authorization, compatibility, validation, or precondition checks reject a mutation
- **THEN** history records a typed rejection without executing the handler

#### Scenario: Durable history is unavailable
- **WHEN** the daemon cannot persist the required history event
- **THEN** it rejects new mutations and enters storage-degraded mode
- **AND** it emits a best-effort redacted live diagnostic without claiming that durable history was written

### Requirement: OBS-008: CLI and TUI share observability semantics
CLI and TUI log and history workflows MUST use the same daemon queries, severity ordering, cursor behavior, labels, and redacted event data.

#### Scenario: Equivalent severity filter
- **WHEN** a user selects the same source, target, and severity in CLI and TUI
- **THEN** both surfaces show the same ordered event set and gap state

#### Scenario: TUI is unavailable
- **WHEN** a user cannot or chooses not to use the TUI
- **THEN** CLI commands expose every log, tail, export, history, and recovery detail needed to perform the same workflow

### Requirement: OBS-009: Exports are portable and redacted
`cadder logs export` SHALL produce a versioned, redacted JSON Lines artifact with stable `LogExportRecord` objects and MUST NOT offer an option to export forbidden secrets. Each object SHALL contain `schemaVersion`, a `recordType` discriminator with value `log`, `gap`, or `error`, and exactly the fields defined for that record type. Export records are an artifact contract and MUST NOT use the operator envelope defined by `CLI-006`.

The closed record shapes SHALL be:

| `recordType` | Required fields |
| --- | --- |
| `log` | `schemaVersion: 1`, `recordType: "log"`, and `event` containing one complete `LogEvent`. |
| `gap` | `schemaVersion: 1`, `recordType: "gap"`, nullable `requestedAfter`, nullable `requestedSince`, `firstAvailableCursor`, `firstAvailableTimestamp`, and `reason: "retention"`. |
| `error` | `schemaVersion: 1`, `recordType: "error"`, and `error` containing stable `kind`, `code`, `message`, nullable `guidance`, `retryable`, nullable `requestId`, and `exitCode`. |

#### Scenario: Successful export
- **WHEN** a user exports a filtered log range
- **THEN** stdout or the selected file contains one valid `log` export record per retained `LogEvent` in sequence order

#### Scenario: Export encounters a retention gap
- **WHEN** part of the requested range is no longer retained
- **THEN** the export includes a versioned `gap` record before the first available log record
- **AND** the operator returns exit code `5` without describing the artifact as complete

### Requirement: OBS-010: Event records and query pages are byte-bounded
After redaction, a serialized `LogEvent` MUST NOT exceed 65,536 bytes and a serialized `HistoryEvent` MUST NOT exceed 32,768 bytes. A log message SHALL contain at most 16,384 UTF-8 bytes, and structured log fields SHALL contain at most 64 entries and 32,768 serialized bytes. Cadder MUST truncate strings or omit excess structured fields before persistence and fan-out while preserving identifiers, severity, event name, outcome, and cursor order. Each truncated record SHALL include `truncation` metadata with `applied`, `originalBytes`, and `omittedFieldCount`.

Log and history query responses MUST use the paged collection contract defined by `IPC-011`. A page SHALL contain at most 500 records and 524,288 serialized bytes; it MUST return fewer records with a continuation cursor when the byte limit is reached. CLI and TUI SHALL follow continuation pages until they reach the requested count, terminal cursor, cancellation, or error.

#### Scenario: Log message exceeds its limit
- **WHEN** a redacted log message or structured field set exceeds its record limit
- **THEN** Cadder stores and streams a bounded record with truncation metadata
- **AND** no secret removed by redaction can reappear through truncation metadata

#### Scenario: Query reaches its byte budget first
- **WHEN** matching records reach 524,288 serialized bytes before the requested count
- **THEN** the daemon returns the bounded prefix and a continuation cursor
- **AND** the next page begins after the last returned record without duplication

#### Scenario: One history record is oversized
- **WHEN** a history candidate exceeds 32,768 serialized bytes after redaction
- **THEN** Cadder preserves its request ID, actor, operation, target, timestamp, outcome, and truncation metadata
- **AND** omits or truncates nonessential detail before the durable transition commits
