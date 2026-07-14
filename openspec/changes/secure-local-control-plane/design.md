## Context

The daemon currently uses `interprocess` and newline-delimited JSON, but its `read_line` paths have no byte limit and each accepted connection becomes an untracked task. Peer identity is available at accept time yet authorization occurs only after a request has already been buffered and decoded. Windows identity is inferred from a client PID instead of a named-pipe impersonation token. Endpoint metadata is written in place, clients can derive the endpoint without reading it, and the stable runtime hash is reused as though it identified a daemon instance.

The protocol already has early capability and error types, but its version is a flat integer, future versions are accepted without negotiated semantics, dispatcher capability checks are incomplete, and client code collapses daemon errors into text. The shim maintains a persistent session but ignores heartbeat failures, starts missing daemons, exposes executable overrides, and detaches immediately when IPC closes.

Cadder protects every directly spawned Caddy process tree. Unix uses the `process-wrap` process-group boundary. Windows uses a Cadder-owned Job Object: the child starts suspended, joins a private kill-on-close job, and resumes only after assignment succeeds. This ordering prevents an uncontained child from running and avoids `process-wrap` 9.1.0's racy ToolHelp-based resume path. A separate owner-loss guard is still required on Unix because Rust `Drop` does not run when the daemon is forcibly terminated.

## Requirements

- `RUN-004`, `RUN-009`: pin one verified Caddy installation and one owned process generation; never enumerate or signal by name or an unverified PID.
- `RUN-005`, `IPC-009`: one shutdown coordinator drains connections, runtime work, storage, discovery, and locks in a fixed order.
- `REG-002`, `REG-003`: persist stable registration identity, bind each lease to one daemon instance and shim session, reconnect within a bounded window, and expire through the normal configuration transaction.
- `REG-005`, `REG-006`: close the shim command surface and prevent project input from selecting executables or privileged behavior.
- `IPC-001`: authenticate the runtime owner at accept time through a local-only endpoint.
- `IPC-003`: make atomically published discovery mandatory for all clients and bind it to a unique daemon instance.
- `IPC-004`, `IPC-005`: bound frames, connections, requests, streams, writes, queues, and shutdown.
- `IPC-006`, `IPC-010`: negotiate protocol major/minor and capabilities before decoding an operation payload.
- `IPC-007`: preserve request identity and typed error semantics at every layer.
- `CAD-001`: select Caddy only from explicit runtime sources and reverify its pinned identity and content for every spawn.
- `TUI-001`, `TUI-002`, `TUI-003`, `TUI-005`: launch the TUI explicitly, render authoritative operator state, route mutations through the shared client, and keep production data out of the binary.

## Goals and non-goals

### Goals

- Establish one authenticated, bounded, negotiated local control plane shared by all 1.0 clients.
- Make trust decisions before allocating request-sized buffers or invoking handlers.
- Keep lifecycle and recovery deterministic under disconnects, timeouts, cancellation, normal shutdown, and owner loss.
- Pin executable identity and process ownership for the daemon lifetime.
- Retain the existing domain boundaries, `interprocess`, NDJSON, Tokio, and focused fake-Caddy testing style.

### Non-goals

- This design does not define the final paged snapshot/view-model protocol.
- This design does not expose the private Caddy Admin API or complete configuration transactions.
- This design does not implement IIS mutations or elevation.
- This design does not complete `IPC-002`; the future IIS helper and handle-inheritance proof stay in the IIS change.
- This design does not complete managed-run readiness, failed-removal snapshots, tombstones, or durable desired activation from `REG-001`, `REG-008`, and `REG-009`. It implements the stable key and public registration ID required for `REG-002` without claiming the remaining desired-state behavior in `REG-009`.
- This design does not complete the full runtime configuration and status/doctor contract in `RUN-008`; it moves only the real-Caddy selector required by `CAD-001`.
- This design does not make pre-1.0 protocol versions wire-compatible.

Lease expiry still uses the existing coordinator's product-level prepare, apply, and commit boundary and proves that accepted routes disappear atomically in integration tests. `stabilize-caddy-runtime` later replaces the real-Caddy backend of that boundary with authenticated `/load` and last-known-good recovery; it does not postpone the lease behavior in `REG-003`.

