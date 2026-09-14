## MODIFIED Requirements

### Requirement: OBS-001: Runtime logs use one structured schema
The daemon SHALL represent every retained operational log as a bounded, versioned `LogEvent` with sequence, timestamp, source, severity, event name, message, optional entrypoint, domain and request identifiers, stream identity, and redaction metadata. Cadder 1.0 SHALL NOT define a separate durable history-event contract.

#### Scenario: Daemon emits an event
- **WHEN** a daemon subsystem records an operational event
- **THEN** the event has a monotonically ordered runtime sequence and valid source, severity, event name, timestamp, and stream identity

#### Scenario: Event lacks an optional dimension
- **WHEN** an event does not belong to a project, entrypoint, domain, or request
- **THEN** the corresponding identifier is absent rather than populated with a placeholder

### Requirement: OBS-002: Redaction occurs before persistence or fan-out
Cadder MUST redact forbidden credentials and configured sensitive values before a log or diagnostic record reaches durable storage or a client response.

#### Scenario: Sensitive structured field
- **WHEN** an event contains an authorization header, cookie, password, token, private key, or configured sensitive field
- **THEN** the retained and returned event contains a stable redaction marker instead of the value

#### Scenario: Redaction cannot classify input safely
- **WHEN** Cadder cannot determine whether an allowed source field contains forbidden data
- **THEN** it omits or redacts that field before retention

### Requirement: OBS-003: Log queries use canonical dimensions
The daemon SHALL accept one bounded recent-log query for a runtime, entrypoint, or domain stream and return events in stable ascending sequence order. The request limit MUST be between 1 and 200. Cadder 1.0 SHALL NOT expose time ranges, cursors, configurable filters, historical identities, or multi-page queries.

#### Scenario: TUI requests recent logs
- **WHEN** the TUI selects a visible runtime, entrypoint, or domain stream
- **THEN** the daemon returns at most the requested number of newest retained events for that exact stream in ascending order

#### Scenario: Unknown stream is requested
- **WHEN** the TUI requests a stream that does not belong to current authoritative state
- **THEN** the daemon returns a typed not-found outcome rather than unfiltered logs

### Requirement: OBS-006: Log and history retention have deterministic limits
The installation runtime SHALL retain at most 5,000 log events globally and at most 1,000 events for one canonical stream. When either count is exceeded, the oldest matching events SHALL be removed. Retention limits SHALL NOT be user-configurable in Cadder 1.0.

#### Scenario: Global limit is reached
- **WHEN** inserting an event would retain more than 5,000 events
- **THEN** Cadder removes the oldest global excess before exposing the committed result

#### Scenario: Stream limit is reached
- **WHEN** inserting an event would retain more than 1,000 events for its stream
- **THEN** Cadder removes the oldest excess for that stream

### Requirement: OBS-010: Event records and query pages are byte-bounded
A serialized retained `LogEvent` MUST NOT exceed 65,536 bytes. A log message SHALL contain at most 16,384 UTF-8 bytes, and structured fields SHALL contain at most 64 entries and 32,768 serialized bytes. Cadder MUST truncate or omit excess nonessential data after redaction while preserving sequence, timestamp, source, severity, event name, stream identity, and truncation metadata.

One recent-log response SHALL contain at most 200 events and remain below the control-plane frame limit. It SHALL be terminal and MUST NOT include a continuation cursor or page token.

#### Scenario: Log message exceeds its limit
- **WHEN** a redacted log candidate exceeds its record limit
- **THEN** Cadder retains a bounded event with truncation metadata and no reintroduced secret value

#### Scenario: Response reaches its byte limit
- **WHEN** fewer than 200 events fill the response byte budget
- **THEN** the daemon returns the newest bounded subset and identifies the result as truncated

## REMOVED Requirements

### Requirement: OBS-004: Log tails are ordered and resumable
**Reason**: The retained TUI uses bounded recent-log refreshes and does not expose follow or resume semantics.

**Migration**: Refresh the selected Logs view to request the newest bounded entries.

### Requirement: OBS-005: Slow subscribers cannot exhaust the daemon
**Reason**: Cadder 1.0 has no log or state subscription protocol.

**Migration**: Clients issue independent bounded requests.

### Requirement: OBS-007: History records state-changing outcomes
**Reason**: No retained product journey reads history, and mandatory audit persistence currently forces mutations to depend on an unused custom subsystem.

**Migration**: Current operation outcomes remain visible in authoritative state, errors, and bounded diagnostic logs.

### Requirement: OBS-008: CLI and TUI share observability semantics
**Reason**: Cadder 1.0 has no operator CLI log or history workflow.

**Migration**: Use the TUI Logs view.

### Requirement: OBS-009: Exports are portable and redacted
**Reason**: Cadder 1.0 has no export command or artifact audience.

**Migration**: Use bounded on-demand diagnostic detail; a future support-bundle journey requires a separate allowlisted export contract.

## RENAMED Requirements

- FROM: `### Requirement: OBS-003: Log queries use canonical dimensions`
- TO: `### Requirement: OBS-003: Log queries use canonical streams`
- FROM: `### Requirement: OBS-006: Log and history retention have deterministic limits`
- TO: `### Requirement: OBS-006: Recent log retention has deterministic limits`
- FROM: `### Requirement: OBS-010: Event records and query pages are byte-bounded`
- TO: `### Requirement: OBS-010: Log records and bounded responses are byte-bounded`
