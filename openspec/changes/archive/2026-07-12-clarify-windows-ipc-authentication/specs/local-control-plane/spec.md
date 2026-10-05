## MODIFIED Requirements

### Requirement: IPC-001: The local endpoint authenticates the runtime owner
Each runtime profile MUST expose one owner-only local endpoint and MUST authenticate the peer's operating-system identity before buffering, decoding, or dispatching any protocol handshake or request frame. On Windows, each new named-pipe connection MUST begin with one ASCII space byte (`0x20`) as a transport-authentication preface solely to bind the accepted pipe client to its operating-system token. The preface MUST NOT carry authority, count toward an NDJSON frame, or extend the acceptance-to-first-frame deadline defined by `IPC-005`. A missing, unknown, or different identity SHALL be denied for every operation.

#### Scenario: Unix owner connects
- **WHEN** the runtime owner connects on Linux or macOS
- **THEN** Cadder accepts the connection through a filesystem-domain socket inside a `0700` owner directory with socket mode `0600`
- **AND** the daemon confirms that the peer UID equals its effective UID

#### Scenario: Windows owner connects
- **WHEN** the runtime owner connects on Windows
- **THEN** the named pipe DACL grants access only to the owner's SID
- **AND** the daemon consumes only the fixed transport-authentication preface before impersonating the pipe client
- **AND** the daemon confirms the peer SID from the impersonated pipe-client token rather than trusting a PID or account display name
- **AND** the daemon successfully reverts impersonation before reading a protocol handshake or request frame

#### Scenario: Another principal reaches the endpoint
- **WHEN** a different UID, SID, or unauthenticated peer attempts to connect
- **THEN** the operating-system ACL or the daemon denies the peer before protocol-frame buffering or dispatch
- **AND** no read-only or mutating handler runs
- **AND** the daemon does not trust a request ID or send a protocol response on the rejected connection

#### Scenario: Recoverable peer authentication fails
- **WHEN** Cadder cannot read the required operating-system identity, validate the preface, open the client token, or match the runtime owner
- **THEN** it closes that connection without exposing the peer identity or unauthenticated input in diagnostics
- **AND** the listener remains available to accept another connection

#### Scenario: Windows impersonation cannot be reverted
- **WHEN** the daemon has impersonated a named-pipe client and `RevertToSelf` fails
- **THEN** the daemon terminates before the executor thread can process protocol input or unrelated work under the client token
- **AND** runtime containment and restart recovery handle the daemon loss
