# project-registration Specification

## Purpose
Define safe shim registration, durable entrypoint identity, ephemeral lease ownership, host conflicts, command delegation, detachment, and alias provenance.

## Requirements
### Requirement: REG-001: Managed run registration
The compatibility shim SHALL treat `caddy run`, optionally with `--config <path>` and `--adapter caddyfile`, as one project-entrypoint registration with the selected daemon profile. The project root SHALL be the shim's initial working directory resolved once to an absolute canonical directory through the operating system's symlink, junction, volume, and case-equivalence rules. An omitted config path SHALL resolve to `Caddyfile` under that root; an explicit config path MUST resolve to a regular file within the same root. Standard input, paths outside the root, other adapters, and every other `run` option MUST be rejected. The shim MUST submit the bounded canonical source, canonical config path, and project-root context, wait for daemon validation and transactional application, and report readiness only after the daemon accepts the registration. It MUST NOT start an unmanaged Caddy process or silently start the daemon.

#### Scenario: Registration becomes ready
- **WHEN** a project invokes `caddy run` with a supported Caddyfile and the selected daemon is ready
- **THEN** the shim submits one registration and receives its stable entrypoint key and daemon-assigned public registration ID
- **AND** the invocation reports readiness only after the accepted configuration is active

#### Scenario: Daemon is unavailable
- **WHEN** a project invokes `caddy run` while the selected daemon is unavailable
- **THEN** the shim exits with daemon-unavailable guidance
- **AND** it starts neither `cadderd` nor a real Caddy process

#### Scenario: Unsupported managed-run input
- **WHEN** `caddy run` requests an unsupported adapter, administration option, or listener override
- **THEN** the shim rejects the invocation before registration
- **AND** the diagnostic identifies the unsupported argument without changing runtime state

### Requirement: REG-002: Registration identity is durable and lease ownership is session-scoped
The daemon MUST assign each stable entrypoint key one opaque public registration ID that persists across daemon restarts and valid lease rebinds. Each live attachment SHALL receive a separate opaque lease bound to the authenticated operating-system principal, daemon instance, and owning shim session identity. The session identity SHALL survive transport reconnection by that shim, and only the owning shim session SHALL renew, replace, or unregister the live attachment.

#### Scenario: Owner renews a registration
- **WHEN** the owning shim renews its lease through the authenticated session
- **THEN** the daemon retains the registration and returns its current lease deadline

#### Scenario: Another session targets the registration
- **WHEN** a different shim session attempts to renew, replace, or unregister an existing registration
- **THEN** the daemon rejects the operation as an ownership conflict before changing state

#### Scenario: Daemon instance changes
- **WHEN** a shim reconnects after the daemon instance identifier changes
- **THEN** the daemon rejects the old lease
- **AND** the shim submits a fresh lease request for the same stable entrypoint key and public registration ID against the new instance

### Requirement: REG-003: Heartbeats use a bounded lease
An attached shim SHALL renew its registration every 5 seconds, and the daemon MUST expire the registration when it receives no valid renewal for 30 seconds. After losing IPC, the shim SHALL reconnect with exponential backoff that starts at 100 milliseconds and is capped at 5 seconds. If it cannot restore ownership before lease expiry, it MUST exit with a nonzero status.

#### Scenario: Temporary IPC interruption
- **WHEN** IPC becomes unavailable and returns before the 30-second lease expires
- **THEN** the shim reconnects with bounded backoff, proves ownership, and resumes heartbeats
- **AND** the registration remains active without a duplicate entrypoint

#### Scenario: Ownership cannot be restored
- **WHEN** the shim cannot renew or recreate its registration before the lease expires
- **THEN** the daemon removes the registration through the normal configuration transaction
- **AND** the shim exits instead of continuing as an apparently managed `caddy run` process

#### Scenario: Slow or suspended shim
- **WHEN** a shim misses the lease deadline even though its process still exists
- **THEN** the daemon expires the registration without using the shim PID as proof of ownership

### Requirement: REG-004: Domain ownership conflicts fail atomically
The daemon SHALL canonicalize every registered host using lowercase IDNA form without a trailing dot and MUST allow at most one active entrypoint to own a canonical host in a runtime profile. A conflicting candidate MUST fail as a whole while the previously active registration and effective Caddy configuration remain unchanged.

#### Scenario: Two projects claim one host
- **WHEN** a second entrypoint attempts to register a host owned by another active entrypoint
- **THEN** the daemon rejects the complete candidate as a conflict
- **AND** the response identifies the canonical host and existing registration ID

