# Local control plane

## Purpose
Define the owner-authenticated exact-version IPC contract.

## Requirements

### Requirement: IPC-001: Local transport is owner authenticated and bounded
Cadder MUST use owner-protected local IPC with peer authentication and newline-delimited JSON frames no larger than 1 MiB.

#### Scenario: Different user connects
- **WHEN** the peer identity does not match the runtime owner
- **THEN** Cadder rejects the connection without disclosing private account data

### Requirement: IPC-002: Handshake requires one exact version
Every connection MUST begin with one exact protocol-version handshake, and a mismatch MUST close before dispatching an operation.

#### Scenario: Mixed binaries
- **WHEN** a client and daemon use different protocol versions
- **THEN** the daemon returns a correlated incompatibility error and performs no mutation

### Requirement: IPC-003: One typed envelope carries eight operations
The protocol SHALL use one typed request envelope and one correlated result-or-error response envelope for register, unregister, heartbeat, query state, set entrypoint activation, set domain activation, query logs, and shutdown.

#### Scenario: Unknown operation or field
- **WHEN** a request names an operation or payload field outside the exact contract
- **THEN** Cadder returns a typed rejection and does not invoke an ad hoc decoder
