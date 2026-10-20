# Runtime storage

## Purpose
Define the single SQLite durability boundary, its ownership rules, and the guarantees required before the daemon accepts work.

## Requirements

### Requirement: STO-001: One owner-protected SQLite database is authoritative
The Node daemon SHALL own a protected v2 application-state and diagnostic-log database using node:sqlite, separate from the lifetime exclusion database. No client SHALL directly access either database. Initial schema creation SHALL be transactional and finish before product operations are accepted.

#### Scenario: Empty v2 application database
- **WHEN** the Node daemon first creates application-state and diagnostic-log storage
- **THEN** it SHALL create and validate its schema under owner-only protection
- **AND** the separate SQLite exclusion connection SHALL remain held

### Requirement: STO-002: Database work is single flight and bounded
One worker SHALL serialize application-state and diagnostic-log database work, with bounded jobs, transaction rollback, integrity checks and orderly close. Worker failures SHALL settle pending requests with typed errors and SHALL NOT release daemon exclusion while owned work/resources remain active.

#### Scenario: Worker or transaction fails
- **WHEN** persistence cannot finish successfully
- **THEN** uncommitted data SHALL be rolled back and pending callers informed
- **AND** shutdown SHALL settle worker work before releasing runtime exclusion

### Requirement: STO-003: The Node schema is validated before mutation
Startup SHALL initialize only an empty uninitialized Node database, and SHALL reject partial, corrupt or unsupported/newer schemas before runtime mutation. It SHALL NOT delete unrelated entries to recover from schema failure.

#### Scenario: Partial schema exists
- **WHEN** database objects or integrity do not match the supported Node schema
- **THEN** startup SHALL fail with a storage diagnostic without destructive repair

### Requirement: STO-004: Old runtime data remains untouched
The Node migration SHALL NOT import, delete or implicitly clean up the Rust runtime or database. Projects SHALL re-register from existing configuration. Old releases and data SHALL remain available for rollback; stable intent SHALL NOT resurrect a live session. Durable logs and state SHALL NOT introduce a history command or view.

#### Scenario: Node first starts beside old data
- **WHEN** old Rust runtime data exists during Node startup
- **THEN** Node SHALL use its separate v2 storage and leave the old data untouched
- **AND** project routes SHALL become live only through current registration
