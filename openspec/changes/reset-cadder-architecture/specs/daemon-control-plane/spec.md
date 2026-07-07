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

#### Scenario: Incompatible client sends a request
- **WHEN** a client uses an unsupported protocol or required capability
- **THEN** `cadderd` SHALL reject the request with a typed compatibility error
- **AND** the error SHALL include upgrade or downgrade guidance where possible

### Requirement: Client access to privileged runtime is explicit
When the daemon or helper runs with higher privilege than the caller, user-level
clients SHALL discover and contact it through a documented local IPC security
policy without requiring the whole client process to run with elevated
privileges.

#### Scenario: User client contacts privileged runtime
- **WHEN** a same-user non-elevated `cadder` client starts while the runtime endpoint has higher privileges
- **THEN** the client SHALL discover the supported endpoint
- **AND** the local IPC policy SHALL allow only the documented principals and operations

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

#### Scenario: Active incompatible runtime
- **WHEN** a lock is owned by an active incompatible runtime
- **THEN** `cadderd` SHALL refuse to start a competing production runtime
- **AND** it SHALL report the active runtime identity and recovery options
