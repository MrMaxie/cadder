## MODIFIED Requirements

### Requirement: STO-001: Runtime state is durable per profile
Each runtime profile SHALL persist stable entrypoint keys, their opaque public registration IDs or query tombstones, desired entrypoint and domain activation state, applied-state metadata, history, and structured logs in an owner-protected file store rooted in the platform's durable per-user local-data directory. Versioned JSON snapshots SHALL checkpoint state, a versioned JSON Lines transaction stream SHALL contain state changes and history, and a separate versioned JSON Lines stream SHALL contain operational logs. Sockets, discovery, process locks, connection leases, and daemon-instance ownership MUST remain ephemeral and MUST NOT be treated as durable state.

#### Scenario: Daemon restart
- **WHEN** a healthy daemon restarts
- **THEN** it restores durable desired state and marks previously live entrypoints as reconnecting before accepting mutations
- **AND** it does not treat a persisted record as a valid lease for the new daemon instance

#### Scenario: Profile isolation
- **WHEN** two profiles use the same project or domain names
- **THEN** each profile file set retains only the state, logs, and history owned by that profile

### Requirement: STO-002: Storage schema is versioned and migrated transactionally
Every authoritative JSON document, JSON Lines record, manifest, journal, and index MUST carry or inherit an explicit schema version. A supported layout upgrade SHALL build and validate a complete replacement generation and atomically select it through the manifest only after every ordered migration succeeds. Event-stream migrations SHALL preserve immutable source segments until the replacement manifest commits.

#### Scenario: Upgrade with pending migrations
- **WHEN** the daemon opens a healthy file set from a supported earlier version
- **THEN** it applies each required migration in order to owner-protected candidates
- **AND** it publishes the new schema version only after every migrated document and stream validates

#### Scenario: Migration failure
- **WHEN** any migration fails
- **THEN** the prior authoritative files remain unchanged
- **AND** the daemon enters a typed storage-degraded state without accepting mutations

#### Scenario: Storage from a newer Cadder version
- **WHEN** an authoritative file schema is newer than the running daemon supports
- **THEN** the daemon refuses to modify the file set and reports an incompatible-storage error

### Requirement: STO-003: State transitions are atomic
Every accepted or rejected state-changing operation MUST commit as one bounded, checksummed, hash-chained JSON Lines transaction record containing a nullable typed state delta and the complete required history outcome. The daemon SHALL change in-memory state only after the complete LF-terminated record is durably flushed. A snapshot is a checkpoint and MUST NOT make an uncommitted state change authoritative.

#### Scenario: Successful registration change
- **WHEN** the daemon accepts and applies a registration change
- **THEN** the new registration state and its success history event become visible at the same committed storage generation

#### Scenario: Operation fails before commit
- **WHEN** validation or external application fails before an operation commits
- **THEN** the prior desired and applied state remains authoritative
- **AND** a failure event records the attempted operation without claiming success

#### Scenario: Crash interrupts a durable transition
- **WHEN** the daemon restarts with an incomplete final transaction line
- **THEN** it preserves a bounded diagnostic and discards only bytes after the last valid LF
- **AND** it restores state and history by replaying complete committed records without exposing the incomplete candidate

#### Scenario: Snapshot lags committed transactions
- **WHEN** a daemon restarts after transactions commit but before the next snapshot publishes
- **THEN** it restores the selected snapshot and replays every later valid transaction in sequence
- **AND** the restored state and history equal the state visible before the restart

### Requirement: STO-004: Corruption recovery preserves evidence
Cadder MUST preserve an unreadable, inconsistent, or integrity-failing authoritative file set as an owner-protected diagnostic backup before creating replacement storage, and MUST NOT silently delete, skip, or overwrite invalid evidence. Derived indexes MAY be rebuilt without backing them up when all authoritative documents and event records validate.

#### Scenario: Integrity check fails at startup
- **WHEN** the daemon detects corruption in authoritative state, a transaction journal, manifest, or event segment
- **THEN** it moves or copies the affected generation and related recovery files to a uniquely named diagnostic backup
- **AND** it reports the backup location through redacted diagnostics
- **AND** it starts replacement storage only after the backup succeeds

#### Scenario: Backup cannot be created
- **WHEN** corruption is detected but the diagnostic backup cannot be written
- **THEN** the daemon remains storage-degraded and leaves the original files untouched

#### Scenario: Derived index is invalid
- **WHEN** an event index is missing, incompatible, or corrupt while its authoritative segments validate
- **THEN** the daemon rebuilds the index without discarding or rewriting the event records

### Requirement: STO-005: Storage files are private
The daemon SHALL create state documents, transaction journals, event segments, manifests, indexes, plans, backups, migration artifacts, and secret files with access restricted to the runtime owner and required operating-system service principals. Temporary candidates MUST receive that protection before content is written.

#### Scenario: New storage on Unix
- **WHEN** the daemon creates storage on Linux or macOS
- **THEN** the durable profile directory is owner-only and storage files are not readable by group or other users

#### Scenario: New storage on Windows
- **WHEN** the daemon creates storage on Windows
- **THEN** its DACL grants access only to the owning user and operating-system principals required for file management

### Requirement: STO-006: Stored data excludes forbidden secrets
Durable state documents, plans, journals, manifests, indexes, history, and logs MUST contain only fields allowed by the accepted state and observability schemas after redaction, and SHALL reject or remove project credentials and user-supplied secrets before persistence. Cadder-generated Admin API mTLS keys and internal-CA private keys SHALL live only in dedicated owner-protected secret files outside general state, logs, history, diagnostics, and exports. Those files MUST use the access policy defined by `STO-005` and MUST NOT be exposed through general storage queries.

#### Scenario: Request contains a secret header
- **WHEN** an operation includes an authorization token, cookie, private key, password, or configured sensitive field
- **THEN** the persisted state, history, and logs contain a redaction marker instead of the secret value

#### Scenario: Export durable diagnostics
- **WHEN** a user exports storage-backed diagnostics
- **THEN** the export applies the same forbidden-data policy as persistence

#### Scenario: Cadder generates private runtime material
- **WHEN** Cadder creates an Admin API mTLS key or internal-CA private key
- **THEN** it writes the key only to the dedicated owner-protected secret location
- **AND** state, plans, journals, manifests, indexes, logs, history, diagnostics, and exports contain only non-secret identity or status metadata

### Requirement: STO-007: Maintenance is bounded and observable
Event rotation, retention compaction, index rebuilding, migration, backup, and integrity checks MUST run with explicit record, byte, and time budgets and expose their outcomes without blocking control-plane availability indefinitely. Maintenance MUST persist manifest changes before removing superseded authoritative files.

#### Scenario: Retention cleanup has extensive work
- **WHEN** more expired records exist than one maintenance budget permits
- **THEN** the daemon removes a bounded set of whole segments or compacts one boundary segment and schedules further cleanup
- **AND** normal read operations remain available

#### Scenario: Maintenance fails
- **WHEN** rotation, compaction, index rebuilding, migration, backup, or integrity checking fails
- **THEN** the daemon records a typed diagnostic event and applies the storage degradation policy appropriate to the authoritative or derived failure
