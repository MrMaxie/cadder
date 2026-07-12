## Context

Cadder uses `interprocess` local sockets and NDJSON across Windows, Linux, and macOS. Unix exposes a peer effective UID without consuming application data. Windows named-pipe impersonation instead uses the security context associated with the last message read from the client. The active `secure-local-control-plane` design therefore cannot both impersonate the Windows client and authenticate before the first transport byte.

The trust boundary still has to complete before Cadder accepts any protocol claim, allocates a request-sized buffer, decodes a handshake or request, or invokes a handler. PID and account display names do not provide a stable Windows security identity.

## Goals / Non-Goals

**Goals:**

- Define one interoperable Windows preface that enables token-based peer authentication without becoming an authority token.
- Keep the accepted stream owned by a fail-closed authentication boundary until the client SID matches the daemon owner.
- Verify that the daemon returns from impersonation before protocol processing continues.
- Preserve NDJSON framing and the existing first-frame deadline.

**Non-Goals:**

- This change does not add protocol negotiation, connection limits, or bounded frame codecs.
- It does not use PID, account names, discovery contents, or the preface as proof of identity.
- It does not change Unix peer-credential behavior or define the elevated IIS helper channel.

## Decisions

### Use a fixed transport-authentication preface on Windows

Every Windows client writes one ASCII space byte (`0x20`) immediately after connecting. The server reads exactly that byte under the acceptance-to-first-frame deadline. The byte is not JSON, an NDJSON frame, a secret, or an authority claim; it only gives Windows a client message to bind to `ImpersonateNamedPipeClient`. An ASCII space also remains harmless to pre-1.0 scripted peers that read a complete JSON line.

Alternatives considered:

- Impersonating without a read does not establish the required message-bound client context.
- Reading part of a protocol frame gives unauthenticated input access to protocol buffering and creates ambiguous replay and framing behavior.
- Authenticating through the client PID is vulnerable to PID reuse and inherited handles.

### Capture the token SID synchronously and fail closed

After the preface read completes, the server performs one synchronous sequence without an await point: impersonate the pipe client, open the current thread token with query access, read `TokenUser`, copy its canonical SID, and call `RevertToSelf`. The server compares that SID with the daemon owner's process-token SID. Only a matching SID produces an authenticated connection that can reach protocol code.

Every missing token, malformed SID, operating-system error, or mismatch closes the connection. `RevertToSelf` failure aborts the daemon because continuing on an executor thread under an untrusted client identity is unsafe. PID remains optional diagnostic metadata and never participates in equality or policy.

### Keep denial outside the protocol

The server does not decode an envelope or trust a request ID before authentication. It therefore closes a rejected connection without a `ProtocolError`. The runtime log includes a stable, redacted reason code; it omits SID, account name, source error text, request content, and operation name. Recoverable authentication failures leave the listener accepting other connections. A `RevertToSelf` failure is different: the daemon terminates before the executor can reuse a thread under the client token.

### Keep the high-level local-socket transport

Cadder retains `interprocess::local_socket`. Its Windows dispatcher creates local-only named pipes, while Cadder supplies a protected owner-only DACL through the supported security-descriptor extension. Moving to the lower-level pipe API only to restate the existing local-only default would duplicate cross-platform transport work and is not justified by this clarification.

## Risks / Trade-offs

- **A silent client can hold one authentication task until the preface deadline.** → The existing connection-limit work counts incomplete connections and applies one absolute acceptance deadline.
- **A future `interprocess` release could change its named-pipe defaults.** → Cadder pins the dependency, tests the endpoint on Windows, and treats a transport upgrade as a security review item.
- **A rejected client receives EOF rather than a typed protocol error.** → Local clients retain a typed transport failure and operator diagnostics direct the user to the runtime owner without trusting unauthenticated wire data.
- **Fail-stop on revert failure interrupts the runtime.** → This is safer than returning a Tokio worker thread to the pool under the client token; owned-process containment and restart recovery handle daemon loss.

## Migration Plan

This is a pre-1.0 wire change. Daemon, operator, shim, readiness probes, and test peers adopt the preface together. No compatibility fallback accepts requests without authenticated transport identity.

## Open Questions

None.
