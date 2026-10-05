## MODIFIED Requirements

### Requirement: IPC-001: The local endpoint authenticates the runtime owner
The installation runtime MUST expose one owner-only local endpoint and authenticate the peer's operating-system identity before decoding or dispatching a protocol handshake or request. On Windows, each named-pipe connection MUST retain the transport-authentication preface required to bind the accepted client to its operating-system token. A missing, unknown, or different identity SHALL be denied for every operation.

#### Scenario: Same owner connects
- **WHEN** a client from the runtime owner connects through the protected endpoint
- **THEN** the daemon authenticates that owner before reading the protocol frame

#### Scenario: Different owner connects
- **WHEN** a client authenticated as another operating-system user connects
- **THEN** the daemon closes the connection without dispatching a request

### Requirement: IPC-003: Endpoint selection and handshake are deterministic
All three executables from one archive SHALL derive the same local endpoint from their canonical installation directory. Each connection MUST begin with one exact protocol-version handshake after transport authentication. A matching version enters request dispatch; any mismatch returns one typed incompatibility response and closes without mutating state.

#### Scenario: Archive binaries connect
- **WHEN** the TUI or shim connects to `cadderd` from the same archive
- **THEN** both derive the same endpoint and complete the exact-version handshake

#### Scenario: Mixed archive versions connect
- **WHEN** a client protocol version differs from the daemon version
- **THEN** the daemon returns the observed client and daemon versions and closes before dispatch

### Requirement: IPC-004: NDJSON frames are bounded and unambiguous
The local protocol SHALL encode one UTF-8 JSON request or response value per line. JSON bytes before the terminating line feed MUST NOT exceed 1,048,576 bytes, and each connection MUST have at most one active request. The daemon and clients MUST bound incomplete-frame buffering and response frames to the same limit.

#### Scenario: Maximum frame
- **WHEN** a valid request contains exactly 1,048,576 JSON bytes followed by a line feed
- **THEN** the daemon decodes and dispatches it normally

#### Scenario: Oversized or unterminated frame
- **WHEN** a peer exceeds the limit before a line feed or leaves a frame incomplete until its deadline
- **THEN** the daemon stops buffering, returns a typed frame error when safe, and closes the connection

#### Scenario: Pipelined request
- **WHEN** a peer sends a second request before the first terminal response
- **THEN** the daemon rejects the second request without running its handler

### Requirement: IPC-005: Connections and operation time are bounded
The runtime SHALL accept at most 64 simultaneous client connections. A new connection MUST send and complete its first protocol frame within bounded deadlines. Each accepted request MUST complete with one response or typed timeout within its operation deadline, and a stalled client write MUST NOT retain a handler indefinitely.

#### Scenario: Client sends no frame
- **WHEN** an authenticated connection does not begin a protocol frame within five seconds
- **THEN** the daemon closes that connection without affecting other clients

#### Scenario: Request exceeds its deadline
- **WHEN** a bounded operation does not finish within its declared deadline
- **THEN** the daemon returns or records a typed timeout and releases the handler resources

### Requirement: IPC-006: Exact protocol version gates a closed operation set
The protocol SHALL use one exact version and a closed typed operation set: register, unregister, heartbeat, query runtime snapshot, set entrypoint activation, set domain activation, query bounded logs, and shutdown. The daemon MUST reject any unknown operation or version before handler execution. Cadder 1.0 SHALL NOT negotiate capabilities or minor-version compatibility.

#### Scenario: Supported operation
- **WHEN** an exact-version authenticated client sends a valid operation
- **THEN** the daemon dispatches exactly that typed handler

#### Scenario: Unknown operation
- **WHEN** a frame contains an operation outside the closed set
- **THEN** decoding fails with a typed unsupported-operation response and no handler runs

### Requirement: IPC-007: Envelopes and errors preserve correlation
Every request SHALL use one serde-tagged `RequestEnvelope` containing a bounded request ID and exactly one typed operation payload. Every terminal response SHALL use one `ResponseEnvelope` with the same request ID and exactly one typed success payload or typed error. The daemon and clients MUST NOT use a legacy flat envelope or serialize one operation through multiple wire shapes.

#### Scenario: Request succeeds
- **WHEN** a valid request completes
- **THEN** one success response carries the matching request ID and operation result

#### Scenario: Request fails
- **WHEN** authentication, compatibility, validation, authorization, timeout, or handler execution rejects a request
- **THEN** one typed error response preserves the request ID when it was safely decoded

### Requirement: IPC-008: Shared DTOs have controlled schemas
The retained request, response, snapshot, activation, log, and error DTOs SHALL use explicit serde field names, deny unknown fields where forward tolerance is not required, and remain bounded by the framing contract. A runtime snapshot MUST contain at most 128 entrypoints and 1,024 domains and MUST serialize within one response frame. A mutation that would make the snapshot exceed any bound SHALL fail before external apply or durable commit. Internal daemon state, storage rows, presentation rows, mock values, and process handles MUST NOT appear in the wire schema.

#### Scenario: Unknown field is supplied
- **WHEN** a client sends a field outside the exact-version DTO schema
- **THEN** decoding rejects the request before handler execution

### Requirement: IPC-009: Shutdown closes the control plane deterministically
During bounded daemon shutdown, the control plane SHALL stop accepting connections, reject new requests on authenticated existing connections, finish or cancel owned handlers, attempt one terminal response where possible, and release the endpoint within the daemon shutdown deadline.

#### Scenario: New request during shutdown
- **WHEN** a client sends a request after shutdown begins
- **THEN** it receives a typed shutting-down outcome or the connection closes within the same bounded timeline

#### Scenario: Handler stalls during shutdown
- **WHEN** an owned handler exceeds its shutdown phase
- **THEN** the daemon cancels it and continues endpoint teardown without an indefinite wait

## REMOVED Requirements

### Requirement: IPC-010: Minor protocol evolution is additive and explicit
**Reason**: All 1.0 binaries ship as one coherent archive, so pre-1.0 minor-version negotiation adds compatibility code without a supported mixed-version journey.

**Migration**: Use matching binaries from one archive; future independent client distribution requires a new compatibility design.

### Requirement: IPC-011: Authoritative collections use bounded consistent pages
**Reason**: The retained snapshot and log query are bounded responses and do not require snapshot tokens, collection cursors, or page assembly.

**Migration**: Query one bounded snapshot and at most 200 recent log entries through the TUI.

## RENAMED Requirements

- FROM: `### Requirement: IPC-006: Versions and capabilities gate dispatch`
- TO: `### Requirement: IPC-006: Exact protocol version gates a closed operation set`
