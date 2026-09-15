## MODIFIED Requirements

### Requirement: TOP-001: Cadder exposes three release surfaces
Cadder 1.0 MUST expose exactly three executable entrypoints: the foreground-capable daemon `cadderd`, the PATH-facing Caddy compatibility shim `caddy`, and the operator executable `cadder`. The operator SHALL expose help, version, and the full-screen TUI only.

#### Scenario: Portable release surface
- **WHEN** a user inspects a Cadder 1.0 portable archive
- **THEN** the archive contains the platform-appropriate forms of `cadderd`, `caddy`, and `cadder`
- **AND** no separate operator CLI commands, Web server, Tauri application, remote control API, MCP surface, installer, or updater is included

#### Scenario: Operator invocation
- **WHEN** a user invokes `cadder` without a subcommand
- **THEN** the operator prints concise help
- **AND** the full-screen interface starts only through `cadder tui`

### Requirement: TOP-002: The daemon owns managed runtime state
`cadderd` SHALL be the sole source of truth for Cadder-managed registrations, desired activation, effective Caddy configuration, process ownership, runtime status, and logs for its installation runtime.

#### Scenario: Client changes runtime state
- **WHEN** the shim or TUI requests a state-changing operation
- **THEN** the client sends the operation through the authenticated local control plane
- **AND** the client does not maintain or apply independent runtime state

#### Scenario: Client reconnects
- **WHEN** the TUI reconnects after losing the daemon connection
- **THEN** it replaces stale presentation state with a fresh bounded daemon snapshot

### Requirement: TOP-003: Core behavior is cross-platform
Cadder SHALL provide its daemon, PATH-facing shim, TUI, local control plane, Caddy coordination, storage, diagnostics, and portable archive lifecycle on Windows x64, Linux x64, macOS x64, and macOS arm64.

#### Scenario: Platform release matrix
- **WHEN** Cadder 1.0 is released
- **THEN** each supported platform has a portable archive containing the same three executable entrypoints and core behavior

#### Scenario: Platform-specific implementation
- **WHEN** endpoint security or process ownership requires operating-system-specific behavior
- **THEN** Cadder preserves the same user-visible managed-run and TUI outcomes through a narrow platform abstraction

## REMOVED Requirements

### Requirement: TOP-004: Cadder publishes reusable local contracts
**Reason**: Cadder 1.0 has two in-tree clients and no public SDK, generated schema product, Web client, or Tauri client. Publishing capability and presentation contracts would preserve speculative compatibility machinery.

**Migration**: The shim and TUI use the same private typed local client boundary. A public client contract requires a future change with a concrete external audience.
