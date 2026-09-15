## ADDED Requirements

### Requirement: RUN-001: Runtime identity is per user and profile
`cadderd` SHALL maintain one isolated runtime identity for each operating-system user and runtime profile.

#### Scenario: Default profile
- **WHEN** a user starts the daemon without selecting a profile
- **THEN** the daemon uses the documented default profile under that user's platform runtime directory

#### Scenario: Two profiles
- **WHEN** the same user starts two different runtime profiles
- **THEN** each profile has independent IPC discovery, locks, storage, Caddy administration, configured listeners, logs, and process ownership
- **AND** startup rejects a listener collision without changing the already-running profile

#### Scenario: Two users
- **WHEN** two operating-system users run Cadder on the same machine
- **THEN** neither user's daemon reads, mutates, or authenticates through the other user's runtime state

### Requirement: RUN-002: Daemon lifecycle is explicit
Cadder SHALL expose explicit status, start, stop, and restart operations and MUST NOT silently create a daemon for an unrelated operator or shim command.

#### Scenario: Explicit start
- **WHEN** a user runs `cadder daemon start`
- **THEN** the command starts the selected per-user daemon and waits for a ready discovery record

#### Scenario: Offline client command
- **WHEN** the shim or an operator command requires a daemon and none is available
- **THEN** the command fails with daemon-unavailable guidance
- **AND** it does not start unmanaged Caddy or silently spawn the daemon

#### Scenario: Restart
- **WHEN** a user runs `cadder daemon restart`
- **THEN** Cadder drains and stops the current daemon before starting a new instance
- **AND** success is reported only after the new instance is ready

### Requirement: RUN-003: A profile admits one live daemon
Each runtime profile MUST use an atomic ownership lock with an instance generation so that no two live daemons can own the same profile.

#### Scenario: Concurrent start
- **WHEN** two start attempts race for one profile
- **THEN** one daemon acquires ownership
- **AND** the other reports the already-running instance without replacing its state

#### Scenario: Stale lock
- **WHEN** a lock refers to an instance that is no longer alive and its generation cannot answer a readiness probe
- **THEN** Cadder recovers the stale lock atomically only after `RUN-009` proves that the previous generation's contained Caddy child has exited
- **AND** it does not report the replacement ready while the previous generation can still serve or hold a listener

#### Scenario: Ambiguous ownership
- **WHEN** Cadder cannot prove that an existing lock is stale
- **THEN** startup fails closed with recovery guidance

### Requirement: RUN-004: The daemon owns only the Caddy process it starts
`cadderd` MUST track and control only the real Caddy child process created for its own runtime profile.

#### Scenario: Managed process shutdown
- **WHEN** the daemon stops normally
- **THEN** it requests graceful termination of its owned Caddy child
- **AND** it waits for or safely escalates that specific child according to the documented timeout

#### Scenario: Unrelated Caddy process
- **WHEN** another Caddy process exists on the machine
- **THEN** Cadder does not enumerate, signal, or terminate that process

#### Scenario: Lost child identity
- **WHEN** the recorded child identity no longer matches the running process
- **THEN** Cadder treats the child as unowned and reports a degraded runtime instead of terminating it

### Requirement: RUN-005: Shutdown drains in-flight work
The daemon SHALL stop accepting new mutating requests, cancel or finish owned background work, flush durable state, and remove discovery metadata only after it attempts terminal delivery to active connections and completes its bounded drain.

#### Scenario: Stop with active request
- **WHEN** daemon shutdown begins while a bounded request is in progress
- **THEN** the request either completes within its operation deadline or receives a typed cancellation error
- **AND** no detached task continues mutating state after shutdown completes

#### Scenario: Log tail during shutdown
- **WHEN** a client tails logs while the daemon stops
- **THEN** the stream emits a terminal shutdown outcome and closes

### Requirement: RUN-006: Autostart is explicit and reversible
Cadder SHALL expose per-user autostart status, enable, and disable operations without requiring administrative privileges.

#### Scenario: Enable autostart
- **WHEN** a user enables autostart for a profile
- **THEN** Cadder installs or updates one user-level platform entry that starts `cadderd` for that profile

#### Scenario: Disable autostart
- **WHEN** a user disables autostart
- **THEN** Cadder removes only the entry it owns
- **AND** it leaves the currently running daemon unchanged

#### Scenario: Foreign autostart entry
- **WHEN** the expected entry exists but its provenance does not match Cadder
- **THEN** Cadder reports a conflict and does not overwrite or remove it

### Requirement: RUN-007: Degraded state remains inspectable
The daemon MUST expose a typed degraded state when storage, Caddy, configuration, or recovery fails and SHALL reject unsafe mutations while preserving read-only diagnostics.

