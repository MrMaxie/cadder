## MODIFIED Requirements

### Requirement: STO-001: Runtime state is durable beside the executable
Cadder SHALL persist stable entrypoint keys, their opaque public registration IDs or query tombstones, desired entrypoint and domain activation state, applied-state metadata, history, and structured logs in the `data` directory under the portable runtime root. The runtime root SHALL be the parent directory of the running Cadder executable and SHALL NOT depend on the process working directory. Versioned JSON snapshots SHALL checkpoint state, a versioned JSON Lines transaction stream SHALL contain state changes and history, and a separate versioned JSON Lines stream SHALL contain operational logs. Sockets, discovery, process locks, connection leases, and daemon-instance ownership MUST remain ephemeral and MUST NOT be treated as durable state.

#### Scenario: Daemon restart
- **WHEN** a healthy daemon restarts from the same portable release directory
- **THEN** it restores durable desired state from that directory's `data` store and marks previously live entrypoints as reconnecting before accepting mutations
- **AND** it does not treat a persisted record as a valid lease for the new daemon instance

#### Scenario: Working directory does not select storage
- **WHEN** Cadder starts from a working directory outside the portable release directory
- **THEN** it uses only the `data` directory beside its executable