## Ownership and trust boundaries

The runtime owner is the effective user that starts the per-user daemon. This change authenticates that principal at the control plane; the separate helper-role and elevation construction boundary remains part of `IPC-002` and the IIS change.

On Linux and macOS, the runtime directory is mode `0700`, the filesystem socket and discovery file are mode `0600`, and the accepted peer effective UID must equal the daemon effective UID before a protocol byte is read. On Windows, the runtime directory, discovery file, and named pipe use an owner-only DACL, and the pipe rejects remote clients. The connection boundary consumes exactly one ASCII space byte (`0x20`) as a transport-authentication preface under the original acceptance deadline. The preface is not a protocol frame, secret, or authority claim. Without an intervening await point, the server impersonates the named-pipe client, reads and copies the thread-token user SID, verifies `RevertToSelf`, and compares the SID to the daemon owner. Only then can the connection reach protocol buffering. A revert failure terminates the daemon; PID remains diagnostic metadata only.

Discovery contains a stable runtime ID, a random daemon instance ID, profile, endpoint, protocol range, capabilities, and publication generation. It is public only to the runtime owner and is never an authentication token. A connection must authenticate at the transport and then complete a handshake that matches the discovery instance.

The real-Caddy selector comes from an explicit daemon-start override, standard per-user configuration, standard system configuration, or a native PATH search that excludes Cadder's shim by file identity. Project configuration, project environment, executable-adjacent files, and shim flags never select the executable. The selected path must be absolute and resolve to a regular native executable, but Cadder does not impose an ownership or ACL policy on that executable or its containing directories. `PinnedCaddyImage` contains canonical path, source, file identity, SHA-256 digest, parsed semantic version, required modules, and probe revision.

Before every probe, adapt, run, reload, or stop spawn, Cadder opens the candidate without following a final link, compares handle identity and digest to `PinnedCaddyImage`, confirms the canonical path still names that identity, and retains the verified handle through child creation. Windows opens the image without write or delete sharing. Unix rechecks handle metadata and digest immediately before the pathname spawn and checks path identity again before project input or traffic is accepted; a mismatch terminates the new owned tree. Tests inject swaps and hardlink mutations at every exposed seam.

Cadder's local boundary treats the authenticated runtime owner as trusted. It prevents project data from selecting Caddy and detects executable substitution through image pinning; it does not claim to enforce a machine-wide executable trust policy or sandbox a malicious process already running as that same owner.

## Data flow and public contracts

The connection sequence is `discovery -> connect -> Windows transport preface -> operating-system peer authentication -> verified impersonation revert -> ClientHello -> ServerHello -> requests`. Unix omits only the Windows preface and impersonation steps. The preface and first protocol frame share the absolute acceptance deadline; each step does not restart it.

`ProtocolVersion` contains `major` and `minor`. `CapabilityId` and `RequestId` are validated string newtypes. Every request and response envelope includes the negotiated version and request ID. Every 1.0 request also carries a required `payloadCapabilities` array. It stays empty for the retained base payload and names each negotiated extension used by a newer mutation payload. `ProtocolError` contains `kind`, stable `code`, `message`, optional `guidance`, `retryable`, and `request_id`.

`RawRequestEnvelope` keeps the payload as `serde_json::value::RawValue`, including duplicate keys, until the registry accepts the version, base operation capability, and every declared payload capability. Only the resulting `AuthorizedRequestEnvelope` exposes typed decoding. A sealed `OperationPayload` binds each decoder and outbound envelope to one operation and an exact extension set, so callers cannot substitute a permissive type, omit a decoder's extension, or mutate the header after construction. Payload-extension metadata belongs to the operation registry and fixes the extension's capability and first protocol minor; client-provided metadata never defines a gate.

Payload-only V1 mutation DTOs do not repeat the envelope request ID. Registration and autostart inputs use deliberately stable input types instead of response DTOs that evolve additively. Unknown mutation fields and unknown string discriminators produce `incompatible_payload`. Missing required fields, wrong scalar types, invalid values, and duplicate known fields produce correlated `invalid_payload`; none reaches a handler. Every retained public response DTO accepts documented additive fields recursively within a negotiated minor version. Unknown enum and union discriminators remain errors instead of falling back to a known meaning.