#### Scenario: Caddy recovery fails
- **WHEN** the daemon cannot restore the last-known-good Caddy configuration
- **THEN** runtime status identifies the Caddy subsystem as degraded
- **AND** status, doctor, logs, history, and export remain available
- **AND** configuration mutations fail with recovery guidance

#### Scenario: Degraded cause clears
- **WHEN** the user completes the documented recovery and the daemon verifies healthy state
- **THEN** the daemon leaves degraded mode and accepts mutations without losing runtime history

### Requirement: RUN-008: Trusted configuration has one explicit precedence
Each profile SHALL load `cadder.toml` from the standard Cadder per-user application-config directory and MAY inherit an administrator-owned `cadder.toml` from the platform system-config directory. Each file SHALL contain a `[defaults]` table and MAY contain `[profiles.<name>]` tables. For the selected profile, configuration precedence MUST be an absolute `--real-caddy` value supplied to `cadder daemon start` or `cadder daemon restart`, then the per-user profile table, per-user defaults, the system profile table, system defaults, and documented built-in defaults. Project files and registration working directories MUST NOT participate in this configuration chain.

The trusted schema SHALL define the real-Caddy path, loopback HTTP and HTTPS listener endpoints, log retention days and count, and history retention days and count. The daemon MUST reject unknown keys, invalid values, non-loopback listeners, unsafe file ownership, and listener collisions before creating a Caddy child. It SHALL load one immutable configuration snapshot at startup; a CLI real-Caddy override applies only to that start or restart and MUST NOT be persisted.

#### Scenario: Per-user configuration overrides system configuration
- **WHEN** owner-protected per-user configuration and administrator-owned system configuration define the same supported field
- **THEN** the daemon uses the per-user value
- **AND** status and doctor identify the active source without exposing sensitive values

#### Scenario: Two profiles select different listeners
- **WHEN** per-user profile tables assign non-conflicting loopback listeners to two profiles
- **THEN** each daemon uses only the endpoints from its selected profile overlay
- **AND** neither profile reads or rewrites the other profile's runtime state

#### Scenario: CLI selects real Caddy for one start
- **WHEN** the runtime owner supplies an absolute `--real-caddy` path to daemon start or restart
- **THEN** that path takes precedence for the new daemon instance
- **AND** Cadder does not write the path to persistent configuration

#### Scenario: Trusted configuration is unsafe
- **WHEN** a configuration file has unsafe ownership, contains an unknown key, selects a non-loopback listener, or conflicts with another live profile
- **THEN** startup fails before creating locks, discovery metadata, IPC endpoints, or a Caddy child
- **AND** the diagnostic identifies the invalid field or conflicting endpoint

#### Scenario: A trusted file changes while the daemon runs
- **WHEN** per-user or system configuration changes after startup
- **THEN** the running daemon keeps its pinned configuration snapshot
- **AND** the new values take effect only after an explicit restart

### Requirement: RUN-009: Managed Caddy cannot outlive runtime ownership
Before a managed Caddy child executes project configuration, the daemon MUST attach it to a cross-platform lifetime-containment boundary owned by that runtime generation. During controlled shutdown, the live daemon SHALL request graceful stop and use its retained child handle for bounded escalation as defined by `RUN-004`. After a crash, forced daemon termination, or parent-session teardown, the independent containment boundary MUST terminate the retained child directly, without depending on daemon code that no longer runs, and the child MUST exit within ten seconds of owner loss. If containment cannot be established before execution, the daemon MUST fail startup without launching Caddy.

A replacement daemon SHALL prove that the previous generation's contained child is absent before acquiring readiness. The proof MUST bind the profile, daemon generation, child operating-system identity, pinned executable identity, and an owner-protected unpredictable generation value; a PID, process name, or listener alone MUST NOT establish ownership. If the previous boundary has not completed or the proof is ambiguous, startup MUST fail closed with recovery guidance instead of starting a second Caddy or signaling an unproven process.

#### Scenario: Daemon crashes while Caddy is serving
- **WHEN** the daemon exits unexpectedly while its managed Caddy child is running
- **THEN** the lifetime boundary stops or terminates that exact child within ten seconds
- **AND** no unrelated Caddy process receives a signal

#### Scenario: Replacement follows a crash
- **WHEN** a new daemon finds a stale lock and a verifiable previous-generation containment record
- **THEN** it waits for proof that the contained child has exited before taking readiness ownership
- **AND** it starts its own Caddy only after the previous generation can no longer serve or hold the profile listeners

#### Scenario: Previous child identity is ambiguous
- **WHEN** a stale record does not match the protected generation value, executable identity, process identity, or profile
- **THEN** Cadder leaves the process untouched and refuses replacement startup
- **AND** diagnostics explain how the runtime owner can inspect and recover the profile safely