#### Scenario: Equivalent host spelling
- **WHEN** two registrations use host spellings that canonicalize to the same IDNA name
- **THEN** the daemon treats them as the same host for conflict detection

#### Scenario: Owner replaces its configuration
- **WHEN** the owning registration submits a valid replacement with the same host
- **THEN** the daemon applies the replacement as one transaction without creating a second owner

### Requirement: REG-005: Shim command policy is explicit
The compatibility shim SHALL manage only `caddy run`. It SHALL delegate the read-only `version`, `help`, `list-modules`, `validate`, and `adapt` commands to the trusted real-Caddy installation, write a delegation notice to stderr, preserve delegated stdout and exit status, and reject `start`, `stop`, `reload`, and unknown mutating commands with guidance to use `cadder`.

#### Scenario: Read-only command
- **WHEN** a project invokes a supported read-only Caddy command through the shim
- **THEN** the shim resolves the configured and pinned real Caddy independently of project inputs and delegates the command
- **AND** stdout contains only the real command output while stderr identifies the delegation

#### Scenario: Caddy lifecycle mutation
- **WHEN** a project invokes `caddy start`, `caddy stop`, or `caddy reload` through the shim
- **THEN** the shim rejects the command before contacting the real Caddy Admin API
- **AND** the diagnostic directs the user to the equivalent `cadder` workflow when one exists

#### Scenario: Unknown command
- **WHEN** a project invokes an unknown command through the shim
- **THEN** the shim fails closed without executing the real Caddy binary

### Requirement: REG-006: Project inputs cannot select executable or privileged behavior
Project-controlled files, working directories, command arguments, and environment variables MUST NOT select the real Caddy executable, alter the daemon's trusted executable sources, start an elevated daemon, or invoke the IIS helper. Executable selection and privileged plans SHALL come only from the trusted control paths defined by the Caddy runtime and IIS contracts.

#### Scenario: Project-local executable override
- **WHEN** a project `cadder.toml`, Caddyfile, or shim argument attempts to name a real-Caddy command
- **THEN** Cadder rejects the selector as unsupported project configuration
- **AND** it does not execute the named program

#### Scenario: Inherited command override
- **WHEN** the shim process inherits a project-provided variable that names a Caddy executable
- **THEN** managed registration and read-only delegation ignore that variable as an executable source

#### Scenario: Project requests elevation
- **WHEN** a project input attempts to request elevation or an IIS helper operation
- **THEN** the daemon rejects the request before launching another process

### Requirement: REG-007: Shim alias setup preserves existing Caddy installations
Cadder SHALL distribute the compatibility shim as `cadder-caddy` and SHALL create a PATH-facing `caddy` alias only through explicit `cadder setup shim`. Without `--dir`, setup SHALL use the documented per-user command directory. An explicit directory MUST already exist, be owner-writable, belong to the current user's PATH, and not be administrator-owned or shared with another user. Setup MUST verify an empty destination or Cadder-owned provenance; removal MUST delete only an alias whose recorded provenance and current target both identify the installed `cadder-caddy`.

#### Scenario: Empty alias destination
- **WHEN** a user explicitly requests shim setup at a writable destination with no `caddy` entry
- **THEN** Cadder creates the alias and records owner-readable provenance
- **AND** the independently installed real Caddy remains unchanged and discoverable

#### Scenario: Existing command collision
- **WHEN** the selected destination already contains a real Caddy executable or an unrelated `caddy` entry
- **THEN** setup reports a conflict without overwriting, renaming, or deleting the entry

#### Scenario: Explicit destination is unsafe
- **WHEN** `--dir` names a missing, non-PATH, shared, administrator-owned, or non-owner-writable directory
- **THEN** setup rejects the destination without creating an alias or provenance record

#### Scenario: Provenance mismatch during removal
- **WHEN** `cadder setup shim --remove` finds that the alias target or provenance has changed
- **THEN** removal fails closed and leaves the entry unchanged

### Requirement: REG-008: Detachment removes only accepted ownership
The owning shim SHALL request detachment during a controlled exit, and the daemon SHALL remove an expired or detached live registration only after the corresponding Caddy configuration transaction succeeds. `EntrypointSnapshot` MUST expose three orthogonal fields: `desiredActivation` as `enabled` or `disabled`; `leaseState` as `active`, `reconnecting`, `expiring`, `detached`, or `expired`; and `configurationStatus` as `active`, `applying`, `removal-failed`, or `degraded`. Rejected candidates SHALL appear in their response and history rather than as accepted entrypoints.

