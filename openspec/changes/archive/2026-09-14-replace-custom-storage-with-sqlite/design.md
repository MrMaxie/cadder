## Context

The retained 1.0 journey needs stable project identity, desired activation across daemon restarts, and recent redacted logs. The current store implements a background worker, admission accounting, JSONL transaction segments, manifests, checksums, hash chains, rotation, replay, recovery copies, and format-specific maintenance for a broader history and export contract that `align-v1-to-core-tui` removes.

The database is private runtime infrastructure used only by the user-owned daemon. The TUI and shim continue through the authenticated local protocol and never open storage. This change is intentionally destructive for the unpublished custom format: the requester selected cleanup without import or backup, with cleanup failure reported but non-blocking.

## Goals / Non-Goals

**Goals:**

- Persist only stable entrypoint identity, public registration IDs, desired entrypoint and domain activation, and bounded redacted logs.
- Replace the custom file engine and worker with one transactional embedded database and one async-safe connection owner.
- Keep owner-only access, bounded operations, crash-safe commits, deterministic count retention, and actionable failure states.
- Delete only known legacy artifacts after the replacement database is proven usable.

**Non-Goals:**

- Preserve or import custom-store history, rejected-operation audit events, tombstones, manifests, recovery evidence, plans, or secrets.
- Add public database access, SQL configuration, an ORM, multiple connections, WAL tuning, exports, paging, subscriptions, or user-configurable retention.
- Recover, replace, or delete a corrupt or newer SQLite database automatically.
- Refactor mutation coordination beyond removing storage-specific worker and admission layers.

## Decisions

### 1. Use focused SQLite crates, not a new storage framework

Add one direct `tokio-rusqlite` dependency with its `bundled` feature through Cargo. That feature forwards to the compatible `rusqlite/bundled` implementation, and the crate re-exports the SQLite API needed by Cadder, so a separate direct `rusqlite` dependency is unnecessary. `tokio-rusqlite` owns one connection on its dedicated blocking thread. Pin the exact resolved version in `Cargo.lock` during implementation.

Wrap calls with one Tokio semaphore permit acquired before `Connection::call`, so at most one closure is active or queued inside `tokio-rusqlite`; other request tasks wait outside its unbounded channel. Capture the connection's thread-safe SQLite interrupt handle during initialization. When a call reaches its deadline or shutdown begins, Cadder signals the interrupt and continues awaiting that same closure until SQLite reports rollback or another terminal result. The permit is released and the caller returns only after that terminal result, preventing a late commit after a timeout response.

Alternatives considered:

- Standard library files would retain custom atomic-write, schema, locking, replay, integrity, and query code, which is the machinery being removed.
- Direct `rusqlite` plus repeated `spawn_blocking` calls would require custom connection ownership and shutdown coordination.
- `sqlx`, an ORM, `redb`, or an RPC-facing database layer would add pooling, mapping, code generation, async runtime, or novel storage concepts that the single-owner local database does not need.

### 2. One private database with a minimal internal schema

The database path is `data/cadder.sqlite3` under the installation runtime. Before SQLite writes content, Cadder creates the data directory and empty database file with the existing owner-only Unix mode or Windows DACL helpers.

New Cadder-generated Admin API or internal-CA private material uses the separate owner-only `data/private-material` directory. The replacement implementation never writes new data to the legacy cleanup names, including `data/secrets`.

Schema version 1 uses `PRAGMA user_version = 1` and exactly these application tables and indexes:

```sql
CREATE TABLE entrypoints (
  entrypoint_key TEXT PRIMARY KEY NOT NULL,
  registration_id TEXT NOT NULL UNIQUE,
  project_root TEXT NOT NULL,
  caddyfile_path TEXT NOT NULL,
  desired_enabled INTEGER NOT NULL CHECK (desired_enabled IN (0, 1))
);

CREATE TABLE domains (
  entrypoint_key TEXT NOT NULL REFERENCES entrypoints(entrypoint_key) ON DELETE CASCADE,
  domain_key TEXT NOT NULL,
  canonical_host TEXT NOT NULL,
  desired_enabled INTEGER NOT NULL CHECK (desired_enabled IN (0, 1)),
  PRIMARY KEY (entrypoint_key, domain_key)
);

CREATE TABLE log_events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  stream_key TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  source TEXT NOT NULL,
  severity TEXT NOT NULL,
  event_name TEXT NOT NULL,
  message TEXT NOT NULL,
  entrypoint_key TEXT,
  domain_key TEXT,
  request_id TEXT,
  fields_json TEXT NOT NULL,
  redaction_json TEXT NOT NULL,
  truncation_json TEXT
);

CREATE INDEX log_events_stream_sequence_idx
  ON log_events (stream_key, sequence);
```

The schema is internal. Wire DTOs and public documentation do not expose table names, SQL, pragmas, or database paths. Version `0` initializes version 1 only when `sqlite_schema` contains no application table; a non-empty or partial version-0 database is left untouched and blocks startup. Version `1` must contain exactly the required application tables, columns, primary and unique keys, domain foreign key, checks, and stream index as verified through focused SQLite metadata queries and rollback-only constraint probes rather than a second schema-fingerprint framework. A higher or otherwise unsupported version fails without mutation. A future schema upgrade increments `user_version` through one explicit transaction.

