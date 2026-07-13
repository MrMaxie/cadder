## ADDED Requirements

### Requirement: OBS-011: Durable event streams use recoverable JSON Lines segments
The daemon SHALL persist operational logs in an owner-protected JSON Lines stream and SHALL persist each `HistoryEvent` inside the owner-protected state-change transaction JSON Lines stream. Each stream SHALL use a versioned manifest, one append-only active segment, immutable sealed segments, and rebuildable sparse indexes. Every complete line MUST contain exactly one bounded record and terminate with LF. Segment filenames and manifests SHALL preserve sequence order and the first available cursor after retention.

#### Scenario: Active segment reaches its rotation limit
- **WHEN** an active segment reaches 8 MiB or 10,000 records
- **THEN** the daemon flushes and seals it before publishing the next active segment
- **AND** queries continue across the segment boundary without a duplicate or missing sequence

#### Scenario: Crash leaves an incomplete final line
- **WHEN** startup finds bytes after the final LF in the active segment
- **THEN** the daemon removes only that incomplete tail after preserving a bounded diagnostic
- **AND** every earlier complete record remains available in sequence order

#### Scenario: Interior event record is invalid
- **WHEN** a sealed segment or a complete non-final active-segment line has invalid JSON, schema, checksum, or sequence continuity
- **THEN** the daemon preserves the segment as corruption evidence and enters the typed storage-degraded state
- **AND** it does not silently skip the invalid record

#### Scenario: Sparse index is unavailable
- **WHEN** a query encounters a missing, incompatible, or corrupt sparse index for otherwise valid segments
- **THEN** the daemon performs a bounded rebuild or scan from authoritative JSON Lines records
- **AND** it does not report an event gap caused only by derived index loss