#### Scenario: Controlled shim exit
- **WHEN** the owning shim receives a supported termination signal
- **THEN** it requests detachment and waits for the daemon's bounded terminal outcome
- **AND** the daemon removes only that shim's accepted registration

#### Scenario: Removal apply fails
- **WHEN** removing a registration would produce a Caddy configuration that fails validation or application
- **THEN** the daemon retains the last successfully applied configuration
- **AND** it exposes the registration as removal-failed with recovery guidance instead of reporting it as removed

#### Scenario: Entrypoint state is inspected
- **WHEN** an operator reads the entrypoint snapshot during reconnect or expiry
- **THEN** the snapshot reports the stable entrypoint key, registration ID, canonical hosts, desired activation state, lease state, configuration status, and last accepted outcome

### Requirement: REG-009: Stable entrypoint identity separates desired state from leases
The daemon SHALL derive a stable `EntrypointKey` from the selected profile, canonical project root, and canonical Caddyfile path. Canonical source identity SHALL mean those paths, not a content hash, so an edited Caddyfile remains the same entrypoint and enters normal replacement validation. The daemon MUST persist desired entrypoint and domain activation state under that key while binding each live registration to a daemon-instance lease. A daemon restart SHALL place restored entrypoints in `reconnecting` state for 30 seconds. During that window, only the same authenticated shim session may rebind the key; after the window expires, a new shim session owned by the same operating-system principal may claim the key with matching canonical source identity. A successful controlled detach releases the session immediately and leaves the durable record in `detached` state.

The runtime owner MAY enable or disable a live entrypoint or domain through CLI or TUI without gaining lease renewal, replacement, or unregister authority. Each activation change MUST update desired state, effective Caddy configuration, and history through the same recoverable transaction. A disabled entrypoint retains its stable key and desired state but releases active host ownership. An enable request that conflicts with another accepted owner MUST fail without changing either entrypoint. The runtime owner MAY forget a detached or expired entrypoint; forgetting removes its stable key and desired state while converting its public registration ID into a read-only query tombstone. The tombstone MUST remain until no retained log or history record references that ID. A live entrypoint MUST be detached before it can be forgotten.

#### Scenario: Shim reconnects after daemon restart
- **WHEN** the original shim reconnects within 30 seconds with the same session identity and stable entrypoint key
- **THEN** the daemon issues a new instance-bound lease for the existing durable entrypoint
- **AND** it retains the public registration ID used by CLI, TUI, logs, and history
- **AND** it restores only the routes allowed by the durable desired activation state

#### Scenario: Different shim races the reconnect window
- **WHEN** another shim session presents the same stable entrypoint key before the reconnect window expires
- **THEN** the daemon rejects the claim as an ownership conflict
- **AND** the durable desired state remains unchanged

#### Scenario: Original shim does not return
- **WHEN** the reconnect window expires without the original session
- **THEN** the daemon removes the old live ownership and its routes through the normal configuration transaction
- **AND** a later same-principal shim may claim the stable key with matching canonical source identity

#### Scenario: New shim follows a controlled detach
- **WHEN** the prior shim detached successfully and a same-principal shim presents the same canonical project root and Caddyfile path
- **THEN** the daemon validates the current Caddyfile as a replacement for the durable entrypoint
- **AND** successful registration retains its stable key, public registration ID, and desired activation state

#### Scenario: Operator disables one domain
- **WHEN** the runtime owner disables an active domain through CLI or TUI
- **THEN** the daemon persists the disabled desired state and removes that route transactionally
- **AND** a daemon restart and valid shim rebind do not enable the domain implicitly

#### Scenario: Re-enable conflicts with another owner
- **WHEN** the runtime owner enables a disabled domain now owned by another accepted entrypoint
- **THEN** the operation fails as a conflict
- **AND** both entrypoints retain their previous desired and effective state

#### Scenario: Operator forgets a detached entrypoint
- **WHEN** the runtime owner forgets an entrypoint with no live lease
- **THEN** the daemon removes its stable key and desired activation state atomically and converts its public registration ID into a query tombstone
- **AND** retained redacted logs and history remain queryable by that ID until normal retention removes them

#### Scenario: Historical identity reaches the end of retention
- **WHEN** no retained log or history record references a forgotten registration ID
- **THEN** maintenance removes its query tombstone
- **AND** a later query for that ID returns not found

#### Scenario: Operator attempts to forget a live entrypoint
- **WHEN** the runtime owner attempts to forget an entrypoint with a current lease
- **THEN** the daemon rejects the operation as a failed precondition
- **AND** it instructs the operator to stop the owning shim first