Terminal clients render an error in user order: the operation that did not complete, what remains safe or available, and the most relevant recovery action. Stable codes, request IDs, component names, and raw transport details remain available in diagnostics or machine output but do not dominate the primary message. Color and symbols never carry error, retry, or permission meaning on their own.

The full-screen terminal starts only for `cadder tui`; bare `cadder` renders command help. On launch and refresh, the TUI queries `OperatorContext` for the current daemon snapshot and retained log stream. A missing daemon renders an explicit unavailable state without starting one implicitly, while transport and protocol failures remain distinct error states. Entrypoint and domain toggles call the shared operator mutations that the CLI slice will expose and refresh from daemon state before presenting success. Unsupported settings are status-only rather than local switches. Tests may inject snapshots and operator outcomes, but the release binary contains no embedded projects, settings, logs, or pseudorandom status generator.

| Condition | Human outcome and recovery | Stable machine outcome |
| --- | --- | --- |
| Daemon unavailable | The daemon is not running; no runtime state changed. Run `cadder daemon start`, then retry. | `daemon_unavailable`, retryable |
| Permission denied | The selected runtime belongs to another principal; no request ran. Use the owning account or select an accessible profile. | `permission_denied`, not retryable without a context change |
| Incompatible protocol | This client and daemon cannot perform the operation together. Upgrade the older Cadder component. | `incompatible_protocol`, not retryable unchanged |
| Incompatible mutation payload | The mutation uses a field or variant outside the negotiated contract; no handler ran. Upgrade the older component or omit the unsupported extension. | `incompatible_payload`, not retryable unchanged |
| Stale discovery instance | The daemon changed before the connection completed. Reread discovery and retry once against the new instance. | `stale_instance`, retryable once |
| Connection capacity exhausted | The daemon is busy; existing work is unaffected. Retry after a bounded delay. | `busy`, retryable |
| Operation timeout | The operation did not finish and its commit authority is revoked. Check current status before retrying. | `timeout`, retryability set by the operation registry |
| Daemon shutting down | The daemon is stopping and did not start the operation. Start it again, then retry. | `shutting_down`, retryable after restart |

The client reads and validates discovery, connects to its endpoint, authenticates through the OS transport, and sends `ClientHello` with its supported version range, requested capabilities, runtime ID, and daemon instance ID. `ClientHello` and `ServerHandshakeFrame` are standalone pre-negotiation frames; operation envelopes begin only after a successful handshake, so request IDs never appear in two nested envelope layers. The daemon accepts with `ServerHello` containing the selected version and capabilities or rejects with a correlated `ProtocolError`, including version-upgrade guidance when ranges do not overlap. Capability advertisement and intersection are computed for the selected protocol version, so a 1.0 session never negotiates an extension introduced in 1.1.

A central `OperationRegistry` maps every message type to its capability, immutable introduction version, read/mutation classification, stream classification, deadline class, and payload extensions. Dispatch first requires the envelope version to belong to the build's supported range, then checks the operation's introduction version and base capability. It next rejects duplicate, unknown, unavailable, or too-new `payloadCapabilities`; typed payload decoding starts only after those checks. Existing operations retain introduction version 1.0 when the current protocol minor advances.

| Capability | Operations | Access and delivery | Deadline | Retry after timeout |
| --- | --- | --- | --- | --- |
| `entrypoint-registration` | register, unregister | mutation, unary | reload | no |
| `entrypoint-registration` | heartbeat | mutation, unary | ordinary | yes |
| `runtime-state` | query state | read-only, unary | ordinary | yes |
| `state-subscription` | subscribe state | read-only, server stream | stream | yes |
| `activation-control` | set entrypoint/domain enabled | mutation, unary | reload | no |
| `iis-handoff` | query bindings | read-only, unary | ordinary | yes |
| `iis-handoff` | set handoff | mutation, unary | reload | no |
| `logs` | query logs | read-only, unary | ordinary | yes |
| `history` | query history | read-only, unary | ordinary | yes |
| `autostart` | query autostart | read-only, unary | ordinary | yes |
| `autostart` | set autostart | mutation, unary | ordinary | no |
| `daemon-lifecycle` | shutdown daemon | mutation, unary | shutdown | no |

