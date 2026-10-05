## MODIFIED Requirements

### Requirement: REG-001: Managed run registration
The PATH-facing compatibility shim SHALL treat `caddy run`, optionally with `--config <path>` and `--adapter caddyfile`, as one project-entrypoint registration in the installation runtime. The project root SHALL be the shim's initial working directory resolved once to an absolute canonical directory. An omitted config path SHALL resolve to `Caddyfile` under that root; an explicit config path MUST resolve to a regular file within the same root. Standard input, paths outside the root, other adapters, and every other `run` option MUST be rejected.

When no compatible daemon answers, the shim SHALL start the daemon from the same Cadder archive, wait for its exact-version readiness handshake, and then submit the bounded canonical source and context. It MUST NOT start an unmanaged Caddy process or report readiness before the daemon accepts and applies the registration.

#### Scenario: Registration auto-starts the daemon
- **WHEN** a project invokes supported `caddy run` and no daemon is running
- **THEN** the shim starts the archive's `cadderd`, waits for readiness, and registers the project
- **AND** readiness is reported only after the accepted configuration is active

#### Scenario: Daemon cannot become ready
- **WHEN** the daemon fails to start, authenticate, or match the shim protocol version
- **THEN** the shim exits with recovery guidance and starts no unmanaged Caddy process

#### Scenario: Unsupported managed-run input
- **WHEN** `caddy run` requests an unsupported adapter, administration option, listener override, or project-external config path
- **THEN** the shim rejects the invocation before changing runtime state

### Requirement: REG-002: Registration identity is durable and lease ownership is session-scoped
The daemon MUST assign each stable entrypoint key one opaque public registration ID that persists across daemon restarts and valid lease rebinds. Each live attachment SHALL receive a separate opaque lease bound to the authenticated operating-system owner, daemon instance, and owning shim session. Only that shim session SHALL renew, replace, or unregister its live attachment.

#### Scenario: Owner renews a registration
- **WHEN** the owning shim renews through its authenticated session
- **THEN** the daemon retains the registration and returns its current lease deadline

#### Scenario: Another session targets the registration
- **WHEN** a different shim session attempts to renew, replace, or unregister a live registration
- **THEN** the daemon rejects the operation before changing state

### Requirement: REG-003: Heartbeats use a bounded lease
An attached shim SHALL renew its registration every 5 seconds, and the daemon MUST expire it after 30 seconds without a valid renewal. After losing IPC, the shim SHALL reconnect with exponential backoff starting at 100 milliseconds and capped at 5 seconds. If ownership cannot be restored before lease expiry, the shim MUST exit nonzero.

#### Scenario: Temporary IPC interruption
- **WHEN** IPC returns before the lease expires
- **THEN** the shim reconnects, proves ownership, and resumes heartbeats without a duplicate entrypoint

#### Scenario: Daemon is absent during reconnect
- **WHEN** the existing daemon exits while an attached shim still owns a live managed run
- **THEN** the shim attempts to start the same archive's daemon and rebind its stable entrypoint before lease expiry

#### Scenario: Ownership cannot be restored
- **WHEN** the shim cannot rebind before lease expiry
- **THEN** it exits instead of continuing as an apparently managed `caddy run` process

### Requirement: REG-004: Domain ownership conflicts fail atomically
The daemon SHALL canonicalize every registered host using lowercase IDNA form without a trailing dot and MUST allow at most one active entrypoint in the installation runtime to own a canonical host. A conflicting candidate MUST fail as a whole while the previously active registration and effective Caddy configuration remain unchanged.

#### Scenario: Two projects claim one host
- **WHEN** a second entrypoint attempts to register a host owned by another active entrypoint
- **THEN** the daemon rejects the complete candidate and identifies the conflict

### Requirement: REG-005: Shim command policy is explicit
The compatibility shim SHALL manage only `caddy run`. It SHALL delegate the read-only `version`, `help`, `list-modules`, `validate`, and `adapt` commands to the trusted real-Caddy installation, identify delegation on stderr, and preserve delegated stdout and exit status. It SHALL reject `start`, `stop`, `reload`, and unknown mutating commands with guidance to use the Cadder TUI.

#### Scenario: Read-only command
- **WHEN** a project invokes a supported read-only Caddy command through the shim
- **THEN** the shim delegates to the independently resolved and pinned real Caddy

#### Scenario: Caddy lifecycle mutation
- **WHEN** a project invokes `caddy start`, `caddy stop`, or `caddy reload`
- **THEN** the shim rejects the command before contacting the real Caddy Admin API
- **AND** the diagnostic directs the operator to `cadder tui`

### Requirement: REG-008: Detachment removes only accepted ownership
The owning shim SHALL request detachment during controlled exit, and the daemon SHALL remove an expired or detached live registration only after the corresponding Caddy configuration transaction succeeds. A snapshot MUST distinguish desired activation, lease state, and configuration status. Rejected candidates SHALL appear only in their response and current diagnostics.

#### Scenario: Controlled shim exit
- **WHEN** the owning shim receives a supported termination signal
- **THEN** it requests detachment and waits for the daemon's bounded terminal outcome

#### Scenario: Removal apply fails
- **WHEN** removing a registration would produce a Caddy configuration that fails validation or application
- **THEN** the daemon retains the last verified configuration and reports removal-failed recovery guidance

### Requirement: REG-009: Stable entrypoint identity separates desired state from leases
The daemon SHALL derive a stable `EntrypointKey` from the canonical project root and canonical Caddyfile path. Editing that Caddyfile SHALL retain the same key and enter normal replacement validation. The daemon MUST persist the public registration ID and desired entrypoint and domain activation under that key while binding each live registration to a daemon-instance lease.

A daemon restart SHALL restore the durable identity and desired activation but MUST NOT serve project routes until a matching shim establishes a new lease. The runtime owner MAY enable or disable a live entrypoint or domain through the TUI without gaining lease ownership. A disabled entrypoint retains its stable identity and desired activation while releasing active host ownership. Cadder 1.0 SHALL NOT expose forget operations or query tombstones.

#### Scenario: Shim reconnects after daemon restart
- **WHEN** a matching shim registers the same canonical project root and Caddyfile after restart
- **THEN** the daemon issues a new instance-bound lease for the durable entrypoint
- **AND** it restores only routes allowed by the persisted desired activation

#### Scenario: Operator disables one domain
- **WHEN** the runtime owner disables an active domain through the TUI
- **THEN** the daemon persists the disabled desired state and removes that route transactionally

#### Scenario: Re-enable conflicts with another owner
- **WHEN** the operator enables a disabled domain now owned by another entrypoint
- **THEN** the operation fails without changing either entrypoint

## REMOVED Requirements

### Requirement: REG-007: Shim alias setup preserves existing Caddy installations
**Reason**: The portable archive ships the compatibility binary under its final PATH-facing name `caddy`; Cadder 1.0 has no alias-management command or installer provenance workflow.

**Migration**: Place the extracted Cadder directory on PATH and configure the independently installed real Caddy through the trusted runtime configuration.
