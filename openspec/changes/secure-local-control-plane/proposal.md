## Why

Cadder cannot safely become the single owner of a user's local Caddy runtime while its control plane accepts unbounded frames and connections, derives client identity too late, and lets project-local input influence executable selection. The shim also treats a broken transport as immediate detachment instead of a bounded lease interruption. This slice establishes the trust boundary that every later CLI, TUI, runtime, and IIS operation depends on.

## Requirement IDs

- `RUN-004`: The daemon owns only the Caddy process it starts
- `RUN-005`: Shutdown drains in-flight work
- `RUN-009`: Managed Caddy cannot outlive runtime ownership
- `REG-002`: Registration identity is durable and lease ownership is session-scoped
- `REG-003`: Heartbeats use a bounded lease
- `REG-005`: Shim command policy is explicit
- `REG-006`: Project inputs cannot select executable or privileged behavior
- `IPC-001`: The local endpoint authenticates the runtime owner
- `IPC-003`: Discovery is authoritative but not an authentication secret
- `IPC-004`: NDJSON frames are bounded and unambiguous
- `IPC-005`: Connections and operation time are bounded
- `IPC-006`: Versions and capabilities gate dispatch
- `IPC-007`: Envelopes and errors preserve correlation
- `IPC-009`: Shutdown closes the control plane deterministically
- `IPC-010`: Minor protocol evolution is additive and explicit
- `CAD-001`: Real Caddy comes only from explicit runtime sources
- `TUI-001`: TUI launch and terminal lifecycle are explicit
- `TUI-002`: TUI renders authoritative operator state
- `TUI-003`: TUI mutations use the operator API
- `TUI-005`: TUI exposes recovery actions without embedded production data

## Scope

This change hardens the per-user daemon boundary, protocol negotiation, transport framing, connection supervision, discovery, explicit Caddy resolution, owned-process containment, shim command policy, registration leases, and coordinated shutdown. It migrates the daemon, operator client, shim, and TUI to one bounded local IPC client and preserves newline-delimited JSON as the wire transport. The TUI slice removes embedded production data and renders the current authoritative operator view model; final paging remains in its owning change.

The implementation touches `cadder-protocol`, `cadder-daemon`, `cadderd`, `cadder-shim`, the operator client used by `cadder`, focused integration tests, portable architecture documentation, and the OpenSpec verification record.

## Non-goals

- Private Caddy Admin API transport, transactional `/load`, last-known-good configuration, drift repair, and the complete Caddyfile allowlist remain in `stabilize-caddy-runtime`.
- Bounded collection paging, final public view-models, generated JSON Schemas, and stable CLI output remain in `deliver-operator-cli`.
- The elevated IIS helper, handle-inheritance boundary, and its typed plan channel remain in `complete-windows-iis-handoff` together with `IPC-002`.
- Full managed-run readiness, durable desired activation, the final entrypoint state model, tombstones, and failed-removal presentation remain in `stabilize-state-observability` and `deliver-operator-cli` together with `REG-001`, `REG-008`, and `REG-009`. This slice implements only the stable key and public ID needed by `REG-002`, not the remaining desired-state behavior in `REG-009`.
- The complete runtime configuration schema, listener-collision checks, retention settings, and status/doctor source presentation remain in their owning runtime, observability, and operator changes together with `RUN-008`. This slice accepts only the explicit real-Caddy selector needed by `CAD-001`.
- Web, Tauri, remote APIs, and MCP surfaces remain outside Cadder 1.0.

## Success criteria

- The local endpoint authenticates the peer before buffering or decoding any protocol frame and is reachable only by the runtime owner on every supported operating system.
- Server, client, shim, and subscriptions reject any NDJSON frame larger than 1 MiB without unbounded allocation.
- A profile admits at most 64 live connections; request, stream, and shutdown deadlines match the accepted contract and leave no detached tasks.
- Every connection completes a version-and-capability handshake tied to the current discovery instance before dispatch.
- Protocol errors retain stable kind, code, guidance, retryability, and request correlation across the wire and client API.
- A project-controlled source cannot select real Caddy; shim aliases and identity or digest changes observed at a spawn gate are rejected before project traffic is served.
- `cadder tui` renders real operator state or an explicit daemon-unavailable state; it contains no embedded projects, logs, settings, or pseudorandom runtime status.
- Shim mutations fail closed, `caddy run` never starts a daemon or unmanaged Caddy, and a transient disconnect preserves ownership for at most 30 seconds while reconnecting. A daemon restart rejects the old lease and issues a fresh instance-bound lease only for the same persisted entrypoint identity and authenticated shim session.
- Forced daemon loss terminates its owned Caddy tree without signaling an unrelated Caddy process.
- Ctrl+C, operator shutdown, and server failure use one bounded shutdown sequence and remove discovery only after connection, runtime, and storage drain.
- Focused security and compatibility tests plus `cargo xtask check` pass without waivers.

## Impact

The local wire protocol changes incompatibly before 1.0 and therefore advances its major version. Existing local daemon/client combinations fail with typed upgrade guidance instead of falling back. Real-Caddy selection moves out of project and executable directories into standard per-user or system configuration without imposing custom ACL policy on the Caddy installation. The shim no longer starts the daemon implicitly or delegates mutation-oriented Caddy commands. The TUI becomes a real operator client instead of a demonstration backed by embedded data. New direct dependencies are limited to mature libraries for bounded codecs, cancellation/task tracking, executable identity, semantic versions, and process-tree ownership.
