## ADDED Requirements

### Requirement: Daemon owns external state
`cadderd` SHALL be the only Cadder component that owns durable runtime state,
generated Caddy config, Caddy process lifecycle, and OS integration state.

#### Scenario: Applying project definitions
- **WHEN** a project definition changes through the shim or operator client
- **THEN** `cadderd` SHALL update Cadder state first
- **AND** `cadderd` SHALL regenerate and apply the effective Caddy config from that state

#### Scenario: External Caddy process exists
- **WHEN** an unrelated Caddy process is already running
- **THEN** `cadderd` SHALL NOT enumerate, terminate, or mutate that process
- **AND** diagnostics SHALL distinguish unmanaged Caddy from Cadder-owned Caddy

### Requirement: Local IPC is stable and version tolerant
The daemon control plane SHALL use a local protocol with typed envelopes,
request/response messages, events, and errors that tolerate additive compatible
changes.

#### Scenario: Older compatible client sends a request
- **WHEN** an older compatible client sends a request with known required fields
- **THEN** `cadderd` SHALL process the request even if newer optional fields exist in the current schema
- **AND** the response SHALL include capability information needed by the client

#### Scenario: Newer compatible client sends a request
- **WHEN** a newer client sends a request whose envelope can be decoded
- **AND** every required capability and capability version is supported by `cadderd`
- **THEN** `cadderd` SHALL process the request
- **AND** it SHALL NOT reject the request solely because the client's protocol version is newer

#### Scenario: Older node participates in communication
- **WHEN** one communication participant advertises an older protocol or capability version
- **THEN** the newer participant SHALL suggest updating the older node where useful
- **AND** communication SHALL continue when all required capability versions are covered

#### Scenario: Incompatible client sends a request
- **WHEN** a client uses an unsupported protocol or required capability
- **THEN** `cadderd` SHALL reject the request with a typed compatibility error
- **AND** the error SHALL include upgrade or downgrade guidance where possible

### Requirement: Capabilities are versioned independently
Cadder protocol participants SHALL advertise capabilities with a current
capability version and the oldest compatible capability version still supported.

#### Scenario: Required capability version is supported
- **WHEN** a client requires a capability version within the advertised compatible range
- **THEN** the receiver SHALL treat the capability as supported
- **AND** it SHALL allow the operation even when the peer protocol version differs

#### Scenario: Required capability version is unsupported
- **WHEN** a client requires a capability version outside the advertised compatible range
- **THEN** the receiver SHALL reject the operation with a typed unsupported-capability error
- **AND** the error SHALL identify the required capability version and advertised supported versions

### Requirement: Client access to privileged runtime is explicit
User-level clients SHALL discover and contact a daemon or helper that runs with
higher privilege than the caller through a documented local IPC security policy
without requiring the whole client process to run with elevated privileges.
The daemon SHALL publish endpoint discovery metadata that includes runtime
identity, socket identity, protocol compatibility, advertised capabilities,
endpoint privilege status, owning principal, and the policy version used for
local authorization.

#### Scenario: User client contacts privileged runtime
- **WHEN** a same-user non-elevated `cadder` client starts while the runtime endpoint has higher privileges
- **THEN** the client SHALL discover the supported endpoint
- **AND** the local IPC policy SHALL allow only the documented principals and operations

#### Scenario: Runtime endpoint metadata is published
- **WHEN** `cadderd` binds the local IPC listener for a runtime
- **THEN** it SHALL publish discovery metadata in the runtime directory
- **AND** the metadata SHALL describe the socket name, runtime profile, protocol compatibility range, capabilities, privilege status, owning principal, and security policy version

#### Scenario: Unauthorized user contacts daemon
- **WHEN** a different unauthorized user tries to contact the daemon endpoint
- **THEN** the IPC layer SHALL deny access before state-changing commands are executed
- **AND** the daemon SHALL log the denied attempt without leaking secrets

#### Scenario: IPC policy is tested without OS mutation
- **WHEN** the IPC security policy is tested
- **THEN** allowed and denied principals SHALL be covered without requiring real platform mutation in every test
- **AND** platform smoke tests SHALL cover the real transport behavior separately

### Requirement: Daemon lifecycle is user friendly
CLI, TUI, and shim flows SHALL detect daemon-unavailable states and return
actionable recovery guidance instead of opaque connection failures.

#### Scenario: State-changing command while daemon is down
- **WHEN** a user runs a state-changing `cadder` command and no daemon is reachable
- **THEN** the client SHALL explain that `cadderd` is not running
- **AND** it SHALL offer the supported start command or TUI action

#### Scenario: Read-only command while daemon is down
- **WHEN** a user runs a read-only command and no daemon is reachable
- **THEN** the client SHALL show daemon-unavailable status
- **AND** it MAY show local setup information or last-known cached state when available

### Requirement: Runtime locking has stale recovery
Production daemon startup SHALL use a lock mechanism with explicit ownership,
diagnostics, and stale-lock recovery.

#### Scenario: Stale lock after crash
- **WHEN** a daemon lock exists but the recorded owner is no longer alive
- **THEN** `cadderd` SHALL recover or replace the stale lock through a documented process
- **AND** it SHALL record the recovery in daemon logs

#### Scenario: Lock owner metadata is readable while lock is active
- **WHEN** a production runtime owns the daemon lock
- **THEN** `cadderd` SHALL publish lock owner metadata outside the locked byte range
- **AND** diagnostics SHALL include the owner process id, Cadder version, protocol compatibility range, runtime profile, runtime directory, socket name, acquisition time, and executable path when available

#### Scenario: Active incompatible runtime
- **WHEN** a lock is owned by an active incompatible runtime
- **THEN** `cadderd` SHALL refuse to start a competing production runtime
- **AND** it SHALL report the active runtime identity and recovery options
