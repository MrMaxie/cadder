## Context

The accepted 1.0 contract currently places durable state, history, and structured logs in one SQLite database. Cadder is a single-writer, per-user local daemon whose clients read through IPC. It does not need concurrent SQL writers, ad hoc relational access, or a public database surface. The runtime still needs atomic mutations, ordered cursors, bounded queries, crash recovery, migrations, retention, private permissions, and preserved corruption evidence on Windows, Linux, and macOS.

## Goals / Non-Goals

**Goals:**

- Make durable runtime data transparent, versioned, owner-protected, and recoverable with ordinary files.
- Preserve the existing atomic state-plus-history contract and deterministic log/history cursors.
- Keep crash recovery bounded and make derived data safe to rebuild.
- Use the same durable algorithms on all supported platforms behind narrow atomic-replace, flush, permission, and lock abstractions.
- Remove SQLite and its native build surface from the product.

**Non-Goals:**

- Expose files as a supported client API; CLI, TUI, and future clients still use IPC contracts.
- Support concurrent writers, remote filesystems, shared profiles, arbitrary user edits, SQL queries, or third-party storage plugins.
- Import pre-1.0 SQLite content into the 1.0 store.
- Replace trusted TOML configuration or introduce YAML for daemon-owned state.

## Decisions

### Use formats according to ownership and access pattern

One profile uses this durable layout under `ProjectDirs::data_local_dir()`:

```text
storage.lock
manifest.json
generations/
  <generation-id>/
    snapshot.json
    transactions/
      <first-sequence>-<generation>.jsonl
      <segment>.idx.json
    logs/
      <first-sequence>-<generation>.jsonl
      <segment>.idx.json
    stream-state.json
plans/
  <plan-id>.json
secrets/
recovery/
```

The socket, discovery document, and process locks remain in `ProjectDirs::runtime_dir()` and are never treated as durable state. Snapshots, manifests, indexes, stream state, and plans are closed schema-versioned JSON documents. The state-change transaction journal and operational logs are append-only JSON Lines because records are versioned and read in sequence order. Each transaction record contains its `HistoryEvent`, so that journal is also the authoritative history stream. Trusted configuration remains TOML. YAML is reserved for a capability that explicitly defines human-authored YAML input; it is not a durable runtime format.

Alternatives rejected:

- One large JSON document rewrites unbounded logs and history and makes retention expensive.
- One file per event creates excessive directory and metadata overhead.
- YAML daemon state has ambiguous representations, slower parsing, and weaker deterministic canonicalization than closed JSON.
- CSV cannot preserve typed nested fields or additive schemas.

### Make one transaction journal record the durable mutation boundary

One `FileStore` actor holds `storage.lock` for the daemon lifetime and serializes commit, query, rotation, maintenance, and shutdown. Every accepted or rejected mutation becomes one bounded, LF-terminated transaction record containing schema version, sequence, previous record hash, nullable typed state delta, complete required `HistoryEvent`, and checksum. The record commits only after the complete line and file data are flushed; in-memory state changes only after that commit. A rejected mutation uses a null state delta, so its audit outcome remains durable without inventing a state change.

`snapshot.json` is an atomically published checkpoint, not an independent authority. Startup restores the selected generation's snapshot and replays every later valid transaction record. Old transaction segments can be removed for history retention only after a durable snapshot covers their state deltas and the manifest selects that snapshot. The manifest atomically selects one complete storage generation, allowing migrations to build and validate a replacement generation without modifying the current one.

Alternatives rejected:

- Best-effort independent state and history writes cannot satisfy `STO-003` or `OBS-007`.
- A pending transaction plus a separate history append creates an avoidable two-file recovery protocol. One bounded journal record is the commit.
- A custom general-purpose write-ahead log recreates a database engine. Cadder's journal has one closed mutation record, forward replay, immutable sealed segments, and no arbitrary query or concurrency layer.
- Filesystem hard links and multi-file rename schemes do not provide portable atomic directory transactions on all supported platforms.

### Publish small documents with the proven atomic-file primitive

