## ADDED Requirements

### Requirement: STO-008: Known pre-1.0 storage is removed after replacement storage is ready
After acquiring the exclusive runtime lease and successfully opening, validating, and initializing replacement storage, the daemon SHALL attempt to remove only the known pre-1.0 entries `manifest.json`, `generations`, `recovery`, `plans`, `secrets`, and `storage.lock` beneath the runtime `data` directory. It MUST NOT import or back up their contents, remove the complete `data` directory, follow a link outside that directory, or remove an unknown entry.

A cleanup failure SHALL produce a redacted diagnostic and SHALL NOT prevent a healthy replacement store from serving the runtime. The daemon SHALL retry each remaining known entry on a later start.

#### Scenario: Legacy artifacts are present
- **WHEN** replacement storage is healthy and known pre-1.0 artifacts remain
- **THEN** the daemon removes only those allowlisted entries without importing or backing them up

#### Scenario: Unknown file is present
- **WHEN** the `data` directory contains an entry outside the cleanup allowlist
- **THEN** the daemon leaves it unchanged

#### Scenario: Cleanup partly fails
- **WHEN** one known artifact cannot be removed
- **THEN** the daemon reports a redacted warning, continues with healthy replacement storage, and retries that artifact on the next start

#### Scenario: Legacy secret material is removed
- **WHEN** cleanup removes the pre-1.0 `secrets` directory
- **THEN** later private material is generated only under `data/private-material`
- **AND** recovery guidance states that local HTTPS trust may need to be established again

#### Scenario: Replacement storage is unavailable
- **WHEN** the replacement database cannot open, validate, or initialize
- **THEN** the daemon removes no legacy artifact

## MODIFIED Requirements

### Requirement: STO-002: Storage schema is versioned and migrated transactionally
The authoritative database SHALL carry one explicit integer schema version. Opening an empty database SHALL create the complete current schema and publish its version in one transaction. A supported future upgrade MUST apply every ordered schema step in one transaction or leave the prior schema and rows unchanged. A database with a newer or unsupported version MUST NOT be modified.

#### Scenario: Empty database opens
- **WHEN** the daemon opens a new owner-protected database
- **THEN** it creates all required tables, indexes, constraints, and the current schema version atomically

#### Scenario: Non-empty version-zero database opens
- **WHEN** a database reports schema version zero but already contains an application table
- **THEN** startup fails without creating, dropping, or altering any table

#### Scenario: Schema initialization fails
- **WHEN** any schema statement or version publication fails
- **THEN** startup fails and no partial current schema is accepted

#### Scenario: Newer schema opens
- **WHEN** the database schema version is newer than the running daemon supports
- **THEN** startup fails without changing or deleting the database

### Requirement: STO-003: State transitions are atomic
Every accepted registration or activation mutation MUST publish its durable identity and desired-state rows in one database transaction after the candidate Caddy configuration is applied and verified. In-memory authoritative state SHALL change only after the database commit succeeds.

If the durable commit fails after Caddy accepted the candidate, the daemon MUST restore and verify the previous in-memory Caddy configuration before returning failure. A failed restoration SHALL enter degraded read-only state. Rejected operations SHALL NOT require a durable history record.

#### Scenario: Successful activation change
- **WHEN** Caddy applies the candidate and the durable transaction commits
- **THEN** the desired activation and authoritative in-memory snapshot become visible together

#### Scenario: Durable commit fails after apply
- **WHEN** the database transaction fails after Caddy accepted a candidate
- **THEN** Cadder restores the previous verified Caddy configuration and reports the mutation as failed

#### Scenario: Daemon crashes during a database transaction
- **WHEN** the process exits before a transaction commits
- **THEN** the database exposes either the complete prior state or the complete committed replacement, never partial rows

### Requirement: STO-004: Corruption recovery preserves evidence
At startup the daemon MUST validate the database before publishing readiness. If open, schema validation, or integrity checking reports corruption or inconsistency, Cadder SHALL leave the database untouched, fail startup, and report a redacted recovery diagnostic. It MUST NOT delete, rename, overwrite, repair, or automatically replace the database.

#### Scenario: Integrity check fails
- **WHEN** startup validation reports a corrupt database
- **THEN** the daemon does not publish readiness or start its Caddy child
- **AND** the original database remains unchanged

#### Scenario: Operator retries without repair
- **WHEN** the same invalid database remains on a later start
- **THEN** Cadder fails consistently without creating a replacement store beside it

### Requirement: STO-005: Storage files are private
The daemon SHALL create the runtime data directory, authoritative database, and dedicated `data/private-material` directory with access restricted to the runtime owner and operating-system principals required for file management before application data is written. SQLite-created rollback journals and sidecars MUST remain inside that owner-only directory and rely on its access boundary. The replacement implementation MUST NOT add a custom SQLite VFS or write new material under the legacy `data/secrets` cleanup path. The database MUST NOT be a supported external integration or shared multi-process store.

#### Scenario: New database on Unix
- **WHEN** Cadder creates storage on Linux or macOS
- **THEN** the data directory is owner-only and the database is not readable by group or other users

#### Scenario: New database on Windows
- **WHEN** Cadder creates storage on Windows
- **THEN** its DACL grants access only to the owning user and required operating-system principals

#### Scenario: Another process opens the database
- **WHEN** a tool outside the owning daemon attempts to use the private database as an integration surface
- **THEN** Cadder provides no compatibility guarantee for that access

### Requirement: STO-006: Stored data excludes forbidden secrets
Durable identity, desired activation, and log rows MUST contain only fields allowed by their accepted schemas after redaction. Cadder SHALL reject or remove project credentials and user-supplied secrets before a transaction. Cadder-generated Admin API or internal-CA private keys MUST remain in `data/private-material` outside the general database and MUST NOT appear in logs, diagnostics, or client responses.

#### Scenario: Request contains a secret field
- **WHEN** a candidate contains an authorization token, cookie, private key, password, or configured sensitive value
- **THEN** durable state and logs contain an allowed redaction marker instead of the value

#### Scenario: Cadder generates private runtime material
- **WHEN** Cadder creates an Admin API or internal-CA private key
- **THEN** it stores that key under owner-only `data/private-material` and never under the legacy cleanup path

### Requirement: STO-007: Maintenance is bounded and observable
Schema validation, integrity checking, durable calls, retention deletes, and database shutdown SHALL use explicit time and work bounds. At most one database closure may be active or queued inside the connection owner. If an active operation reaches its deadline, Cadder MUST interrupt SQLite and await that closure's terminal rollback or completion before releasing admission or returning; a timed-out operation MUST NOT commit later. Log retention MUST be enforced in the same transaction as each log insert. Cadder 1.0 SHALL NOT run a custom compactor, segment rotator, index rebuilder, backup worker, or maintenance scheduler.

#### Scenario: Log insertion exceeds retention
- **WHEN** a log insert crosses its per-stream or global count limit
- **THEN** the same transaction removes the oldest excess rows before committing

#### Scenario: Database call exceeds its deadline
- **WHEN** a database operation does not finish within its bounded deadline
- **THEN** Cadder interrupts it, waits for rollback or another terminal result, and only then reports the outcome
- **AND** the operation cannot publish a late commit after the caller observes timeout

#### Scenario: Shutdown reaches storage
- **WHEN** daemon shutdown begins with database work in flight
- **THEN** Cadder stops admitting new storage calls and makes one bounded connection-close attempt within the shutdown timeline
