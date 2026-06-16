---
id: TASK-1.18
title: Support detached backend mode with multiple dashboards
status: Done
assignee:
  - '@agent'
created_date: '2026-06-10 12:04'
updated_date: '2026-06-16 16:35'
labels: []
milestone: m-2
dependencies:
  - TASK-1.11
  - TASK-1.12
references:
  - docs/ARCHITECTURE.md
  - 'https://docs.rs/whirlwind/latest/whirlwind/'
modified_files:
  - README.md
  - crates/cadder-daemon/src/ipc.rs
  - crates/cadder-daemon/src/lib.rs
  - crates/cadder-daemon/src/state.rs
  - crates/cadder-daemon/tests/ipc_lifecycle.rs
  - crates/cadder-shim/src/main.rs
  - crates/cadder-tui/src/main.rs
  - crates/cadder-tui/src/model.rs
  - docs/ARCHITECTURE.md
  - docs/site/src/content/docs/index.mdx
  - docs/site/src/content/docs/quick-start/getting-started.mdx
  - docs/site/src/content/docs/reference/runtime-configuration.mdx
  - docs/site/src/content/docs/user-guide/how-to-use.mdx
  - docs/site/src/content/docs/user-guide/path-and-shim.mdx
  - docs/site/src/content/docs/user-guide/tui-diagnostics.mdx
  - docs/verification/testcontainers-e2e.md
  - docs/verification/tui-smoke.md
parent_task_id: TASK-1
priority: medium
ordinal: 18000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Rebaseline Cadder's future runtime architecture around a single per-user `cadderd` operational backend and zero-to-many attach-only clients. `cadderd` is the only process that owns the backend and the real Caddy runtime. `cadder-tui` and the PATH-facing `caddy` shim never auto-start the daemon; they attach when it is available, surface offline or unavailable state when it is not, and keep working across daemon restarts. `cadder-tui` may expose an explicit user-triggered action to start `cadderd`, but backend startup is never an automatic side effect of opening a dashboard or running a shim command. This task should establish that detached backend foundation for multiple dashboards, while leaving the future desktop `cadder` app to a later task.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Cadder supports a backend-only `cadderd` mode that runs independently of any interactive dashboard lifecycle and remains the only owner of the real Caddy runtime.
- [x] #2 Multiple dashboard/TUI clients can connect to the same backend concurrently, query state, subscribe to changes, disconnect independently, and reconnect without stopping the backend or removing shim registrations.
- [x] #3 `cadder-tui` and the `caddy` shim never auto-start `cadderd`; when the daemon is unavailable they surface explicit offline or unavailable guidance instead of starting it implicitly.
- [x] #4 `cadder-tui` can offer a user-triggered backend start action, but that action is explicit and distinct from ordinary attach, reconnect, and refresh behavior.
- [x] #5 CLI and UX clearly distinguish backend operation, dashboard attach, explicit backend start, and backend shutdown; `caddy run` requires a running backend and fails with a clear Cadder-owned message when `cadderd` is unavailable.
- [x] #6 Tests cover zero-to-many entrypoints with zero-to-many dashboard clients, including late backend availability, backend loss and reconnect, dashboard disconnects, and shim behavior when no backend is running.
- [x] #7 Architecture and user documentation explain the manual-daemon-first attach model, process-role boundaries, and when to use explicit backend start from the dashboard.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
## Goal
Establish Cadder's detached runtime foundation around a single manual-start per-user `cadderd` backend and zero-to-many attach-only clients, so multiple dashboards can observe the same backend without owning its lifecycle.

## Scope
- Rebaseline runtime ownership so `cadderd` is the only backend process and the only owner of the real Caddy runtime.
- Keep `cadder-tui` and the `caddy` shim as attach-only clients that never auto-start the daemon.
- Allow `cadder-tui` to expose an explicit user-triggered backend start action, but keep that distinct from normal attach and reconnect behavior.
- Keep future desktop `cadder` semantics aligned with this model, but do not implement that application in this task.
- Cover reconnect and offline behavior, many dashboards, and shim behavior when the daemon is absent.

## Key Files And Modules
- `crates/cadder-daemon/src/ipc.rs`
- `crates/cadder-daemon/src/lib.rs`
- `crates/cadder-daemon/src/state.rs`
- `crates/cadderd/src/main.rs`
- `crates/cadder-shim/src/main.rs`
- `crates/cadder-tui/src/main.rs`
- `crates/cadder-tui/src/model.rs`
- `crates/cadder-daemon/tests/ipc_lifecycle.rs`
- `docs/ARCHITECTURE.md`
- `README.md`
- `docs/site/src/content/docs/user-guide/tui-diagnostics.mdx`
- `docs/site/src/content/docs/reference/runtime-configuration.mdx`