Writers serialize a complete candidate, create an unpredictable same-directory temporary file with exclusive creation and owner-only permissions, flush it, atomically replace the destination, and persist the directory entry where the platform permits. Windows uses owner-only security descriptors plus `ReplaceFileW` or write-through `MoveFileExW`; Unix uses mode `0600`, `rename`, and parent-directory `fsync`. Generation-aware cleanup never deletes an unknown or replacement document.

The discovery publication work supplies the first reusable implementation and platform tests. Storage wraps that primitive instead of duplicating pathname and DACL logic.

### Segment append-only event streams

Each JSONL segment begins at a sequence encoded in its filename. Records end with LF and carry schema version, sequence, timestamp, previous-record hash where required, and checksum-covered payload. Transaction and log active segments rotate at 8 MiB or 10,000 records, whichever occurs first. Sealed segments never change except for bounded retention compaction of the oldest boundary segment after a covering snapshot exists.

Manifests record the active segment, first and last retained sequence, retention gap boundary, and segment checksums. Sparse JSON indexes map every 256th sequence and timestamp to a byte offset. Indexes are derived: a missing, incompatible, or corrupt index is rebuilt from validated segments and never makes authoritative records unavailable by itself.

The daemon discards only an incomplete final line after a crash. Invalid JSON, sequence discontinuity, checksum mismatch, or schema failure before the final tail preserves the segment as evidence and enters the typed degradation policy. It never skips an interior record silently.

### Keep maintenance incremental

Retention removes whole sealed segments first. If a limit crosses a segment, Cadder atomically writes a compacted replacement containing the retained suffix, updates the manifest, and removes the superseded segment only after the manifest commits. Each cycle has record, byte, and wall-clock budgets. The manifest retains the first available cursor so clients receive deterministic gap outcomes.

Integrity checks validate closed JSON schemas, filenames, sequence ranges, checksums, and manifest references. They sample or process bounded segments during normal operation and perform a full scan only at startup, explicit doctor/recovery, or before release verification.

### Preserve evidence and keep secrets separate

Unsupported schemas, invalid snapshots, broken transaction hash chains, and corrupt authoritative segments move as one generation-stamped set into `recovery/` only after an owner-protected destination is durable. Cadder never overwrites the evidence. It can rebuild indexes and stream metadata from valid authoritative files, but it does not synthesize missing state or event records.

Secret keys remain dedicated owner-protected files in `secrets/`; references in state, history, logs, manifests, indexes, diagnostics, and exports contain only non-secret identity and status metadata.

## Risks / Trade-offs

- [Filtering JSONL is slower than indexed SQL] → Keep deterministic limits, sparse seek indexes, immutable segments, and bounded in-memory indexes for common dimensions; benchmark the maximum supported retention before 1.0.
- [Multi-file mutation recovery is more complex than a database transaction] → Restrict the journal to one serialized transition, define every crash point, use generation visibility, and verify each point with fault injection.
- [Users can inspect but may edit files] → Treat files as internal owner-readable diagnostics, validate every schema/checksum, fail closed on edits, and expose supported changes only through CLI/TUI.
- [Windows and Unix durability semantics differ] → Keep platform operations narrow and run first-publication, replacement, power-loss simulation, permissions, long-path, and recovery suites on native CI hosts.
- [Retention compaction can amplify writes] → Rotate moderate immutable segments, delete whole segments first, and compact only one boundary segment per bounded cycle.

## Migration Plan

1. Update the target `runtime-storage` and `observability` contracts and archive this requirement change normally.
2. Split durable profile data paths from ephemeral transport paths and implement reusable atomic document, owner-only path, checksum, and fault-injection primitives.
3. Implement the single-writer `FileStore`, hash-chained transaction journal, replay, and versioned snapshots behind the existing `RuntimeStore` interface.
4. Implement segmented JSONL logs and history, sparse indexes, manifests, queries, tails, and retention.
5. Replace storage health DTO values and fixtures that identify SQLite, then remove `rusqlite` through Cargo.
6. Preserve any detected pre-1.0 SQLite artifacts under `recovery/legacy-sqlite-<timestamp>/` without importing or deleting them.
7. Update architecture and recovery documentation after executable tests prove the file layout.

Rollback before 1.0 restores the previous code and its development-only database format together. No released 1.0 data format is downgraded.

## Open Questions

None. Segment limits are implementation constants verified against the maximum supported retention and can change before 1.0 without changing cursor or event contracts.