The transitional flat envelope uses an explicit legacy registry adapter that preserves its numeric compatibility floor, embedded request IDs, and advertised-capability semantics without treating that connection as a negotiated 1.0 session. Removal of this adapter belongs to the shared-client migration in task 6.1 together with the production switch to the handshake, raw versioned envelope, and payload-only request DTOs.

During this transition, every local client entry point returns `IpcClientResult`. `IpcClientError::Daemon` owns the exact `ProtocolError` received from the daemon. `IpcClientError::Local` owns a stable local code, failure phase, guidance, retryability, optional request and operation correlation, and the original error source when one exists. The operator retains that typed error as its source while exposing a serializable projection for output. The shim branches on typed classifications and presents the supplied recovery guidance; it suggests daemon startup only when the local endpoint is actually unavailable.

The sealed legacy request trait binds each request DTO to one operation and one response contract. The client validates both bindings before connection or write, rejects malformed request IDs locally, and treats every `protocol-error-response` as `Err` rather than a successful payload. It validates the response version before interpreting its message type or payload, then checks the outer and nested response IDs against the request. A reusable unary session becomes terminal before an exchange starts and returns to service only after a complete success or typed daemon rejection. A local failure, deadline, or external cancellation drops the transport because cancelling `read_line` or `write_all` cannot safely preserve a partial frame. A later call on that terminal session returns retryable `connection_closed` because it sends no new request. One absolute operation deadline spans write and read while preserving `RequestWrite` or `ResponseRead` as the failed phase. Timeout retryability comes from the operation registry, so an uncertain read-only query can be retried while an uncertain mutation cannot.

Unary connect and response deadlines apply in the transitional client. An idle state subscription remains unbounded until task 3.3 supplies the cancellation-safe bounded codec and heartbeat contract; placing `timeout` directly around the current `read_line` would corrupt a reusable stream after partial input. A stream drops its transport and becomes terminal after any event error. Daemon launch readiness uses a typed local probe: only expected unavailable-endpoint conditions mean not ready, while permission, endpoint, timeout, and unexpected transport failures return immediately without starting a competing daemon. Task 2.5 replaces this transitional socket probe with authoritative discovery and the instance-matching handshake.

One codec based on `tokio_util::codec::LinesCodec` serves daemon, client, shim, and subscription paths. Its maximum line length is 1,048,576 bytes excluding LF. Outbound envelopes are serialized into a bounded buffer before writing. The connection reader remains active while a handler runs so that a second pipelined request can be rejected without starting another handler; later sequential requests on the persistent shim session remain valid.

`IpcLimits` supplies the production constants and short test values: 64 live connections; 5-second first-byte and write-progress deadlines; 30-second frame, ordinary handler, and shutdown deadlines; a 120-second reload deadline; 15-second stream heartbeat; at most 256 queued stream records and 524,288 serialized queued bytes; cancellation checks at least once per second. A semaphore permit covers the entire connection lifetime. Root and child `CancellationToken`s plus `TaskTracker` own every connection, handler, writer, heartbeat, lease, and background task.

The daemon derives the stable entrypoint key from profile, canonical project root, and canonical Caddyfile path, persists one opaque public registration ID for that key, and issues an opaque live lease bound to the authenticated principal, daemon instance, and shim session nonce. The shim renews that lease every five seconds. A successful renewal advances the lease without publishing a full state-change event; lease-state transitions publish state changes for operator clients. Transport loss moves it to `reconnecting`; it remains authoritative for at most 30 seconds. The shim rereads discovery and reconnects with exponential backoff from 100 milliseconds capped at five seconds. With the same daemon instance it resumes the existing lease. With a new daemon instance, the old lease is rejected and the same authenticated shim session submits the stable key and public ID to receive a fresh instance-bound lease. Expiry removes routes through the existing normal configuration transaction and causes the managed shim process to exit nonzero.

