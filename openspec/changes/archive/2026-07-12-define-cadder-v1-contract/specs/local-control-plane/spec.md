## ADDED Requirements

### Requirement: IPC-001: The local endpoint authenticates the runtime owner
Each runtime profile MUST expose one owner-only local endpoint and MUST authenticate the peer's operating-system identity before reading or dispatching a request. A missing, unknown, or different identity SHALL be denied for every operation.

#### Scenario: Unix owner connects
- **WHEN** the runtime owner connects on Linux or macOS
- **THEN** Cadder accepts the connection through a filesystem-domain socket inside a `0700` owner directory with socket mode `0600`
- **AND** the daemon confirms that the peer UID equals its effective UID

#### Scenario: Windows owner connects
- **WHEN** the runtime owner connects on Windows
- **THEN** the named pipe DACL grants access only to the owner's SID
- **AND** the daemon confirms the peer SID from the impersonated pipe-client token rather than trusting a PID or account display name

#### Scenario: Another principal reaches the endpoint
- **WHEN** a different UID, SID, or unauthenticated peer attempts to connect
- **THEN** the operating-system ACL or the daemon denies the peer before request dispatch
- **AND** no read-only or mutating handler runs

### Requirement: IPC-002: Elevated helpers cannot inherit the control plane
Only the ordinary user daemon defined by `IIS-003` SHALL expose the normal control-plane endpoint. An elevated IIS helper MUST NOT inherit, publish, or reuse that endpoint and MUST NOT accept general protocol requests.

#### Scenario: IIS helper runs
- **WHEN** the one-shot IIS helper starts with elevation
- **THEN** it accepts only the authenticated immutable IIS plan channel
- **AND** it does not publish `cadder-ipc.json` or a general Cadder endpoint

#### Scenario: Control-plane handles exist during helper launch
- **WHEN** the normal daemon launches the one-shot IIS helper
- **THEN** control-plane listener and client handles are not inherited by the elevated process

### Requirement: IPC-003: Discovery is authoritative but not an authentication secret
The daemon SHALL atomically publish an owner-only `cadder-ipc.json` containing its metadata schema version, profile, runtime identifier, instance identifier, endpoint, supported protocol range, capabilities, and diagnostic PID. Every client MUST validate and consume this record before connecting, then confirm the instance identifier during the protocol handshake. Possession of the file MUST NOT authorize a client.

#### Scenario: Client attaches
- **WHEN** a client attaches to a selected profile or runtime directory
- **THEN** it reads the discovery record, validates its schema and profile, connects to the published endpoint, and confirms the same daemon instance

#### Scenario: Discovery is stale or malformed
- **WHEN** discovery is malformed, names a missing endpoint, or presents an instance identifier different from the connected daemon
- **THEN** the client reports a typed stale-discovery or invalid-discovery error
- **AND** it does not guess a derived endpoint or send the requested operation elsewhere

#### Scenario: Old daemon exits after replacement
- **WHEN** an older daemon instance cleans up after a replacement has published a newer generation
- **THEN** the old instance leaves the newer discovery record unchanged

### Requirement: IPC-004: NDJSON frames are bounded and unambiguous
The local protocol SHALL encode one UTF-8 JSON value per line. The JSON bytes before the terminating line feed MUST NOT exceed 1,048,576 bytes, and each connection MUST have at most one active request. The daemon and clients MUST bound incomplete-frame buffering and response frames to the same limit.

#### Scenario: Maximum frame
- **WHEN** a valid request contains exactly 1,048,576 JSON bytes followed by a line feed
- **THEN** the daemon decodes and dispatches it normally

#### Scenario: Oversized or unterminated frame
- **WHEN** a peer exceeds 1,048,576 bytes before a line feed or leaves a frame incomplete until the request deadline
- **THEN** the daemon stops buffering, returns a typed frame error when a safe response is possible, and closes the connection

#### Scenario: Pipelined request
- **WHEN** a peer sends a second request while the first request or subscription setup remains active
- **THEN** the daemon rejects the second request as a protocol violation without running its handler

### Requirement: IPC-005: Connections and operation time are bounded
A runtime profile SHALL accept at most 64 simultaneous client connections, including streams and incomplete connections. A new connection MUST send its first frame byte within five seconds of acceptance and complete that frame within 30 seconds of the first byte. After a complete valid frame is accepted, an ordinary handler and its terminal response MUST finish within 30 seconds; any individual response write that makes no progress for five seconds SHALL fail the request within that deadline. A configuration reload's advertised handler deadline begins when its complete frame is accepted and MUST NOT exceed 120 seconds.

A negotiated stream MAY remain open beyond 30 seconds with at most 256 queued records and 524,288 queued serialized bytes. It SHALL emit a heartbeat control record after 15 seconds without a data record, enforce a five-second no-progress deadline for each write, and observe cancellation at least once per second while otherwise idle. Queue overflow MUST produce an explicit sequence gap when the connection remains writable; a stalled connection that cannot receive that outcome within its write deadline MUST close. Connection permits, queued records, and tracked tasks MUST be released after close or cancellation.

