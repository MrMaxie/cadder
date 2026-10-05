## ADDED Requirements

### Requirement: Daemon-centered product topology
Cadder SHALL be organized around `cadderd` as the runtime owner, `caddy` as the
PATH-facing shim, and `cadder` as the CLI/TUI operator client.

#### Scenario: Normal runtime ownership
- **WHEN** a project invokes the `caddy` shim to register or update a Caddy definition
- **THEN** the shim SHALL send the request to `cadderd` instead of starting an unmanaged Caddy process
- **AND** `cadderd` SHALL own the resulting runtime state and real Caddy process coordination

#### Scenario: Operator client inspection
- **WHEN** a user opens `cadder` CLI or TUI
- **THEN** the client SHALL read daemon state through the shared Cadder protocol
- **AND** the client SHALL NOT inspect daemon storage or Caddy Admin API state directly

### Requirement: Future operator surfaces use the same client boundary
Future operator surfaces SHALL be clients of the same daemon protocol and
reusable view-model contracts used by the initial operator clients.

#### Scenario: Adding a future operator surface
- **WHEN** a future change introduces another operator surface
- **THEN** it SHALL attach to `cadderd` through the supported local protocol
- **AND** it SHALL NOT create an independent runtime state model or Caddy control plane

### Requirement: Workspace boundaries are minimal and explainable
The Rust workspace SHALL keep product crates and library crates aligned with the
target topology and SHALL require each crate to have a documented, testable
responsibility.

#### Scenario: Contributor evaluates a crate
- **WHEN** a crate is reviewed during the reset
- **THEN** it SHALL be classified as daemon, shim, operator client, shared protocol/API, test support, documentation/tooling, or obsolete
- **AND** crates without a current responsibility SHALL be removed, merged, or explicitly justified

#### Scenario: New crate proposed
- **WHEN** a new crate is proposed
- **THEN** the proposal SHALL explain why a module inside an existing crate is insufficient
- **AND** the crate SHALL have independently testable responsibilities

### Requirement: Production runtime identity is singular
Production Cadder SHALL run one stable runtime identity per user or per explicit
system installation. Multiple Cadder runtimes SHALL be limited to dev/debug
profiles with separate runtime directories and visible labels.

#### Scenario: Production daemon already running
- **WHEN** a production `cadderd` instance starts and a compatible production runtime already owns the lock
- **THEN** the new process SHALL attach, report the existing runtime, or exit with a clear diagnostic
- **AND** it SHALL NOT create a second production runtime silently

#### Scenario: Developer starts isolated runtime
- **WHEN** a developer starts Cadder with an explicit dev/debug runtime profile
- **THEN** Cadder SHALL use an isolated runtime directory and lock identity
- **AND** clients and logs SHALL display the active runtime profile
