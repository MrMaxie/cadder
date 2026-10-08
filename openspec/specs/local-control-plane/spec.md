# Local control plane

## Purpose
Define the owner-authenticated exact-version IPC contract.

## Requirements

### Requirement: IPC-001: Local transport is owner authenticated and bounded
Cadder SHALL use NDJSON frames no larger than 1 MiB over Unix sockets or Windows named pipes with bounded connections/timeouts, owner-only protected runtime/secret storage and security policy 2. Both peers SHALL authenticate using fresh direction-bound HMAC challenges before RPC. Secret bytes SHALL NOT cross IPC. Unsafe ownership, links, modes or ACLs SHALL fail closed rather than be silently repaired or weakened.

#### Scenario: Spoofed endpoint or unsafe secret file
- **WHEN** endpoint proof fails or the secret/runtime protection is unsafe
- **THEN** the client or daemon SHALL reject contact without performing RPC
- **AND** it SHALL NOT transmit the secret or repair the unsafe boundary implicitly

#### Scenario: Replay or reflected proof
- **WHEN** a prior proof or authenticated RPC frame is replayed or reflected
- **THEN** fresh challenge or directional sequence validation SHALL reject it
- **AND** no product mutation SHALL execute

### Requirement: IPC-002: Handshake requires one exact version
Every connection SHALL validate protocol 3 and security policy 2 during mutual authentication and SHALL reject mismatches before product dispatch. The Node product SHALL NOT communicate with Rust IPC or negotiate legacy capabilities.

#### Scenario: Rust daemon remains running
- **WHEN** a Node client encounters an old Rust endpoint or incompatible peer
- **THEN** it SHALL NOT attach or mutate it
- **AND** isolated v2 discovery SHALL preserve the old runtime and its data

### Requirement: IPC-003: One typed envelope carries eight operations
RPC SHALL preserve the eight released operations: register, unregister, heartbeat, query state, set entrypoint activation, set domain activation, query logs and shutdown. Zod SHALL validate their closed method/payload/result/error catalog; unknown methods/fields SHALL be rejected. Each response SHALL correlate the request and contain exactly one result or typed error. CLI/TUI SHALL use one shared client service. Migration SHALL NOT add history, IIS, autostart or presentation-specific RPC methods.

#### Scenario: Malformed product request
- **WHEN** a request violates its method schema or names an unknown method
- **THEN** it SHALL receive a bounded typed rejection without invoking a handler
- **AND** valid responses SHALL contain exactly one correlated outcome