#### Scenario: Connection capacity is exhausted
- **WHEN** 64 accepted connections are active and another client connects
- **THEN** the daemon returns a retryable busy outcome when it can frame one safely or rejects the connection
- **AND** accepted connections keep their existing service guarantees

#### Scenario: Ordinary handler exceeds its deadline
- **WHEN** an ordinary request has no terminal result within 30 seconds
- **THEN** the daemon cancels its owned work and returns a retryable timeout error
- **AND** the timed-out work cannot mutate state later

#### Scenario: Reload advertises an extended deadline
- **WHEN** a compatible client starts a configuration reload whose negotiated capability advertises an extended deadline
- **THEN** both peers enforce the same deadline of at most 120 seconds

#### Scenario: Stream remains idle
- **WHEN** a negotiated stream has no data records for 15 seconds
- **THEN** the daemon emits a heartbeat control record without adding it to durable event history
- **AND** a cancellation closes the idle stream within one second and releases its connection permit

#### Scenario: Slow peer never completes a frame
- **WHEN** an accepted peer sends no first byte within five seconds or does not finish its frame within 30 seconds of the first byte
- **THEN** the daemon closes the connection without dispatching a handler

#### Scenario: Stream queue reaches its bound
- **WHEN** a producer would exceed 256 queued records or 524,288 queued bytes for one stream
- **THEN** the daemon replaces the omitted range with an explicit gap outcome when it can write one within five seconds
- **AND** it otherwise closes that stalled stream without affecting other clients

### Requirement: IPC-006: Versions and capabilities gate dispatch
Every handshake SHALL exchange a `ProtocolVersion` with major and minor components and a set of stable `CapabilityId` values. The dispatcher MUST verify the operation's minimum version and required capabilities before invoking its handler. Equal major versions SHALL interoperate only for operations supported by both peers; different major versions MUST fail as incompatible.

#### Scenario: Older client uses a retained operation
- **WHEN** an older-minor client connects to a newer daemon with the same protocol major and requests a capability both advertise
- **THEN** the daemon processes the operation using that operation's retained wire contract

#### Scenario: New client requests an unavailable operation
- **WHEN** a client requests an operation whose capability or minimum minor version the daemon does not advertise
- **THEN** the dispatcher returns an unsupported-capability or incompatible-version error before the operation handler runs

#### Scenario: Protocol major differs
- **WHEN** client and daemon protocol majors differ
- **THEN** the handshake ends with a protocol-incompatible outcome and upgrade guidance

### Requirement: IPC-007: Envelopes and errors preserve correlation
Each request SHALL carry its protocol version, operation, `RequestId`, and typed payload. Each response MUST echo the request ID and contain exactly one typed result or one `ProtocolError`. `ProtocolError` SHALL expose `kind`, stable `code`, `message`, `guidance`, `retryable`, and `request_id`, and every Cadder client MUST preserve those fields without reducing them to an unstructured string.

#### Scenario: Handler rejects a request
- **WHEN** a handler returns a permission, conflict, configuration, Caddy, storage, or internal failure
- **THEN** the response carries the same request ID and the corresponding stable error fields

#### Scenario: Client cannot receive a protocol response
- **WHEN** discovery, connection, framing, EOF, or local timeout fails before a response arrives
- **THEN** the client returns a typed local transport error distinct from a daemon `ProtocolError`

#### Scenario: Retry guidance is consumed
- **WHEN** an error is marked retryable
- **THEN** clients retain its code and request ID while applying only the bounded retry policy for that operation

### Requirement: IPC-008: Shared DTOs have controlled schemas
The protocol SHALL publish versioned schemas for `RuntimeSnapshot`, `PageEnvelope`, `EntrypointSnapshot`, `DomainSnapshot`, `CaddyConfigStatus`, `LogEvent`, `HistoryEvent`, `IisChangePlan`, and `IisApplyResult`. CLI and TUI MUST consume these shared contracts rather than infer daemon state from storage or process inspection. The published schemas and capability metadata SHALL remain available to additional local clients without installing another runtime service.

#### Scenario: Schema-compatible client
- **WHEN** a local client decodes a snapshot that uses its supported schema version and contains additional declared-compatible fields
- **THEN** it consumes the known fields and ignores or preserves additive optional fields according to the published schema

#### Scenario: Breaking DTO version
- **WHEN** a client receives a DTO schema version it cannot safely interpret
- **THEN** it rejects the payload with protocol-incompatibility guidance instead of presenting partial state as authoritative

#### Scenario: Published schemas drift
- **WHEN** generated JSON Schemas differ from the checked-in public protocol contract
- **THEN** the release validation fails

