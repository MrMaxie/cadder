## ADDED Requirements

### Requirement: INSPECT-001: Port inspection correlates sockets and registrations
The operator CLI SHALL inspect a requested local port without starting the daemon, SHALL report every visible local socket owner, and SHALL report every Cadder domain whose syntactically local upstream uses that port when daemon state is available.

#### Scenario: Registered upstream is listening
- **WHEN** a developer inspects a port used by a registered local upstream with a visible process owner
- **THEN** Cadder reports the socket, process, project, Caddyfile, domain, upstream, and their independent activation states

#### Scenario: Daemon is unavailable
- **WHEN** a developer inspects a local port while `cadderd` is unavailable
- **THEN** Cadder reports the visible socket owners and states that Cadder registration data is unavailable without starting the daemon

#### Scenario: Remote upstream has the same port
- **WHEN** a registered upstream uses the requested port on a non-local host
- **THEN** Cadder does not attribute that upstream to the local port owner

### Requirement: INSPECT-002: Caddyfile inspection exposes effective relationships
The operator CLI SHALL accept a Caddyfile path and report whether it is registered, each matching project's activation, its registered domains and upstreams, Caddy runtime and configuration states, and visible owners of its local upstream ports.

#### Scenario: Active registered Caddyfile
- **WHEN** a developer inspects a Caddyfile registered by an enabled project
- **THEN** Cadder reports the matching project and each domain without collapsing registration, activation, runtime, configuration, and listener ownership into one ambiguous state

#### Scenario: Caddyfile is not registered
- **WHEN** the normalized path does not match any current registration
- **THEN** Cadder reports that the Caddyfile is not registered

### Requirement: INSPECT-003: Process termination is explicit and revalidated
The operator CLI MUST require an expected PID before terminating a process associated with a port and MUST revalidate that the PID currently owns the requested port immediately before signaling it.

#### Scenario: Expected owner still matches
- **WHEN** the requested PID still owns the port and the operating system accepts the termination signal
- **THEN** Cadder reports that the signal was sent to that PID

#### Scenario: Port ownership changed
- **WHEN** the requested PID no longer owns the port at revalidation time
- **THEN** Cadder refuses to signal any process and returns a conflict result

#### Scenario: Multiple owners are visible
- **WHEN** more than one PID owns sockets on the requested port
- **THEN** Cadder signals only the explicitly supplied PID after revalidation

### Requirement: INSPECT-004: Local process control does not expand daemon ownership
Socket inspection and process termination SHALL execute only in the explicitly invoked operator process and SHALL NOT cause `cadderd` to enumerate or terminate unrelated processes.

#### Scenario: Developer kills an upstream process
- **WHEN** a developer invokes the guarded port-owner kill command
- **THEN** the `cadder` process performs the check and signal while daemon process ownership remains limited to the real Caddy process it started