### 3. Favor one writer and rollback journaling

Use one connection, SQLite's rollback journal mode, `synchronous = FULL`, enabled foreign keys, and a short bounded busy timeout. WAL is not selected because no external reader or concurrent connection exists and its additional sidecar and checkpoint lifecycle would not improve the retained journey. The owner-only `data` directory is the security boundary for SQLite-created rollback journals and temporary sidecars; Cadder securely pre-creates the directory and database, relies on directory access control for sidecars, and does not introduce a custom VFS, file watcher, or post-creation permission race.

Database initialization runs `quick_check` before the daemon publishes readiness. A failed check, open, schema validation, or newer `user_version` leaves the database untouched and aborts startup with a redacted operational error that the TUI launcher or foreground daemon can present.

### 4. State mutation follows apply, commit, or verified rollback

For registration and activation changes, the daemon validates the candidate and applies it to the owned Caddy child first. After the active Caddy hash is verified, one SQLite transaction upserts the affected durable identity and desired activation and inserts the bounded outcome log. Only a successful commit publishes the new in-memory snapshot.

If the SQLite commit fails after Caddy accepted the candidate, Cadder reloads and verifies the previous in-memory Caddy configuration. The request fails and prior in-memory state remains authoritative. Failure to restore Caddy places the runtime in degraded read-only state. A daemon restart never restores project routes from a stored effective configuration; it waits for shims to establish new leases and rebuilds routes from current registrations plus durable desired activation.

Ordinary operational log inserts use their own transaction. An invalid or failed log row preserves current serving state and records an in-memory log-durability diagnostic. It does not latch the whole runtime into restart-only degradation: the next durable mutation performs its own transaction and decides its own storage outcome. Only failure of that desired-state transaction triggers Caddy rollback; corruption or schema failure found at startup still blocks readiness.

### 5. Retention is enforced in the insert transaction

Each log insertion removes the oldest rows exceeding 1,000 for the inserted canonical stream and then removes the oldest rows exceeding 5,000 globally. Queries select one canonical stream and return at most the requested 200 newest rows in ascending sequence order. The small fixed limits make direct indexed deletes and selects sufficient; no custom segments, compactor, sparse index, cursor, or maintenance task remains.

### 6. Legacy cleanup is allowlisted and best-effort

Startup order is: acquire the exclusive runtime endpoint lease, create or open and validate SQLite, commit schema initialization if required, then attempt legacy cleanup before creating or loading new private material. Cleanup targets only these entries under `data`: `manifest.json`, `generations`, `recovery`, `plans`, `secrets`, and `storage.lock`. It never recursively removes `data`, follows a link outside `data`, touches `private-material`, or removes an unknown entry.

There is no import or backup. Removing legacy `data/secrets` deliberately discards pre-1.0 generated key material; later startup creates replacement keys only under `data/private-material`, so existing local HTTPS trust may require the documented explicit trust action again. Every remaining known artifact is retried on the next start. A deletion failure produces a redacted diagnostic naming the artifact class and recovery action, but the healthy SQLite-backed daemon continues. Cleanup success needs no product notification.

## Risks / Trade-offs

- **The first SQLite-backed start permanently discards pre-1.0 custom data** -> Cleanup runs only after SQLite is healthy, targets an explicit allowlist, and is covered by migration tests; the release notes state that no downgrade or import is supported.
- **A database commit can fail after Caddy accepted a change** -> Keep the previous verified Caddy JSON in memory and require verified rollback before reporting the mutation result.
- **`tokio-rusqlite` uses an unbounded internal channel and a dropped future does not cancel its closure** -> Admit one call at a time, interrupt on deadline, and await the same closure's terminal rollback before returning.
- **Bundled SQLite increases binary size** -> Accept the archive-size cost in exchange for a self-contained cross-platform database and removal of substantial custom persistence code.
- **Best-effort cleanup may leave private legacy data** -> Preserve owner-only permissions, show a diagnostic, and retry the exact known artifact on every start.
- **Deleting legacy secrets changes the generated local CA identity** -> Generate replacement material only in `data/private-material` and document that the user may need to repeat the explicit trust step.
- **A corrupt database blocks the daemon** -> Leave it untouched and provide explicit path-independent recovery guidance; automatic deletion would risk destroying the only durable desired state.

## Migration Plan

1. Apply and verify `align-v1-to-core-tui` so no removed history, export, profile, subscription, paging, or tombstone contract remains.
2. Add the SQLite crates through Cargo and implement owner-protected database creation, schema initialization, validation, and bounded connection shutdown.
3. Move durable state mutations and recent-log queries to SQLite, then remove the custom worker, JSONL persistence, and format-specific tests.
4. Add allowlisted legacy cleanup after successful database initialization and cover success, partial failure, links, unknown files, corruption, and newer-version cases.
5. Run the full cross-platform-relevant validation and verify the final dependency and code reduction before marking the change complete.

Rollback is safe only before the first SQLite-backed daemon start. After legacy cleanup begins, downgrade to a custom-store binary is unsupported; restore the previous binary set only with an independently preserved pre-migration runtime directory.

## Open Questions

None.
