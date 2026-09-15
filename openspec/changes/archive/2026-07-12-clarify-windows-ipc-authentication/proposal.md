## Why

`IPC-001` requires peer authentication before reading a request, while the active implementation design also says that authentication happens before the first byte. Windows named-pipe impersonation binds to the client context established by the last pipe message read, so the contract needs to distinguish a fixed transport-authentication preface from a protocol frame without weakening the pre-dispatch trust boundary.

## What Changes

- Define a fixed one-byte Windows transport-authentication preface that carries no authority and is not part of NDJSON framing.
- Require the daemon to capture the impersonated client-token SID and verify `RevertToSelf` before it buffers, decodes, or dispatches a handshake or request frame.
- Keep PID diagnostic-only and deny missing, unknown, or different operating-system identities for every operation.
- Specify that transport-authentication failure closes only that connection without trusting a request ID or exposing peer identity in diagnostics.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `local-control-plane`: Clarify the observable Windows connection sequence and the boundary between transport authentication and protocol framing for `IPC-001`.

## Impact

The change affects the Windows named-pipe connection preface, daemon peer-authentication boundary, local clients, IPC security tests, and the active `secure-local-control-plane` design. It does not change the NDJSON frame format, supported operations, public DTOs, or remote surface area.
