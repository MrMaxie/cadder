## Why

After `align-v1-to-core-tui` removes history, exports, streams, profiles, and paging, Cadder still carries a custom file-store worker and persistence modules (`crates/cadder-daemon/src/storage.rs` and `crates/cadder-daemon/src/storage/persistence.rs`) implementing manifests, JSONL segments, checksums, hash chains, rotation, recovery copies, and format-specific failure states required by the current `runtime-storage` and `observability` specs. Replacing that machinery with one embedded transactional database preserves the durable activation and recent-log journey while deleting concepts that users never interact with.

This change MUST be applied after `align-v1-to-core-tui`; it must not reintroduce any removed product surface.

## What Changes

- Replace the custom segmented file store and storage worker with one owner-protected `data/cadder.sqlite3` database accessed through focused SQLite crates.
- Persist only stable entrypoint identity, public registration ID, desired entrypoint and domain activation, and bounded redacted logs. Keep leases, sessions, endpoints, daemon identity, and processes ephemeral.
- Use transactional schema versioning and state mutation without public history, tombstones, snapshots, journals, manifests, hash chains, segments, indexes, exports, or custom recovery generations.
- **BREAKING** Do not import or back up the pre-1.0 custom store. After SQLite opens successfully under the exclusive runtime lease, remove only the known legacy manifest, generations, recovery, plans, secrets, and storage-lock artifacts.
- Continue startup when a known legacy artifact cannot be removed, expose a redacted warning, and retry on the next start. Leave unknown files untouched.
- Refuse to modify or automatically delete a corrupt SQLite database or one with a newer schema; fail startup with an actionable diagnostic.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `runtime-storage`: Replace the format-specific custom storage, migration, integrity, privacy, and maintenance contract with one minimal transactional database contract.
- `observability`: Remove the JSONL-segment requirement while retaining bounded, redacted, durable recent logs.
- `caddy-runtime`: Replace durable configuration-generation publication with verified in-memory rollback plus transactional desired-state persistence.

## Success Criteria

- One owner-protected `data/cadder.sqlite3` stores only stable entrypoints, desired domain activation, and bounded redacted logs through one single-flight connection owner.
- A timed-out or interrupted database operation reaches rollback or another terminal result before its caller returns and cannot commit later.
- Restart restores desired activation but no lease, session, process, or project route until a current shim registers.
- Per-stream and global log retention remain at 1,000 and 5,000 rows, and one TUI query returns at most 200 recent events.
- The custom worker, admission queue, JSONL, manifest, segment, checksum, hash-chain, recovery-backup, and format-specific maintenance concepts are removed, and portable builds require no system SQLite.
- Cleanup runs only after a healthy database exists, touches only the allowlist, preserves unknown and new-format data, and reports partial failure without blocking startup.

## Impact

Future implementation adds `tokio-rusqlite` with its forwarded bundled-`rusqlite` feature and removes the custom worker, admission queue, manifests, JSONL replay, segment rotation, checksums, hash chains, recovery backups, sparse-index concepts, and their tests. The SQLite schema is internal and versioned but not a public API. Failure and migration tests must cover Windows, Linux, and macOS ownership behavior without launching real Caddy.