| Shim state | Human stderr behavior | Action and final status |
| --- | --- | --- |
| Initial daemon unavailable | Print one daemon-unavailable outcome with `cadder daemon start` recovery. | Start neither daemon nor Caddy; exit nonzero. |
| Reconnecting to the same instance | Print one calm notice that routes remain active during the bounded reconnect window; do not print every retry. | Continue managed run without changing stdout. |
| Ownership restored | Print one restrained restoration notice because it resolves the earlier warning. | Resume heartbeat; keep running. |
| Clean Ctrl+C | Print no connection-loss warning. | Request bounded detach and exit zero after the terminal outcome. |
| Clean daemon shutdown | State that management ended because the daemon stopped and name the restart action. | Exit nonzero after bounded cleanup. |
| Daemon instance changed | Print no extra warning while the shim submits a fresh lease for the same stable identity. If accepted, use the normal restoration notice. | Reject the old lease; continue only with the new instance-bound lease. |
| Ownership conflict | State that another live session owns the registration and name the conflicting object when safe. | Preserve the accepted owner; exit nonzero. |
| Reconnect deadline expired | State that reconnect failed within 30 seconds and routes are being removed. | Wait for the bounded removal outcome; exit nonzero. |

Human shim notices use stderr. Delegated Caddy stdout and exit status remain unchanged, and the required delegation notice remains on stderr. Non-TTY and `TERM=dumb` presentations contain no ANSI sequences and keep the same textual state and recovery labels without relying on color, icons, spinners, or animation. Stable CLI machine envelopes and `--no-color` behavior remain in `deliver-operator-cli`.

All Caddy children use `ProcessTreeChild`. One hidden `cadderd` runtime guard runs for the entire daemon generation, including periods with no active Caddy child. The guard executable must be an absolute regular native executable and is pinned by file identity and digest, but Cadder does not impose an owner, ACL, or containing-directory permission policy on its installation path. The guard owns the process-tree handle and `cadder-containment.lock` while monitoring one owner-only, single-connection control channel from the daemon. The channel accepts only bounded, versioned bootstrap, start, terminate, status, and finalize frames. Every frame binds the profile, runtime ID, daemon instance ID, daemon owner generation, an unpredictable 256-bit generation nonce, and a monotonically correlated request ID. It never accepts an arbitrary executable, argument vector, environment, working directory, or general Cadder IPC request. A malformed, oversized, timed-out, unknown, duplicate, or cross-generation frame fails closed. After the ready response, control EOF is irreversible owner loss and the guard does not accept a replacement connection.

The guard independently reverifies the pinned Caddy claim before it starts `caddy run`, then owns the resulting `ProcessTreeChild`. Short-lived `adapt`, `reload`, and private stop helpers continue through the daemon's verified spawn gate. Caddy does not inherit the guard listener or daemon-side control handle. Its stdout and stderr remain connected to the daemon's bounded log readers without sharing the guard protocol channel.

The owner-only containment record is atomically replaced and binds schema and protocol revisions, profile, runtime ID, daemon instance and owner generations, the nonce commitment, guard image and operating-system process identity, optional child generation and process identity, and the pinned executable path, file identity, digest, version, and probe revision. Only the live guard writes `Ready`, child, and terminal transitions after handoff. `treeEmpty` can become `true` only after the guard has joined the exact owned tree. On clean finalize or daemon-channel EOF, the guard terminates and joins that tree, writes the terminal record, and releases the containment lock in that order.

A candidate daemon acquires `cadder.lock`, then `cadder-containment.lock`, and validates the previous terminal record against the stale daemon metadata before it creates a new generation. Missing containment data is acceptable only for a first start with no stale generation. A missing, unreadable, nonterminal, or mismatched record is ambiguous and fails closed without signaling a recorded PID. After writing `Preparing`, the candidate starts and authenticates the guard, releases the containment lock for the waiting guard, and waits for `Ready`. IPC discovery and Caddy startup remain unavailable until that handoff succeeds. The lock acquisition followed by terminal-record validation provides the replacement proof that the previous guard completed tree cleanup.

## Decisions