## Implementation Steps
1. Rebaseline the daemon-launch contract so `ensure_daemon_running_with_options` is no longer used implicitly by attach-only clients. Replace automatic startup paths with explicit connection attempts and clear unavailable-daemon diagnostics.
2. Update `cadder-shim` so `caddy run` only attaches to an already running daemon. If `cadderd` is unavailable, fail with a Cadder-owned operational message that explains how to start the backend manually.
3. Update `cadder-tui` startup so opening the dashboard never starts `cadderd` automatically. The TUI should boot into an offline or attach state, keep the UI responsive, and continue trying to detect and attach to the daemon in the background.
4. Add a deliberate user-triggered backend start path in the TUI. The control and messaging should clearly distinguish explicit backend start from ordinary reconnect and refresh behavior.
5. Extend the IPC client layer with a durable state-subscription path built on `subscribe-state-request`, so dashboards can observe state changes continuously after they attach rather than relying only on periodic snapshot polling. Keep polling or reconnect probes only as a fallback for daemon detection and recovery.
6. Fix daemon shutdown semantics so `ShutdownDaemonRequest` can terminate the actual `cadderd` process after replying to the client, not only stop the owned Caddy runtime. Ensure connected dashboards surface the loss of backend cleanly and can reattach after a later manual restart.
7. Preserve the existing ownership boundaries for shim registrations: dashboard disconnects must not remove registrations or stop the backend, while shim disconnect cleanup continues to remove only the registration owned by that shim session.
8. Expand automated coverage for zero-to-many dashboards and zero-to-many shims, including attach before backend startup, attach after backend startup, backend loss while dashboards are open, dashboard reconnect after backend restart, explicit backend start from the TUI, and shim failure behavior when no backend is running.
9. Update architecture and user documentation to describe the manual-daemon-first attach model, the process-role boundaries (`cadderd`, `cadder-tui`, `caddy`), and the explicit backend start and shutdown operations available to users.

## Validation
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo run -p xtask -- check`
- Focused: `cargo test -p cadder-daemon --test ipc_lifecycle`
- Focused: `cargo test -p cadder-tui`

## Assumptions
- `cadder-tui` is the only dashboard delivered in this task; the future desktop `cadder` application remains out of scope but should follow the same attach-first contract later.
- `--no-start` may be retained temporarily as a compatibility alias if needed, but the resulting behavior must still be "never auto-start the daemon".
- Background daemon detection in the TUI means a lightweight reconnect or detection loop, not blocking startup and not busy-looping.
- Optional library evaluation: `whirlwind` may be considered only if subscriber or client bookkeeping becomes simpler with its concurrent sharded map or set types than with the existing Tokio and standard synchronization primitives. It is not required for this task.

## Risks And Boundaries
- This task changes the current runtime contract from auto-start to manual-daemon-first; docs, help text, and user-facing errors must be updated consistently to avoid operator confusion.
- Do not introduce OS service managers, autostart agents, or network-remote dashboards in this task.
- Do not broaden scope into the future `cadder` desktop application implementation.
- Keep the backend single-instance per runtime directory; multiple dashboards do not imply multiple backend owners.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Plan approved and recorded before implementation. Task rebaselined to the manual-daemon-first attach model per user approval, and the description plus acceptance criteria were updated to remove automatic dashboard or shim daemon startup.

Implemented the detached manual-daemon-first contract across daemon, shim, TUI, tests, and docs. `caddy run` now requires an already running `cadderd` and returns a Cadder-owned guidance message instead of auto-starting the backend. `cadder-tui` now launches in attach-only mode, keeps `s` as an explicit backend start action, retries attach with `r`, upgrades successful attaches into a durable `subscribe-state-request` stream, and surfaces backend loss cleanly before reattaching after later availability. `ShutdownDaemonRequest` now stops the owned real Caddy runtime and terminates the actual daemon process after replying. Added IPC lifecycle coverage for multiple dashboard subscribers and daemon self-shutdown, plus shim/TUI tests for attach-only and reconnect flows. Validation run: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo run -p xtask -- check`, `cargo run -p xtask -- coverage`. Coverage gate passed at 87.37% lines (8051/9215).

Fresh-eyes review found a shutdown-signal race in `ShutdownSignal::wait()` that could let `cadderd` miss a successful `ShutdownDaemonRequest` notification and continue accepting IPC clients. Fixed by registering the `Notify` waiter before checking the atomic shutdown flag, then reran focused IPC verification plus full `xtask` validation and coverage. Final coverage is 87.28% lines on the updated state.

Attempted an independent Claude audit twice via `claude --print --dangerously-skip-permissions --permission-mode bypassPermissions --effort max --output-format text`. The CLI answered a trivial heartbeat prompt but both substantive audit runs stalled without producing output, so no Claude findings were available for this closeout.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Rebased Cadder onto a manual-daemon-first runtime contract where `cadderd` is the only backend owner and both `cadder-tui` and the `caddy` shim are attach-only clients by default. The shim now fails `caddy run` with Cadder-owned startup guidance when the backend is unavailable, while the TUI starts in offline attach mode, offers an explicit `s` backend start action, upgrades successful attaches into durable `subscribe-state-request` updates, and handles backend loss plus later reattach without taking ownership of daemon lifecycle.

Extended the daemon IPC layer with zero-to-many state subscriptions and daemon self-shutdown semantics, including stopping the real Caddy runtime and ending the daemon accept loop after `ShutdownDaemonRequest`. Added coverage for multi-dashboard subscriptions, backend shutdown behavior, attach-only shim/TUI flows, and updated architecture, user docs, and verification guides to explain the detached backend model and explicit operator actions.

Validation on the final state: `cargo fmt --check`, `cargo test -p cadder-daemon --test ipc_lifecycle shutdown_daemon_request_stops_server_and_rejects_new_clients`, `cargo run -p xtask -- check`, `cargo run -p xtask -- coverage`. Coverage passed at 87.28% lines. Residual risk is low; the main follow-up is future live/manual smoke validation against a real backend environment if release confidence needs to go beyond the automated coverage already added.
<!-- SECTION:FINAL_SUMMARY:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [x] #1 Tests or explicit verification were run for the changed behavior
- [x] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
