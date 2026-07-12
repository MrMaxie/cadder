# product-topology Specification

## Purpose
Define the Cadder 1.0 processes, ownership boundaries, supported operating systems, shared contracts, and intentionally excluded product surfaces.

## Requirements
### Requirement: TOP-001: Cadder exposes three release surfaces
Cadder 1.0 MUST expose a daemon named `cadderd`, a Caddy compatibility shim distributed as `cadder-caddy`, and one operator executable named `cadder` that contains CLI and TUI workflows.

#### Scenario: Installed release surface
- **WHEN** a user inspects an installed Cadder 1.0 distribution
- **THEN** the distribution contains `cadderd`, `cadder-caddy`, and `cadder`
- **AND** no separate operator, Web server, Tauri application, remote control API, or MCP product surface is installed or started

#### Scenario: Operator invocation
- **WHEN** a user invokes `cadder` without a subcommand
- **THEN** the operator prints command help
- **AND** the fullscreen interface starts only through `cadder tui`

### Requirement: TOP-002: The daemon owns managed runtime state
`cadderd` SHALL be the sole source of truth for Cadder-managed registrations, effective Caddy configuration, process ownership, runtime status, history, and logs within one runtime profile.

#### Scenario: Client changes runtime state
- **WHEN** the shim, CLI, or TUI requests a state-changing operation
- **THEN** the client sends the operation through the local control plane
- **AND** the client does not maintain or apply an independent runtime state

#### Scenario: Client reconnects
- **WHEN** a client reconnects after missing state changes
- **THEN** it replaces local presentation state with a fresh daemon snapshot

### Requirement: TOP-003: Core behavior is cross-platform
Cadder SHALL provide its daemon, shim, CLI, TUI, local control plane, Caddy coordination, storage, diagnostics, and installation lifecycle on Windows x64, Linux x64, macOS x64, and macOS arm64.

#### Scenario: Platform release matrix
- **WHEN** Cadder 1.0 is released
- **THEN** each supported platform has an installable artifact containing the same core product surfaces

#### Scenario: Platform-specific capability
- **WHEN** a capability exists only on one operating system
- **THEN** the shared product reports that capability as unsupported elsewhere
- **AND** core daemon, shim, CLI, and TUI behavior remains available

### Requirement: TOP-004: Cadder publishes reusable local contracts
Cadder 1.0 SHALL publish the daemon protocol schemas, capability identifiers, and shared presentation contracts used by the `cadder` operator. The operator MUST consume those contracts through the local control plane instead of reading daemon storage or inferring runtime state from processes.

#### Scenario: Operator contract inspection
- **WHEN** a release candidate generates its public protocol and view-model reference
- **THEN** the generated schemas match the contracts consumed by the CLI and TUI
- **AND** the release gate fails if generated and checked-in contracts differ

#### Scenario: Version 1.0 product scope
- **WHEN** a user installs Cadder 1.0
- **THEN** no Web server, Tauri application, remote control API, or MCP product surface is installed or started