- Keep `interprocess` and NDJSON. They fit a local per-user control plane and avoid introducing a remote-service framework.
- Use `tokio-util` codecs, cancellation tokens, and task trackers. Hand-written buffering and detached task bookkeeping add avoidable security risk.
- Authenticate before reading a protocol frame. Rejecting a different user after buffering a frame still grants that user memory and CPU work; the fixed Windows transport preface exists only to bind pipe impersonation to the client token.
- Use Windows pipe impersonation, not PID-to-token lookup. PID reuse and process inspection are not authorization primitives.
- Require discovery plus handshake. A derived socket name cannot prove that the peer is the daemon instance the client discovered.
- Make `payloadCapabilities` part of the closed 1.0 request header. Handshake capabilities alone cannot tell an older daemon whether a retained mutation payload uses a newer field or discriminator.
- Preserve the raw payload until authorization and use `serde_ignored` for recursive unknown-field reporting. Distinct V1 registration and autostart input types prevent additive response fields or variants from silently widening a mutation contract.
- Generate closed JSON Schemas for every mutation payload and retain the readable schemas as release fixtures. A field, requiredness, type, discriminator, or closure change produces a reviewable diff instead of an opaque snapshot update.
- Keep recovery ahead of diagnostics in terminal errors. A person needs the safe next action before internal identifiers; automation still receives the complete typed error.
- Allow sequential requests but reject pipelining. The shim requires a persistent session, while one active request keeps ordering, cancellation, and ownership understandable.
- Pin and reverify Caddy by handle identity, digest, and semantic compatibility at every spawn. Re-resolving PATH or trusting a prior path check permits substitution.
- Retain `process-wrap` for Unix process groups. Own the small Windows Job Object spawn boundary so suspended spawn, job assignment, and resume form one fail-closed sequence; do not use `process-wrap`'s Windows resume path. Add a separate Cadder guard only where forced daemon termination bypasses Rust `Drop`. Neither a process group nor a Job Object alone provides a portable daemon-death guarantee.
- Reject implicit daemon start from the shim. Lifecycle belongs to `cadder daemon start`, which can report configuration and permission failures coherently.

Rejected alternatives include gRPC for local IPC, a bearer secret in discovery, authorization by PID, unbounded `read_line`, retrying flaky process tests, project-selected Caddy wrappers, and killing processes by executable name.

## Failure and recovery

Malformed, oversized, timed-out, incompatible, unauthorized, busy, pipelined, or shutting-down connections receive a typed bounded error when it is safe to write one; otherwise the server closes the connection. A transport-authentication failure closes the connection without a `ProtocolError` because no trusted frame or request ID exists yet. Its runtime diagnostic includes a stable, redacted reason code and excludes SID, account name, source error text, request content, and operation name; recoverable failures leave the listener accepting connections. If `RevertToSelf` fails after Windows client impersonation, the daemon terminates before the executor can reuse that thread under the client token. No rejected request reaches an operation handler. Every mutation receives an `OperationFence` containing the lifecycle epoch and a separate per-operation `CommitPermit`. Drain or daemon replacement advances the shared lifecycle epoch and invalidates every older fence. A request timeout revokes only that request's permit. Commit APIs atomically require both the current lifecycle epoch and a live permit, so late work cannot publish state and one timeout cannot invalidate unrelated operations.

Atomic discovery publication creates an unpredictable same-directory temporary file with exclusive create and owner-only permissions. On Unix it writes and `sync_all`s the file, renames it over the destination, and syncs the parent directory. On Windows it applies the owner DACL before writing, calls `FlushFileBuffers`, then uses atomic `ReplaceFileW` for an existing destination or write-through `MoveFileExW` for the first publication. Cleanup removes a file only when its instance and generation still match the publisher, so an older daemon cannot delete a replacement's discovery. A crash before replace leaves no authoritative partial record; startup removes only a verified stale temporary generation.

