## Why

Cadder 1.0 uses transparent, owner-readable runtime files instead of an embedded database. This keeps recovery, diagnostics, backup, and operator inspection proportional to a per-user local coordinator while preserving strict durability and bounded access guarantees.

## What Changes

- **BREAKING**: Replace the SQLite runtime database and WAL with a versioned file layout.
- Store authoritative state and small immutable plans as schema-versioned JSON documents published through atomic replacement.
- Store operational logs and state-change history as redacted, append-only JSONL segments with explicit cursors, bounded segment sizes, retention manifests, and crash-tail recovery.
- Keep trusted user-authored configuration in TOML; use YAML only for human-authored project inputs where the owning capability explicitly defines YAML. Durable daemon state never depends on YAML parsing or formatting.
- Define a transaction journal that makes state plus required history outcomes recoverable as one durable transition without pretending that unrelated files share filesystem transactions.
- Preserve corrupt or unsupported files as owner-protected diagnostic backups and rebuild only derived indexes.
- Remove `rusqlite`, bundled SQLite, `runtime.sqlite3`, WAL checkpoints, and database-specific diagnostics from the 1.0 implementation and documentation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `runtime-storage`: Replace the transactional SQLite contract with a versioned, atomic, owner-protected file store and recoverable transaction journal.
- `observability`: Define JSONL segment, cursor, indexing, rotation, retention, and crash-recovery behavior for durable logs and history.

## Impact

- Replaces `crates/cadder-daemon/src/storage.rs`, its `rusqlite` dependency, and `RuntimePaths::storage_path` with file-store modules and typed paths.
- Changes storage health metadata and wire fixtures that currently name `sqlite`.
- Updates architecture documentation, recovery diagnostics, migration tests, retention tests, and release artifacts.
- Does not change CLI/TUI query semantics, protocol event DTOs, retention limits, redaction rules, or daemon ownership boundaries.
