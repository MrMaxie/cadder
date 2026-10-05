## MODIFIED Requirements

### Requirement: CAD-008: Configuration application is transactional
For every candidate, the daemon SHALL adapt and validate with the pinned real-Caddy installation, submit the final Cadder-owned JSON to the private Admin API, and read the active configuration to verify its canonical hash. It SHALL then commit matching durable desired state and publish the new in-memory authoritative snapshot. A failed stage MUST preserve or restore the previously verified Caddy configuration and durable desired state.

The daemon SHALL retain the previous verified Caddy JSON in memory for rollback during its lifetime. It MUST NOT persist an effective or last-known-good Caddy generation for replay after daemon restart; project routes return only after current shims establish new leases.

#### Scenario: Candidate succeeds
- **WHEN** Caddy accepts the candidate, the active hash matches, and the durable transaction commits
- **THEN** the new desired state and in-memory snapshot become authoritative

#### Scenario: Caddy rejects the load
- **WHEN** the private Admin API rejects a candidate
- **THEN** Caddy continues serving the prior configuration and durable state remains unchanged

#### Scenario: Persistence fails after load
- **WHEN** Caddy loads the candidate but durable state cannot commit
- **THEN** Cadder reloads and verifies the previous in-memory configuration
- **AND** it enters degraded read-only state if restoration cannot be verified

#### Scenario: Daemon restarts
- **WHEN** a new daemon starts with durable desired activation but no live shim leases
- **THEN** it starts from an empty project-route configuration and waits for current registrations instead of replaying stored effective JSON
