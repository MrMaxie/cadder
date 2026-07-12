# runtime-storage Specification

## Purpose
Define owner-protected durable runtime state, transactional SQLite behavior, migrations, integrity checks, backups, corruption recovery, and maintenance.

## Requirements
### Requirement: STO-001: Runtime state is durable per profile
Each runtime profile SHALL persist stable entrypoint keys, their opaque public registration IDs or query tombstones, desired entrypoint and domain activation state, applied-state metadata, history, and structured logs in one owner-protected SQLite database. Connection leases and daemon-instance ownership MUST remain ephemeral.

#### Scenario: Daemon restart
- **WHEN** a healthy daemon restarts
- **THEN** it restores durable desired state and marks previously live entrypoints as reconnecting before accepting mutations
- **AND** it does not treat a persisted record as a valid lease for the new daemon instance

#### Scenario: Profile isolation
- **WHEN** two profiles use the same project or domain names
- **THEN** each database retains only the state and history owned by its profile

### Requirement: STO-002: Storage schema is versioned and migrated transactionally
The database MUST carry an explicit schema version, and every supported upgrade SHALL apply ordered migrations in one transaction before normal runtime work begins.

#### Scenario: Upgrade with pending migrations
- **WHEN** the daemon opens a healthy database from a supported earlier version
- **THEN** it applies each required migration in order
- **AND** it commits the new schema version only after every migration succeeds

#### Scenario: Migration failure
- **WHEN** any migration fails
- **THEN** the transaction rolls back
- **AND** the daemon enters a typed storage-degraded state without accepting mutations

#### Scenario: Database from a newer Cadder version
- **WHEN** the database schema version is newer than the running daemon supports
- **THEN** the daemon refuses to modify it and reports an incompatible-storage error

### Requirement: STO-003: State transitions are atomic
Every accepted state-changing operation MUST commit its durable state and history outcome atomically or leave the prior durable state unchanged.

#### Scenario: Successful registration change
- **WHEN** the daemon accepts and applies a registration change
- **THEN** the new registration state and its success history event become visible in the same committed transaction

#### Scenario: Operation fails before commit
- **WHEN** validation or external application fails before an operation commits
- **THEN** the prior desired and applied state remains authoritative
- **AND** a failure event records the attempted operation without claiming success

### Requirement: STO-004: Corruption recovery preserves evidence
Cadder MUST preserve an unreadable or integrity-failing database as an owner-protected diagnostic backup before creating replacement storage, and MUST NOT silently delete or overwrite it.

#### Scenario: Integrity check fails at startup
- **WHEN** the daemon detects database corruption
- **THEN** it moves or copies the database and related recovery files to a uniquely named diagnostic backup
- **AND** it reports the backup location through redacted diagnostics
- **AND** it starts replacement storage only after the backup succeeds

#### Scenario: Backup cannot be created
- **WHEN** corruption is detected but the diagnostic backup cannot be written
- **THEN** the daemon remains storage-degraded and leaves the original files untouched

### Requirement: STO-005: Storage files are private
The daemon SHALL create databases, journals, backups, and migration artifacts with access restricted to the runtime owner and required operating-system service principals.

#### Scenario: New storage on Unix
- **WHEN** the daemon creates storage on Linux or macOS
- **THEN** the runtime directory is owner-only and storage files are not readable by group or other users

#### Scenario: New storage on Windows
- **WHEN** the daemon creates storage on Windows
- **THEN** its DACL grants access only to the owning user and operating-system principals required for file management

### Requirement: STO-006: Stored data excludes forbidden secrets
The runtime database MUST contain only fields allowed by the accepted state and observability schemas, after redaction, and SHALL reject or remove project credentials and user-supplied secrets before persistence. Cadder-generated Admin API mTLS keys and internal-CA private keys SHALL live only in dedicated owner-protected secret files outside SQLite, logs, history, diagnostics, and exports. Those files MUST use the access policy defined by `STO-005` and MUST NOT be exposed through general storage queries.

#### Scenario: Request contains a secret header
- **WHEN** an operation includes an authorization token, cookie, private key, password, or configured sensitive field
- **THEN** the persisted state, history, and logs contain a redaction marker instead of the secret value

#### Scenario: Export durable diagnostics
- **WHEN** a user exports storage-backed diagnostics
- **THEN** the export applies the same forbidden-data policy as persistence

#### Scenario: Cadder generates private runtime material
- **WHEN** Cadder creates an Admin API mTLS key or internal-CA private key
- **THEN** it writes the key only to the dedicated owner-protected secret location
- **AND** the database, logs, history, diagnostics, and exports contain only non-secret identity or status metadata

### Requirement: STO-007: Maintenance is bounded and observable
Database checkpoints, retention cleanup, and integrity checks MUST run with bounded work and expose their outcome without blocking control-plane availability indefinitely.

#### Scenario: Retention cleanup has extensive work
- **WHEN** more expired records exist than one maintenance budget permits
- **THEN** the daemon removes a bounded batch and schedules further cleanup
- **AND** normal read operations remain available

#### Scenario: Maintenance fails
- **WHEN** a checkpoint, cleanup, or integrity check fails
- **THEN** the daemon records a typed diagnostic event and applies the storage degradation policy appropriate to the failure
