## MODIFIED Requirements

### Requirement: RUN-002: Managed run may start the daemon
When attachment reports that no daemon is running, supported `caddy run` MUST launch the version-matched sibling `cadderd`, poll the exact handshake to readiness, and retry attachment. Automatic managed startup MUST NOT discover `cadderd` through PATH. An explicit daemon path MAY remain available to focused tests and foreground diagnostics.

#### Scenario: Startup fails
- **WHEN** the sibling daemon cannot be found or become ready within the bounded deadline
- **THEN** the shim reports actionable diagnostics and never launches an unrelated PATH executable or delegates an unmanaged run
