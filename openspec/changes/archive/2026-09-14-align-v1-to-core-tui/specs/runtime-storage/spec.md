## MODIFIED Requirements

### Requirement: STO-001: Runtime state is durable per installation
The installation runtime SHALL durably retain stable entrypoint keys, opaque public registration IDs, desired entrypoint and domain activation, and the bounded redacted logs defined by the observability contract. Storage MUST remain owner-protected. Local endpoints, connection leases, shim sessions, daemon-instance identity, and process ownership SHALL remain ephemeral and MUST NOT become valid merely because a record survived restart.

The product contract SHALL NOT require profiles, historical tombstones, operation history, exports, snapshots, journals, manifests, hash chains, segments, indexes, or a particular storage engine. Those are not public compatibility surfaces.

#### Scenario: Daemon restart
- **WHEN** a healthy daemon restarts
- **THEN** it restores durable identity and desired activation without restoring any previous live lease
- **AND** project routes remain absent until a matching shim establishes a new instance-bound lease

#### Scenario: Storage implementation changes
- **WHEN** Cadder replaces one internal storage engine with another before 1.0
- **THEN** the retained product state and owner-only access behavior remain the compatibility boundary

## RENAMED Requirements

- FROM: `### Requirement: STO-001: Runtime state is durable per profile`
- TO: `### Requirement: STO-001: Runtime state is durable per installation`
