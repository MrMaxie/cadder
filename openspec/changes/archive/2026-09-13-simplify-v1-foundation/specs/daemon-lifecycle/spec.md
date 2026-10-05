## MODIFIED Requirements

### Requirement: RUN-003: A profile admits one live daemon
Each runtime profile MUST admit only one live daemon by exclusively owning its local IPC endpoint for the daemon lifetime.

#### Scenario: Concurrent start
- **WHEN** two start attempts race for one profile
- **THEN** one daemon acquires the local endpoint
- **AND** the other reports the already-running instance without replacing its state

#### Scenario: Stale Unix endpoint
- **WHEN** the endpoint path remains after its daemon has exited and no owner answers the readiness handshake
- **THEN** Cadder removes only that stale socket and retries the exclusive bind
- **AND** it does not inspect or signal unrelated processes

#### Scenario: Ambiguous ownership
- **WHEN** an existing endpoint cannot be claimed and another daemon cannot be confirmed ready
- **THEN** startup fails closed with recovery guidance

### Requirement: RUN-004: The daemon owns only the Caddy process it starts
`cadderd` MUST track and control only the real Caddy child process created for its own runtime profile. It SHALL retain the child handle and use the cross-platform process-group or job-object support supplied by the process library.

#### Scenario: Managed process shutdown
- **WHEN** the daemon stops normally
- **THEN** it requests graceful termination of its owned Caddy child
- **AND** it waits for or safely escalates that specific child according to the documented timeout

#### Scenario: Unrelated Caddy process
- **WHEN** another Caddy process exists on the machine
- **THEN** Cadder does not enumerate, signal, or terminate that process

#### Scenario: Child handle is unavailable
- **WHEN** Cadder can no longer inspect its retained child handle
- **THEN** it reports a degraded runtime
- **AND** it does not discover or terminate a process by name or an unverified process identifier

### Requirement: RUN-005: Shutdown drains in-flight work
The daemon SHALL stop accepting new mutating requests, cancel or finish owned background work, request bounded shutdown of its owned Caddy child, flush durable state, and release its local endpoint. The normal drain budget SHALL NOT exceed 30 seconds. If a phase exceeds its budget, Cadder SHALL report shutdown failure and continue process teardown instead of waiting indefinitely.

#### Scenario: Stop with active request
- **WHEN** daemon shutdown begins while a bounded request is in progress
- **THEN** the request either completes within its operation deadline or receives a typed cancellation error
- **AND** no new mutation is accepted after shutdown begins

#### Scenario: Storage flush exceeds its phase
- **WHEN** the storage worker does not finish before its shutdown deadline
- **THEN** Cadder reports that shutdown did not complete cleanly and continues process teardown
- **AND** the next start recovers only an incomplete final transaction tail while preserving complete committed records
