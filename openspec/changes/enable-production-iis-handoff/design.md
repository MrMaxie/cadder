## Context

The daemon already discovers IIS bindings, composes IIS proxy routes, stores restore records, and tests direct binding mutations against a fake provider. Production mutation is intentionally disabled: the old prototype writes a temporary PowerShell script and elevates it with `ExecutionPolicy Bypass`, which creates a mutable-code and time-of-check/time-of-use boundary. The operator currently exposes neither IIS commands nor an IIS TUI view.

The target Windows setup keeps IIS on a loopback high port while one Cadder-owned Caddy process owns the public HTTP and HTTPS front door. Shim registrations such as Smarketing are composed alongside the IIS route. The normal daemon and all operator surfaces remain per-user and unelevated.

## Goals / Non-Goals

**Goals:**

- Implement authoritative IIS status, immutable preview, single-use apply, and restore.
- Execute exactly one scoped elevated helper invocation per apply or restore.
- Keep the same helper alive until the daemon either commits the matching Caddy route or requests reverse rollback.
- Revalidate the exact IIS pre-state immediately before mutation and preserve unrelated sites and bindings.
- Recover an existing Cadder-shaped loopback backend only when verified restore metadata or an explicitly reviewed recovery plan establishes its original public binding.
- Expose the same operations through the CLI and TUI.

**Non-Goals:**

- Running the normal daemon elevated.
- Accepting project-provided commands, scripts, executable paths, PowerShell fragments, or arbitrary IIS configuration.
- Managing IIS application pools, content, authentication, certificates, or sites beyond the selected binding pair.
- Treating an arbitrary high-port IIS binding as Cadder-owned without a reviewed recovery plan.

## Decisions

### Preview and plan store

The ordinary daemon discovers IIS state and builds a canonical typed plan containing the exact pre-state, post-state, inverse operations, selected route, expected Caddy change, privilege requirement, expiry, and semantic SHA-256. The plan payload is bounded to 524,288 bytes, retained in the owner-only profile plan directory for five minutes, and consumed atomically before elevation. At most 32 unused plans are retained. Active restore metadata is not published before mutation; the pre-mutation record is an in-flight plan only.

This preserves an explicit review boundary and prevents a delayed operator action from applying against changed IIS state.

### One-shot elevated `cadderd` helper

The installed `cadderd` binary gains a hidden helper mode. The normal daemon launches it through `ShellExecuteExW` with `runas` and `SEE_MASK_NOCLOSEPROCESS`, retaining the exact process handle and PID. The helper refuses to run without an elevated token, never initializes normal runtime paths, never publishes discovery, and never starts the general IPC dispatcher.

The daemon creates one random owner-restricted named pipe for the helper. Both sides bind the handshake to the daemon instance, plan hash, expiry, a 256-bit nonce commitment, and the observed server and client process identities. The helper accepts one plan and no general Cadder request. Control-plane and unrelated handles remain non-inheritable.

### Typed allowlisted IIS operations

The helper accepts only bounded `AddBinding` and `RemoveBinding` values that have already passed protocol validation. It resolves Windows PowerShell and the WebAdministration module from absolute system locations, runs without profiles or user module search paths, and constructs commands only from the typed fields. It never receives or executes a script path or free-form PowerShell text from the daemon, operator, project, environment, or runtime directory.

Immediately before mutation, the helper rediscovers IIS and compares the exact plan pre-state. It verifies each completed step and records inverse steps. If any step fails, or if the daemon disconnects before commit, the helper rolls completed operations back in reverse order before exiting.

### Caddy commit remains inside the helper transaction

For handoff, the helper first replaces the public IIS binding with the reviewed loopback backend and then waits. The ordinary daemon applies the matching Caddy proxy route. On success it sends `Commit`; only then does the helper exit and the daemon atomically publishes the active restore record. On Caddy failure, timeout, cancellation, or daemon loss, the helper performs the inverse IIS rollback without another UAC prompt.

Restore is symmetrical: the helper removes the reviewed loopback backend and restores the original public binding only after current state and Cadder ownership match the restore record. The daemon removes the Caddy route within the same transaction and clears restore metadata only after verified success.

### Operator surfaces

`cadder-operator` owns IIS status, preview, apply, and restore calls and maps typed IIS issues to the shared exit-code contract. The CLI implements the existing command hierarchy and output envelopes. The TUI adds an IIS tab; Space creates a preview, an overlay displays the exact changes and rollback, and a separate confirmation invokes apply or restore. Both surfaces refresh authoritative state after completion.

### Recovery of an existing loopback backend

Startup first loads a verified active restore record. When the IIS site is already on the recorded loopback binding, the daemon restores the matching Caddy route without changing IIS. If the loopback shape exists but no valid record is readable, status reports an orphaned recovery state. A recovery preview must show the inferred original public binding and certificate metadata and requires explicit confirmation before it may create ownership; it must never silently adopt an arbitrary binding.

## Risks / Trade-offs

- [Risk] UAC cancellation or timeout leaves the daemon waiting. -> Bound launch, handshake, mutation, and finalization; close the pipe and terminate only the exact retained helper process on timeout.
- [Risk] IIS changes between preview and apply. -> Consume the plan once, recompute and compare the exact pre-state in both daemon and helper before mutation.
- [Risk] Caddy apply fails after IIS moves. -> Keep the same helper alive and roll IIS back before reporting failure.
- [Risk] A generated script or user-controlled module is elevated. -> Elevate only installed `cadderd`; use absolute system binaries and modules; reject paths and free-form script text in the helper protocol.
- [Risk] A backend port collides. -> Preview probes both IIS binding state and listener availability; the helper rechecks before mutation.
- [Risk] Existing local state predates the new plan format. -> Import only verifiable restore records; otherwise report an explicit recovery preview or require manual restore.
- [Trade-off] The helper protocol adds Windows-specific code. -> Keep it behind narrow modules and fake the helper transaction for deterministic cross-platform tests, with an ignored real-IIS smoke on Windows.

## Migration Plan

1. Add the bounded plan and helper protocol without enabling production dispatch.
2. Add fake-provider and fake-helper tests for preview, replay, drift, rollback, and restore.
3. Enable the new IPC operations and operator surfaces; keep the legacy direct `SetIisHandoff` request rejected.
4. Add the elevated binary mode and Windows identity/channel tests.
5. Run a real local IIS handoff and restore cycle, then run IIS and a shim-managed Smarketing Caddyfile concurrently.
6. Remove the test-only elevated temporary-script prototype after equivalent coverage uses the typed helper seam.

Rollback disables the new mutation capabilities while preserving restore metadata and status visibility; an owned handoff must be restored before uninstalling the supporting binary.

## Open Questions

- Whether an existing orphaned `127.0.0.1:<high-port>` binding can be recovered automatically depends on verified legacy restore metadata. Without that evidence, recovery remains an explicit reviewed operation.
