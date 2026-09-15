# Daemon lifecycle

## Purpose
Define singleton ownership, startup, and bounded shutdown for one installation runtime.

## Requirements

### Requirement: RUN-001: One installation admits one daemon
`cadderd` MUST claim the owner-protected IPC endpoint before initializing storage or Caddy, and a second start SHALL attach to the healthy owner rather than creating competing state.

#### Scenario: Concurrent starts
- **WHEN** two starts race for one installation directory
- **THEN** exactly one process becomes the daemon and the other observes its readiness

### Requirement: RUN-002: Managed run may start the daemon
When attachment reports that no daemon is running, supported `caddy run` MUST launch the version-matched sibling `cadderd`, poll the exact handshake to readiness, and retry attachment.

#### Scenario: Startup fails
- **WHEN** the sibling daemon cannot become ready within the bounded deadline
- **THEN** the shim reports actionable diagnostics and never delegates an unmanaged run

### Requirement: RUN-003: Shutdown is bounded and owned
Shutdown SHALL stop accepting work, drain or revoke owned operations, close SQLite, terminate only the Cadder-owned Caddy child, and release the endpoint within bounded phase deadlines.

#### Scenario: Work exceeds a phase
- **WHEN** an owned operation exceeds its shutdown phase
- **THEN** Cadder escalates only that owned work and continues teardown

### Requirement: RUN-004: Trusted configuration is immutable per start
The daemon SHALL load one non-profile configuration snapshot from trusted sources at startup and pin the selected real Caddy executable until Restart.

#### Scenario: Configuration changes while running
- **WHEN** a trusted file changes after readiness
- **THEN** the active daemon retains its existing snapshot until Restart
