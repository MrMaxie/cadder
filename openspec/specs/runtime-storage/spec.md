# Runtime storage

## Purpose
Define the single SQLite durability boundary, its ownership rules, and the guarantees required before the daemon accepts work.

## Requirements

### Requirement: STO-001: One owner-protected SQLite database is authoritative
Cadder MUST store application state only in `data/cadder.sqlite3` beneath the installation runtime using the bundled SQLite engine and owner-only filesystem permissions.

#### Scenario: Database is created
- **WHEN** the daemon initializes an empty runtime
- **THEN** it creates schema version 1 in one transaction before accepting operations

### Requirement: STO-002: Database work is single flight and bounded
One `tokio-rusqlite` connection SHALL serialize database closures behind a pre-call permit with an interrupt handle, rollback journal, full synchronous commits, foreign keys, busy timeout, integrity validation, and bounded close.

#### Scenario: Operation deadline expires
- **WHEN** a database closure exceeds its deadline
- **THEN** Cadder interrupts it and waits for a terminal result before releasing the permit

### Requirement: STO-003: Schema compatibility is exact before 1.0
Cadder MUST initialize version zero only when the database is otherwise empty and MUST reject partial, corrupt, or newer schemas before cleanup or runtime mutation.

#### Scenario: Partial database exists
- **WHEN** required tables or indexes do not match schema version 1
- **THEN** startup fails with a storage diagnostic without deleting unrelated data

### Requirement: STO-004: Legacy cleanup is allowlisted
After endpoint ownership and healthy database validation, Cadder MAY delete only the named pre-1.0 storage artifacts beneath `data`, MUST refuse link traversal, and MUST preserve unknown entries and the database.

#### Scenario: Cleanup partially fails
- **WHEN** one allowlisted artifact cannot be removed
- **THEN** startup continues with a redacted diagnostic and retries that artifact on the next start