### Requirement: IPC-009: Shutdown closes the control plane deterministically
During the shutdown behavior defined by `RUN-005`, the control plane SHALL stop accepting new connections, reject new requests on existing connections, attempt a terminal outcome on active streams, and drain or cancel owned handlers within 30 seconds. Discovery metadata and the endpoint MUST remain until terminal outcomes are attempted and MUST be removed only by the owning daemon generation. A stalled or disconnected client MUST NOT extend the shutdown deadline.

#### Scenario: New request during shutdown
- **WHEN** an authenticated client sends a request after shutdown begins
- **THEN** the daemon returns a non-retryable shutting-down error without invoking the handler

#### Scenario: Active subscription during shutdown
- **WHEN** shutdown begins while a responsive client holds an active state or log subscription
- **THEN** the stream receives a terminal shutdown outcome and closes

#### Scenario: Subscription does not accept the terminal outcome
- **WHEN** a stream is stalled or disconnects during terminal delivery
- **THEN** the daemon ends the bounded delivery attempt and continues shutdown
- **AND** endpoint cleanup does not wait beyond the 30-second grace

#### Scenario: Handler does not drain
- **WHEN** an in-flight handler exceeds the 30-second shutdown grace
- **THEN** the daemon cancels and joins its owned task before process exit
- **AND** no detached task can mutate the runtime after endpoint cleanup

### Requirement: IPC-010: Minor protocol evolution is additive and explicit
Within one protocol major version, a retained operation SHALL evolve only by adding optional response fields with documented defaults. Clients MUST ignore unknown additive response fields and unknown `CapabilityId` values while preserving every field they understand. Required fields, field meaning, operation names, error codes, and existing enum or union discriminators MUST remain stable throughout the major version.

Mutation request payloads SHALL use closed schemas. A new request field, enum variant, or union variant MUST use a new capability identifier and minimum minor version; an older daemon MUST reject the unavailable capability before parsing or dispatching that payload. Removing a field, changing a required field, reusing a discriminator, or changing existing field meaning MUST increment the protocol major version.

#### Scenario: New daemon adds an optional response field
- **WHEN** an older-minor client receives a retained response with a new optional field and both peers advertise the operation capability
- **THEN** the client ignores the unknown field, preserves all known fields, and presents an authoritative result

#### Scenario: New client sends an extended mutation
- **WHEN** a new client sends a mutation variant gated by a capability the older daemon does not advertise
- **THEN** the daemon returns unsupported-capability before parsing the extended payload
- **AND** no mutation handler runs

#### Scenario: Unknown enum variant lacks a capability gate
- **WHEN** a peer receives an unknown enum or union discriminator for a retained contract without an advertised extension capability
- **THEN** it rejects the payload as protocol-incompatible instead of mapping it to an existing meaning

#### Scenario: Breaking wire change is proposed
- **WHEN** a generated schema removes a field, adds an ungated required field, or changes an existing discriminator or meaning within the current major version
- **THEN** compatibility validation fails before release

### Requirement: IPC-011: Authoritative collections use bounded consistent pages
`RuntimeSnapshot` SHALL be a bounded manifest containing the runtime generation, scalar status, collection counts, and a 30-second opaque snapshot token; it MUST NOT embed unbounded entrypoint, domain, log, or history collections. `PageEnvelope` SHALL contain `schemaVersion`, `collection`, `generation`, nullable `snapshotToken`, `items`, nullable `nextCursor`, and `complete`. Each page MUST contain at most 256 items and 524,288 serialized bytes so the complete response remains below the `IPC-004` frame limit. An individual snapshot item MUST NOT exceed 262,144 serialized bytes; a mutation that would create a larger item SHALL fail as configuration-too-large.

Every entrypoint and domain page for one snapshot token MUST represent the same immutable runtime generation even if later mutations commit. The daemon SHALL expire the token after 30 seconds and return a typed snapshot-expired result instead of mixing generations. Log and history queries SHALL use the same page envelope without a snapshot token and continue through their ordered cursors under `OBS-010`.

#### Scenario: Runtime contains many entrypoints
- **WHEN** an authoritative snapshot cannot fit in one response frame
- **THEN** the client receives a bounded manifest and follows entrypoint and domain pages with the same snapshot token and generation
- **AND** it treats the assembled state as authoritative only after every page is complete

#### Scenario: Snapshot expires between pages
- **WHEN** a client requests another page after the 30-second snapshot token expires
- **THEN** the daemon returns snapshot-expired without returning mixed-generation items
- **AND** the client discards the incomplete assembly and starts a fresh snapshot

#### Scenario: One entrypoint snapshot is too large
- **WHEN** a candidate would produce an `EntrypointSnapshot` larger than 262,144 serialized bytes
- **THEN** the daemon rejects the candidate before applying configuration
- **AND** the prior runtime snapshot remains representable and unchanged
