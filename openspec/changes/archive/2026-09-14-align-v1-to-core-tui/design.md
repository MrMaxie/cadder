## Context

The current executable exposes only `cadder tui`, while the accepted specifications describe a large CLI, multiple profiles, streams, history, autostart, setup commands, and machine-output contracts. The shim already starts a missing daemon for managed `caddy run`, and the portable release work already packages the PATH-facing shim as `caddy`. Keeping the broader contract would require substantial IPC, storage, compatibility, and presentation code without completing another user journey.

The affected audiences are project users invoking `caddy run`, operators using the TUI, and maintainers diagnosing a foreground daemon or building portable archives. Mock backends, hidden flags, private paths, and repository task runners are local or contributor information and do not belong in these product contracts.

This change depends on `simplify-v1-foundation` being verified, synchronized, and archived first. This change owns the product-level storage and observability reduction: retained durable state, bounded logs, and removed history, export, paging, and subscription behavior. `replace-custom-storage-with-sqlite` owns only the replacement engine, internal schema, destructive pre-1.0 cleanup, and storage failure implementation so the data migration remains separately reviewable.

## Goals / Non-Goals

**Goals:**

- Make managed `caddy run` through one user-owned daemon and the operator TUI the complete 1.0 product journey.
- Give the TUI explicit daemon start, stop, and restart controls alongside Status, Domains, Logs, and activation controls.
- Replace transitional IPC layers with one exact-version typed request/response protocol.
- Make `cadder-api` the only client transport and daemon-launch boundary used by the TUI and shim.
- Remove contracts that require code for unavailable or speculative surfaces.
- Keep runtime durability and observability requirements behavioral so the dependent SQLite design can replace the custom store without preserving its mechanisms.

**Non-Goals:**

- Implement SQLite or migrate runtime data; that belongs to the dependent storage change.
- Replace the existing local transport, peer authentication, Caddy process ownership, or executable identity protections.
- Introduce Web, Tauri, a general public SDK, a complete operator CLI, autostart, package-manager installation, or native installers.
- Refactor mutation coordination, repository tooling, or release packaging beyond what the reduced contract directly makes obsolete.

## Decisions

### 1. One product journey and three information layers

The product layer consists of managed `caddy run` and the TUI. Foreground `cadderd`, real-Caddy configuration failures, and bounded shutdown are operational. Detailed errors and logs are diagnostic and shown on demand. Local development fixtures and hidden overrides remain outside public specs and documentation.

The TUI owns daemon lifecycle controls because removing the planned CLI must not leave upgrade and recovery dependent on Task Manager, `kill`, or a hidden command. Start is idempotent, stop requests bounded shutdown of the owned daemon and Caddy child, and restart observes stop before launching and confirming readiness.

### 2. One runtime per installation directory

Cadder resolves one runtime from the executable directory and admits one daemon through the existing endpoint lease. Public profile selection and the single-variant `RuntimeProfile` abstraction are removed. Test-only runtime overrides remain implementation seams and are not part of the public contract.

This preserves ordinary user privilege and existing OS-specific endpoint protection while deleting selection, precedence, compatibility, and UI concepts that have no second real runtime.

### 3. One typed exact-version protocol

`cadder-ipc` defines one serde-tagged request enum and one correlated response envelope. The retained operations are register, unregister, heartbeat, runtime snapshot, set entrypoint activation, set domain activation, bounded log query, and shutdown. Each connection performs one exact protocol-version handshake before dispatch; a mismatch returns one typed incompatible-version outcome and closes.

Legacy flat envelopes, minor-version capability negotiation, operation discovery, subscriptions, watch streams, history, autostart, paging, snapshot tokens, and compatibility fixtures are removed. NDJSON framing, owner authentication, frame limits, deadlines, request correlation, and bounded shutdown remain.

Alternatives considered:

- Standard-library ad hoc messages would reduce dependencies but recreate framing, serialization, and error handling already provided by serde and the current transport.
- The focused existing `serde` plus `interprocess` stack keeps the proven security boundary and removes transitional protocol layers.
- A general RPC framework would add transport adaptation, generated interfaces, and compatibility policy without reducing the small retained operation set.

### 4. `cadder-api` is the sole client boundary

Client connection, handshake, request execution, error mapping, daemon launch, and readiness polling move behind `cadder-api`. The shim and operator depend on `cadder-api` and `cadder-ipc`, never on `cadder-daemon`. `cadder-daemon` remains the server implementation and depends on the wire contract only.

Presentation-specific view models are removed. The TUI maps the small shared response DTOs directly into rows and actions. A new client crate would add another package and migration seam; placing client transport in `cadder-ipc` would mix the wire schema with process-launch policy. Reusing the existing API crate gives the boundary a single concrete responsibility with fewer dependency edges.

### 5. Portable archive remains the release boundary

The archive contains the version-matched `cadder`, `cadderd`, and PATH-facing `caddy` executables, license, sample configuration, and published SHA-256 checksum. No installer, alias-setup command, package-manager formula, SBOM, signing gate, or updater is part of 1.0. Manual upgrade uses the TUI stop action before coherent binary replacement.

## Risks / Trade-offs

- **Pre-1.0 clients stop interoperating after the protocol reset** -> Exact version rejection gives a clear recovery action: use binaries from one archive.
- **Removing the CLI narrows automation options** -> The accepted 1.0 audience is an interactive local operator; automation is deferred until a concrete journey justifies a stable surface.
- **One runtime removes profile isolation** -> Separate installation directories remain independently keyed; multi-profile behavior can return only with a demonstrated user need.
- **TUI stop disconnects its own session** -> Treat the expected disconnect as successful only after the daemon acknowledges shutdown; restart performs a fresh launch and readiness probe.
- **This change overlaps an unfinished foundation change** -> Do not apply it until `simplify-v1-foundation` passes its reopened verification gate and is archived.

## Migration Plan

1. Verify, synchronize, and archive `simplify-v1-foundation` without adding this scope to it.
2. Reduce canonical product, CLI, TUI, registration, lifecycle, Caddy, IPC, and distribution contracts in this change.
3. Implement the client boundary and exact-version protocol, then remove superseded operations and presentation models.
4. Implement the TUI lifecycle actions and align documentation and archive verification with the retained journey.
5. Apply `replace-custom-storage-with-sqlite` before treating the reduced 1.0 foundation as release-ready.

Rollback before 1.0 is a source rollback of this complete change. Mixed old and new binaries are unsupported and fail the exact-version handshake without mutating daemon state.

## Open Questions

None.
