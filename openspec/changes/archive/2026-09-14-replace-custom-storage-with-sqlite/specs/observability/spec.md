## REMOVED Requirements

### Requirement: OBS-011: Durable event streams use recoverable JSON Lines segments
**Reason**: Bounded recent logs are stored transactionally by the replacement database. JSONL segments, manifests, checksums, sparse indexes, rotation, incomplete-tail recovery, and stream replay are implementation mechanisms with no retained user journey.

**Migration**: Existing pre-1.0 event files are removed by the allowlisted cleanup after the new database opens successfully; no history or export is imported.
