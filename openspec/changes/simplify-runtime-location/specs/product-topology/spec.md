## MODIFIED Requirements

### Requirement: TOP-002: The daemon owns managed runtime state
`cadderd` SHALL be the sole source of truth for Cadder-managed registrations, effective Caddy configuration, process ownership, runtime status, history, and logs within the portable release runtime rooted beside the installed Cadder executables.

The daemon SHALL load `cadder.toml` from that runtime root before consulting per-user, system, or generic PATH sources for real Caddy. The portable `[caddy]` table SHALL accept exactly one of `real_command`, a single program name resolved from PATH, or `real_path`, an absolute path to the real Caddy executable.

#### Scenario: Client changes runtime state
- **WHEN** the shim, CLI, or TUI requests a state-changing operation
- **THEN** the client sends the operation through the local control plane
- **AND** the client does not maintain or apply an independent runtime state

#### Scenario: Client reconnects
- **WHEN** a client reconnects after missing state changes
- **THEN** it replaces local presentation state with a fresh daemon snapshot

#### Scenario: Release executables share one runtime root
- **WHEN** a portable release runs its operator, daemon, and shim from the same release directory
- **THEN** each process resolves that directory as the same runtime root

#### Scenario: Portable configuration selects real Caddy
- **WHEN** `cadder.toml` beside `cadderd` sets `caddy.real_command` to `caddy-real`
- **THEN** the daemon resolves that program from PATH before generic Caddy discovery
- **AND** it does not infer `caddy-real` when the portable configuration does not select it
