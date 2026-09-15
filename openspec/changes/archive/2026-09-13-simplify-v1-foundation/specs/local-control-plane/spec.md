## MODIFIED Requirements

### Requirement: IPC-003: Endpoint selection and handshake are deterministic
Every client SHALL derive the same per-user local endpoint from the selected runtime directory, connect only to that endpoint, and complete the versioned handshake before dispatch. The daemon SHALL generate a fresh instance identifier for each endpoint lease and authenticate the peer independently through the operating-system transport.

#### Scenario: Client attaches
- **WHEN** a client attaches to a selected profile or runtime directory
- **THEN** it derives the selected local endpoint, connects to it, and completes a compatible handshake before sending an operation

#### Scenario: Endpoint is unavailable
- **WHEN** no daemon answers at the derived endpoint
- **THEN** the client reports a typed daemon-unavailable error
- **AND** it does not probe unrelated endpoints or start a daemon implicitly

#### Scenario: Daemon changes during attach
- **WHEN** the endpoint owner changes before the handshake completes
- **THEN** the client accepts operations only after completing a handshake with the current owner
- **AND** a failed or incompatible handshake dispatches no operation

### Requirement: IPC-009: Shutdown closes the control plane deterministically
During the shutdown behavior defined by `RUN-005`, the control plane SHALL use one absolute 30-second timeline to stop accepting new connections, reject new requests on existing connections, attempt a terminal outcome on active streams, and drain or cancel owned handlers. A stalled client, handler, rollback, or storage operation MUST NOT extend shutdown indefinitely. Exceeding a phase SHALL produce a shutdown failure while process teardown releases the local endpoint.

#### Scenario: Handler does not drain
- **WHEN** an in-flight handler exceeds its assigned shutdown phase
- **THEN** the daemon cancels the task, makes one bounded join attempt, and reports shutdown failure if the task still does not finish
- **AND** process teardown ends any remaining in-process work
