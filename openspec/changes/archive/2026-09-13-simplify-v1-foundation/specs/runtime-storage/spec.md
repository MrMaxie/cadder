## MODIFIED Requirements

### Requirement: STO-001: Runtime state is durable per profile
Each runtime profile SHALL persist stable entrypoint keys, their opaque public registration IDs or query tombstones, desired entrypoint and domain activation state, applied-state metadata, history, and structured logs in an owner-protected file store rooted in the platform's durable per-user local-data directory. Versioned JSON snapshots SHALL checkpoint state, a versioned JSON Lines transaction stream SHALL contain state changes and history, and a separate versioned JSON Lines stream SHALL contain operational logs. Local sockets, connection leases, and daemon-instance ownership MUST remain ephemeral and MUST NOT be treated as durable state.

#### Scenario: Daemon restart
- **WHEN** a healthy daemon restarts
- **THEN** it restores durable desired state and marks previously live entrypoints as reconnecting before accepting mutations
- **AND** it does not treat a persisted record as a valid lease for the new daemon instance
