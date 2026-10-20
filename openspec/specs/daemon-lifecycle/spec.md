# Daemon lifecycle

## Purpose
Define singleton ownership, startup, and bounded shutdown for one installation runtime.

## Requirements

### Requirement: RUN-001: One installation admits one daemon
One owner runtime v2 SHALL admit one daemon through a separate SQLite lock database holding BEGIN EXCLUSIVE for the process lifetime. Metadata SHALL be diagnostic only. Startup SHALL acquire the actual lock before crash recovery, bind/publish readiness only after protected startup succeeds, and release acquired resources on failure. PID checks or expiring leases SHALL NOT grant ownership.

#### Scenario: Concurrent start after a crash
- **WHEN** two starts encounter stale or misleading owner metadata
- **THEN** only the actual SQLite lock owner SHALL recover residue and become ready
- **AND** the other SHALL observe the existing owner or report bounded contention

### Requirement: RUN-002: Managed run may start the daemon
Managed caddy run SHALL use the version-matched packaged daemon and MAY start it when missing through the supported npm or SEA path and authenticated readiness. It SHALL NOT discover an unrelated daemon through PATH or start independent Caddy. State/inspection CLI commands SHALL remain attach-only; explicit daemon start/restart and cadder tui --start-daemon SHALL retain the released bounded launch behavior.

#### Scenario: Packaged daemon cannot start
- **WHEN** the packaged daemon fails to become authenticated and ready
- **THEN** the shim SHALL report the failure without unmanaged Caddy fallback
- **AND** ordinary inspection commands SHALL NOT implicitly start a daemon

### Requirement: RUN-003: Shutdown is bounded and owned
Shutdown SHALL stop admission, settle or revoke owned work, close the application-state and diagnostic-log worker, gracefully stop the owned Caddy child, then force only that child after bounded deadlines. It SHALL release endpoint resources and the SQLite exclusion connection only after owned teardown. It SHALL NOT enumerate or kill unrelated Caddy processes.

#### Scenario: Graceful child shutdown stalls
- **WHEN** the owned Caddy child exceeds the graceful deadline
- **THEN** the daemon SHALL terminate only its owned process and complete teardown
- **AND** unrelated Caddy processes SHALL remain untouched

### Requirement: RUN-004: Trusted configuration is immutable per start
The daemon SHALL load one non-profile configuration snapshot from trusted sources at startup and pin the selected real Caddy executable until Restart.

#### Scenario: Configuration changes while running
- **WHEN** a trusted file changes after readiness
- **THEN** the active daemon retains its existing snapshot until Restart

### Requirement: RUN-005: Owner clients can contact explicitly elevated daemons
Normal operation SHALL run at user privilege. A same-owner ordinary client SHALL retain authenticated access to an explicitly elevated daemon. Unix root startup SHALL require an explicit runtime directory and owner UID. Other users and remote clients SHALL be denied without weakening filesystem or ACL policy.

#### Scenario: Ordinary owner contacts elevated endpoint
- **WHEN** the ordinary owner authenticates to an explicitly elevated daemon
- **THEN** documented operations SHALL remain available under the same policy
- **AND** a different account SHALL be rejected before product handlers execute
