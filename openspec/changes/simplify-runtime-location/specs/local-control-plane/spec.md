## MODIFIED Requirements

### Requirement: IPC-001: The local endpoint authenticates the runtime owner
The runtime rooted beside the portable Cadder executables MUST expose one local endpoint and MUST authenticate the peer's operating-system identity before buffering, decoding, or dispatching any protocol handshake or request frame. Cadder SHALL NOT validate or modify ownership or privacy of the executable parent directory when creating runtime files. On Windows, each new named-pipe connection MUST begin with one ASCII space byte (`0x20`) as a transport-authentication preface solely to bind the accepted pipe client to its operating-system token. The preface MUST NOT carry authority, count toward an NDJSON frame, or extend the acceptance-to-first-frame deadline defined by `IPC-005`. A missing, unknown, or different identity SHALL be denied for every operation.

#### Scenario: Peer identity does not match
- **WHEN** a local client connects with an operating-system identity different from the daemon runtime owner
- **THEN** the daemon denies the connection before protocol dispatch

### Requirement: IPC-003: Discovery is authoritative but not an authentication secret
The daemon SHALL atomically publish `cadder-ipc.json` in the executable-colocated runtime root containing its metadata schema version, fixed profile identity, runtime identifier, instance identifier, endpoint, supported protocol range, capabilities, and diagnostic PID. Every client MUST validate and consume this record before connecting, then confirm the instance identifier during the protocol handshake. Possession of the file MUST NOT authorize a client.

#### Scenario: Client attaches to the portable runtime
- **WHEN** a client starts from a portable Cadder release directory
- **THEN** it reads the discovery record beside its executable, validates its schema and fixed profile identity, connects to the published endpoint, and confirms the same daemon instance

#### Scenario: Discovery is replaced during attach
- **WHEN** a client reads a discovery record while the daemon is replaced
- **THEN** the client retries bounded discovery, connects only after the handshake confirms the published instance identifier, and does not dispatch a request to an unconfirmed endpoint

### Requirement: IPC-005: Connections and operation time are bounded
The executable-colocated runtime SHALL accept at most 64 simultaneous client connections, including streams and incomplete connections. A new connection MUST send its first frame byte within five seconds of acceptance and complete that frame within 30 seconds of the first byte. After a complete valid frame is accepted, an ordinary handler and its terminal response MUST finish within 30 seconds; any individual response write that makes no progress for five seconds SHALL fail the request within that deadline. A configuration reload's advertised handler deadline begins when its complete frame is accepted and MUST NOT exceed 120 seconds.

#### Scenario: Slow client does not pin the endpoint
- **WHEN** a client connects but does not complete its first frame within the configured deadline
- **THEN** the runtime closes that connection and remains available for other clients
