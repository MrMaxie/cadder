## 1. Database foundation

- [x] 1.1 Confirm `align-v1-to-core-tui` is applied and no history, profile, tombstone, paging, subscription, tail, or export operation remains in the runtime contract
- [x] 1.2 Add exact `tokio-rusqlite` with its `bundled` feature through Cargo, use its compatible re-exported SQLite API without a second direct dependency, and record the resolution in the lockfile
- [x] 1.3 Create the owner-protected `data/cadder.sqlite3` path before application content is written and preserve the existing Unix and Windows permission boundaries
- [x] 1.4 Implement one single-flight database owner with a pre-call permit, SQLite interrupt handle, rollback journaling, full synchronous commits, foreign keys, busy timeout, integrity validation, and bounded close; never return timeout before the active closure reaches a terminal result
- [x] 1.5 Create the exact schema version 1 tables and stream index in one initialization transaction, initialize version zero only for an empty database, and validate existing version 1 through focused SQLite metadata queries
- [x] 1.6 Test new, current, non-empty version-zero, partial, newer, corrupt, permission-denied, interrupted, late-commit, and shutdown database cases

## 2. Durable state transactions

- [x] 2.1 Load stable entrypoint identity, public registration IDs, desired activation, and domains without restoring leases, sessions, process ownership, or active routes
- [x] 2.2 Persist accepted registration and activation changes in one transaction after verified Caddy apply and publish in-memory state only after commit
- [x] 2.3 Retain the previous verified Caddy JSON in memory and implement verified rollback when persistence fails after apply, including degraded read-only fallback when rollback fails
- [x] 2.4 Remove durable history, tombstone, snapshot, applied-generation, manifest, journal, hash-chain, index, recovery-backup, storage admission, and custom worker paths
- [x] 2.5 Verify restart, reconnect, detach, activation persistence, commit failure, rollback success, rollback failure, and shutdown behavior with fake Caddy and temporary databases

## 3. Bounded redacted logs

- [x] 3.1 Store bounded redacted log fields in the database with a SQLite-assigned monotonic sequence and canonical stream key
- [x] 3.2 Enforce 1,000 rows per stream and 5,000 rows globally in the insertion transaction and return at most 200 newest matching rows in ascending sequence order
- [x] 3.3 Surface ordinary log-write failure as a log-durability diagnostic without stopping current traffic or latching unrelated mutations; let each later desired-state transaction determine its own storage outcome
- [x] 3.4 Cover redaction, truncation, ordering, stream isolation, both retention limits, response byte bounds, write timeout, and restart persistence with focused tests

## 4. Pre-1.0 cleanup

- [x] 4.1 After exclusive lease acquisition and successful database validation, delete only `manifest.json`, `generations`, `recovery`, `plans`, `secrets`, and `storage.lock` beneath `data`
- [x] 4.2 Reject cleanup traversal through links, preserve unknown entries, and never delete the `data` directory, SQLite database, new `private-material`, or files outside the allowlist
- [x] 4.3 Continue startup with a redacted diagnostic when cleanup partially fails and retry every remaining known artifact on the next start
- [x] 4.4 Test complete cleanup, partial failure, retry, unknown files, link boundaries, absent artifacts, legacy-secret regeneration guidance, and the guarantee that no cleanup runs before a healthy database exists

## 5. Verification and reduction

- [x] 5.1 Remove storage dependencies that become unused and review the final graph to confirm no ORM, SQL pool, alternate embedded store, or second custom queue was introduced
- [x] 5.2 Run focused storage and state tests, formatting, workspace static checks, workspace tests, coverage at the repository threshold, and strict OpenSpec validation without starting real Caddy
- [x] 5.3 Verify portable builds use bundled SQLite on Windows x64, Linux x64, macOS x64, and macOS arm64 and require no system SQLite library
- [x] 5.4 Review the final diff for net removal of custom storage concepts, correct owner permissions, destructive-cleanup disclosure, private-path leakage, and accidental mutation-actor or repository-tooling scope
