## Why

Preserve the released project/shim/daemon/operator journey without porting Caddy
before G2. The Node foundation currently accepts arbitrary fixture RPC methods
and unconstrained results. The next capability is authenticated local IPC with
exactly eight validated operations and one correlated result or error, plus the
small contract-only ports that keep the next product boundaries explicit.

## Requirement IDs

- `IPC-001`: Keep authenticated, bounded local transport.
- `IPC-002`: Reject incompatible peers before dispatch.
- `IPC-003`: Close the eight-operation request/result/error catalog.
- `TOP-001`: Keep one daemon-owned installation boundary across the ports.
- `TOP-002`: Keep CLI/TUI access behind one typed client-service boundary.

## Scope

Implement reset-cadder-architecture task 2.4 in the protocol module, client
connection and existing dispatcher. Migrate runtime test fixtures to the product
catalog without introducing production handlers or test-only product fields.
Add only four mockable leaf ports—Caddy, platform, storage and client service—using
existing protocol DTO/result/error types; these are contracts, not handlers.

## Non-goals

No Caddy porting or lifecycle implementation, CLI/TUI implementation, new RPC
operation, storage worker, installation resolver, privilege acceptance or release
change. The four port declarations do not implement any of those boundaries. The existing
publish-cadder-1-0 implementation change owns separate Rust release work and is
not modified or a dependency of this Node protocol slice.

## Success criteria

Every operation has closed payload/result schemas. Unknown methods and fields
receive bounded typed rejection without invoking a handler. Clients reject
uncorrelated, malformed and non-exclusive responses. Runtime fixtures exercise
the same transport without arbitrary status/shutdown methods.

## Impact

Only Node protocol/connection/dispatch source, the four-port contract leaf and
focused tests/fixtures change. Dependencies, released Rust code and packaging
remain unchanged. This slice does not close G1, G2 or Windows Sandbox acceptance.
