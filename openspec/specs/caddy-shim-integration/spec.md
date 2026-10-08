# Caddy shim integration

## Purpose
Define the PATH-facing Caddy shim command and managed runtime contract.

## Requirements

### Requirement: SHIM-001: Shim policy preserves command fidelity
The shim SHALL classify commands as managed, read-only inspection, explicit passthrough or unsupported and preserve argument arrays, standard streams and exit codes. Unknown commands SHALL fail rather than silently pass through.

#### Scenario: Explicit passthrough command exits
- **WHEN** a supported passthrough command completes
- **THEN** the shim SHALL preserve its output streams and exit status
- **AND** it SHALL NOT mutate daemon-managed state through an undocumented path

### Requirement: SHIM-002: Managed run never starts independent Caddy
Managed caddy run SHALL use daemon registration/heartbeat and MAY start a missing packaged daemon. Failure SHALL not become an independent Caddy run. Read-only inspection MAY use safely resolved real Caddy and SHALL identify its result as inspection rather than daemon state.

#### Scenario: Managed startup fails
- **WHEN** the daemon cannot become ready for managed run
- **THEN** the shim SHALL fail with recovery guidance without unmanaged fallback

### Requirement: SHIM-003: Both distributions avoid recursive resolution
Real Caddy resolution SHALL detect npm wrappers, SEA entries and equivalent file/alias identities, preventing recursion in local/global npm and standalone PATH layouts. The real upstream server SHALL remain separately installed.

#### Scenario: Globally installed shim precedes upstream Caddy
- **WHEN** PATH resolves Cadder before upstream Caddy
- **THEN** the shim SHALL skip its own wrappers/entries and find a trusted candidate
- **AND** failure SHALL be bounded without recursively spawning Cadder