Shutdown uses one 30-second normal-operation budget: at most two seconds to enter drain, close accept, and enqueue terminal outcomes; eight seconds to finish or cancel request and stream tasks; ten seconds for graceful Caddy stop and owned-tree escalation; five seconds to interrupt, flush, and join storage; and five seconds for generation-matched discovery cleanup and lock release. Entering drain revokes every mutation fence before tasks are cancelled. Handler code cannot start unbounded blocking work; bounded blocking adapters expose an interrupt or process-tree kill seam and return through tracked tasks. Phase expiry cancels remaining asynchronous work and joins its handles within that phase. Work already inside an asynchronous rollback or a non-interruptible durability call enters fail-stop containment: the daemon retains runtime, discovery, and lock ownership until that work finishes instead of detaching it after the normal budget. Ctrl+C, IPC shutdown, and fatal server errors use this same coordinator, and the shutdown response is written before its connection enters drain.

Lease reconnect preserves routes only until the monotonic deadline. Ownership mismatch, capability loss, or deadline expiry fails closed. A daemon-instance change always rejects the old lease and can issue a new one only after the stable key, public ID, authenticated principal, and shim session identity all match persisted state. The daemon never transfers a lease based solely on a registration ID or PID.

## Migration and rollback

This is a pre-1.0 protocol break. The protocol major advances and old clients receive typed incompatibility guidance. There is no fallback to the old endpoint derivation, unbounded reader, project resolver, or immediate disconnect cleanup.

Trusted real-Caddy selection is copied by the user or installer into the documented per-user location; Cadder does not silently import project or executable-adjacent selectors. A rollback of the code requires rolling back daemon, shim, and operator together. Runtime and discovery formats carry explicit versions, and a newer unknown format fails closed without deletion.

## Test strategy

Unit tests cover protocol version negotiation, version-aware capability intersection, recursive additive decoding through every retained public response DTO, gated-mutation compatibility, operation gating before payload decode, closed mutation DTOs, malformed-input classification, typed error round trips, exact codec boundaries, queue byte accounting, selector precedence, native executable validation, shim exclusion, executable identity, command policy, durable registration identity, lease transitions, cancellation, and atomic publication generations. Checked fixtures cover every registered mutation, its generated closed JSON Schema, the complete operation and capability registry, the exhaustive protocol-error kind-to-code mapping, and every serialized enum or union discriminator. Their generated contract comparison fails the release test until an intentional wire change receives an IPC-010 compatibility review and updates the readable fixture.

Platform tests cover Unix modes and peer UID, Windows owner DACL, local-only pipe mode and impersonated SID, and process-tree and owner-loss containment. Integration tests use short injected limits and paused Tokio time for slowloris connections, the 65th connection, frame and handler deadlines, pipelining, reconnect backoff, lease expiry, stream lag, stalled writers, non-cooperative handlers, interrupted storage, and shutdown drain. Fake Caddy fixtures cover shim identity, swaps between resolution and spawn, hardlink content changes, version/module probes, child/grandchild cleanup, and unrelated-process preservation.

Terminal golden and pseudo-terminal tests cover each human error and shim state above, Ctrl+C during steady state, reconnect and drain, exact delegated stdout and exit status, the delegation notice on stderr, no ANSI, non-TTY output, and `TERM=dumb`.

Focused checks run for each task. The final evidence includes strict OpenSpec validation, formatting, Clippy with warnings denied, all workspace tests, docs checks, and `cargo xtask check`.

## Risks and trade-offs

- [Risk] Windows impersonation and DACL code is unsafe and platform-specific. -> Keep it behind one small adapter, document every unsafe invariant, and test the resulting access control on Windows CI.
- [Risk] A runtime guard adds a hidden process mode. -> Authenticate its one-shot channel, bind it to a random generation, expose no general command execution, and test forced owner loss on every platform.
- [Risk] Reading while handling adds connection state complexity. -> Use one explicit state machine and one writer task instead of concurrent ad hoc reads and writes.
- [Risk] Removing project and executable-adjacent Caddy selectors breaks a convenient local override. -> Provide actionable diagnostics and keep the explicit daemon-start override for controlled development and tests.
- [Risk] Thirty-second lease preservation temporarily retains stale routes. -> Bound the window, report reconnecting state, and remove routes transactionally at expiry.
- [Risk] A full shutdown drain can take longer than immediate process exit. -> Enforce one 30-second budget and escalate only the owned process tree after the graceful phases.

## Open questions

None. Implementation starts only after this section is resolved.
