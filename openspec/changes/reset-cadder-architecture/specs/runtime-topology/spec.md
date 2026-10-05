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
The TypeScript product SHALL keep protocol, daemon, Caddy adapter, platform
adapters, client service, CLI and TUI modules aligned with the target topology.
Rust crates remain only as a migration baseline until the release gates pass.

#### Scenario: Contributor evaluates a module
- **WHEN** a module is reviewed during the reset
- **THEN** it SHALL be classified as daemon, shim, operator client, shared protocol/API, test support, documentation/tooling, or obsolete
- **AND** modules without a current responsibility SHALL be removed, merged, or explicitly justified

#### Scenario: New module proposed
- **WHEN** a new module is proposed
- **THEN** the proposal SHALL explain its independent responsibility
- **AND** it SHALL NOT introduce a new public product or native runtime addon

### Requirement: Distribution channels are equal
Cadder SHALL ship one npm package on Node 24 LTS >=24.18.0 and standalone Node SEA
archives through GitHub Releases. Both SHALL provide the same three entrypoints
and CLI/TUI functionality. Both channels SHALL pass before migration completion.

#### Scenario: Packaging release sources
- **WHEN** npm or SEA artifacts are prepared
- **THEN** TypeScript sources SHALL compile only into staging outside checkout
- **AND** generated JavaScript and binaries SHALL NOT be committed

#### Scenario: Standalone consumer has no Node
- **WHEN** a supported standalone archive is used without Node/npm on PATH
- **THEN** all entrypoints, TUI, SQLite and owned subprocess flows SHALL work

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
