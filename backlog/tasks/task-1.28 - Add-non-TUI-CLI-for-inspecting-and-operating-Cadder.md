---
id: TASK-1.28
title: Add non-TUI CLI for inspecting and operating Cadder
status: Done
assignee:
  - '@agent'
created_date: '2026-06-16 15:25'
updated_date: '2026-06-16 18:42'
labels:
  - cli
  - automation
milestone: m-2
dependencies: []
references:
  - Cargo.toml
  - crates/cadder-protocol/src/lib.rs
  - crates/cadderd/src/main.rs
  - crates/cadder-tui/src/main.rs
documentation:
  - docs/ARCHITECTURE.md
  - docs/site/src/content/docs/reference/runtime-configuration.mdx
modified_files:
  - Cargo.lock
  - Cargo.toml
  - README.md
  - docs/ARCHITECTURE.md
  - docs/site/astro.config.mjs
  - docs/site/src/content/docs/index.mdx
  - docs/site/src/content/docs/quick-start/getting-started.mdx
  - docs/site/src/content/docs/reference/runtime-configuration.mdx
  - docs/site/src/content/docs/user-guide/how-to-use.mdx
  - docs/site/src/content/docs/user-guide/cadderctl.mdx
  - xtask/src/main.rs
  - crates/cadderctl/Cargo.toml
  - crates/cadderctl/src/lib.rs
  - crates/cadderctl/src/main.rs
  - crates/cadderctl/src/cli.rs
  - crates/cadderctl/src/error.rs
  - crates/cadderctl/src/render.rs
  - crates/cadderctl/src/view.rs
  - crates/cadderctl/src/app.rs
  - crates/cadderctl/tests/cli.rs
  - crates/cadderctl/tests/support/mod.rs
parent_task_id: TASK-1
priority: high
ordinal: 25300
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Add a command-line management interface so operators and automations can query Cadder state and execute common actions without opening `cadder-tui`. The CLI should reuse daemon contracts, provide stable exit codes and output modes, and cover the core inspection and control workflows needed for scripting.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 The CLI can inspect daemon status, entrypoints, domains, diagnostics, and recent logs without launching the TUI.
- [x] #2 The CLI supports common management actions such as starting or reconnecting to the daemon, refreshing state, and enabling or disabling managed domains using non-interactive commands.
- [x] #3 The CLI offers both human-readable output and a machine-readable mode suitable for scripts and agents.
- [x] #4 Permission-sensitive or unavailable-daemon operations return typed errors, stable exit codes, and actionable guidance instead of interactive UI prompts.
- [x] #5 Automated tests cover core commands, output modes, exit codes, and unavailable-daemon behavior.
- [x] #6 User documentation explains command structure, scripting patterns, and how this CLI relates to the TUI and future agent integrations.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add a new operator-facing CLI crate and workspace binary, recommended as `cadderctl`, instead of overloading `cadder`, because the architecture notes reserve `cadder` for a future desktop surface. Wire Clap command metadata, shared runtime arguments, and release identity consistently with the existing Cadder binaries.
2. Keep the CLI attach-only by default. Reuse `RuntimePaths`, `CadderClient`, `CadderSession`, and `ensure_daemon_running_with_options` from `crates/cadder-daemon` so only an explicit `daemon start` path may launch `cadderd`, while inspect and mutate commands only attach to an already running daemon.
3. Define the v1 command surface around the existing daemon contracts: `daemon status`, `daemon start`, `daemon shutdown`; `entrypoints list|enable|disable`; `domains list|enable|disable`; `diagnostics show`; `logs show`; and continuous `watch` or `tail` flows for the core inspection paths. Reuse current IPC request and response types instead of introducing a parallel backend contract.
4. Implement a small typed CLI application layer that resolves selectors, normalizes target lookup, and maps daemon/protocol failures into explicit operator-facing error kinds. Cover unavailable daemon, start timeout, target not found, rejected operation, unsupported platform or operation, permission or elevation issues, invalid usage, and serialization or IPC failures.
5. Define stable exit codes and keep them centralized and testable: success, invalid usage, daemon unavailable, daemon start failure or timeout, target not found, conflict or rejected operation, permission or elevation issue, unsupported platform or operation, and IPC or serialization failure. Ensure `Ctrl+C` exits streaming commands cleanly with success when the user intentionally stops them.
6. Implement stable output modes. Use human-readable output by default for one-shot commands, `json` for machine-readable one-shot results, and `jsonl` plus human streaming output for `watch` and `tail` commands. Keep machine-readable envelopes stable, explicit, and free of interactive prompts.
7. Build `watch` and `tail` on the current transport model instead of adding new daemon features to this task. Use `subscribe_state` for state-oriented watches, and implement log tailing through repeated `query_logs` calls with cursor progression, including explicit handling for stream status, gaps, retention truncation, and daemon loss while streaming.
8. Keep scope aligned with the milestone by focusing on core scripting and operator workflows. Do not require full TUI parity. Include state, entrypoints, domains, diagnostics, recent logs, start and shutdown, and enable or disable actions; only include IIS-specific CLI control if the existing contracts and time budget make it clearly part of the same bounded surface.
9. Update packaging and documentation alongside the binary. Extend workspace membership, portable dist and package verification, README usage, architecture wording where needed, and user docs so the shipped command set, output modes, scripting patterns, and the relationship between CLI, TUI, and future agent integrations stay aligned.
10. Validate with focused CLI unit and integration coverage plus the normal repository gates. Add tests for Clap help and version metadata, selector parsing, output rendering, exit-code mapping, unavailable-daemon behavior, state watch subscription handling, log tail cursor behavior, retention and gap reporting, and packaging expectations. Then run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo run -p xtask -- check`, and `cargo run -p xtask -- coverage`.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Plan approved by the user and recorded before implementation. Task moved to In Progress and assigned to @agent. Approved scope includes `watch` and `tail` support in the non-TUI CLI surface.

Confirmed that the current daemon IPC does not expose a dedicated refresh or reconnect mutation. The CLI implementation will stay within existing contracts by using one-shot `query_state` requests for explicit refreshes, `subscribe_state` for `watch`, and repeated `query_logs` cursor polling for `tail` instead of adding new daemon messages in this task.

Implemented a new `cadderctl` workspace crate for non-interactive Cadder inspection and control. The CLI now covers daemon status/start/shutdown, entrypoint and domain listing/toggles, diagnostics, retained log queries, log tailing, and state watches with `human`, `json`, and `jsonl` output modes.

The implementation stayed within existing daemon contracts: one-shot refreshes use `query_state`, `watch` uses `subscribe_state`, and `logs tail` advances by repeated `query_logs` cursor polling rather than adding new IPC messages. Packaging and docs were updated to ship and document `cadderctl` alongside the existing binaries.

Fresh-eyes review found and fixed two contract issues before closeout: top-level CLI flags are now global so documented command examples work even when `--output` or `--runtime-dir` appear after subcommands, and `cadderctl --help` / `--version` now exit with status 0 instead of invalid-usage status 2.

Verification run:
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo run -p xtask -- check`
- `cargo run -p xtask -- coverage`

Workspace line coverage now passes the project threshold at 87.42%.

Attempted an additional external Claude audit per repo guidance, but the local `claude` CLI invocation produced no output for an extended period and had to be aborted, so there are no external review findings to record from that pass.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Added a new operator-facing `cadderctl` workspace crate so Cadder can be inspected and controlled from scripts and non-interactive terminal workflows without opening `cadder-tui`.

What changed:
- Implemented `daemon status|start|shutdown`, `entrypoints list|enable|disable`, `domains list|enable|disable`, `diagnostics show`, `logs show`, `logs tail`, and `watch status|entrypoints|domains|diagnostics`.
- Kept the CLI attach-only by default and reused existing daemon contracts instead of introducing new IPC messages. One-shot refreshes use fresh `query_state` calls, `watch` uses `subscribe_state`, and `logs tail` advances via repeated `query_logs` cursor polling.
- Added stable `human`, `json`, and `jsonl` output modes plus centralized typed error handling and stable exit codes for unavailable daemon, invalid usage, target-not-found, rejected/conflict, permission/elevation, start failure, unsupported operations, and IPC failures.
- Extended workspace packaging and verification so `cadderctl` ships with releases and is covered by dist validation.
- Updated README, architecture docs, and docs site content with command structure, scripting patterns, runtime override examples, and the relationship between `cadderctl` and the TUI.

Why it changed:
- Operators, automations, and future agents need a stable non-TUI surface for querying daemon state, toggling routes, and retrieving logs without depending on an interactive dashboard.

Impact:
- Cadder now exposes a scriptable CLI surface suitable for shell usage, CI checks, and agent integration while preserving the existing daemon ownership and runtime model.

Tests and verification:
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo run -p xtask -- check`
- `cargo run -p xtask -- coverage`
- Workspace line coverage after the change: 87.42%

Key closeout fixes found during review:
- Made top-level CLI flags global so documented forms like `cadderctl logs show ... --output json` work after subcommands.
- Fixed `cadderctl --help` and `cadderctl --version` to exit with status `0` instead of invalid-usage status `2`.

Risks / follow-ups:
- No dedicated reconnect or refresh daemon mutation was added in this task by design; the CLI intentionally relies on fresh state queries, state subscriptions, and log cursor polling over the existing protocol.
- An additional external Claude audit was attempted but the local `claude` CLI produced no output and had to be aborted, so no external findings were available from that pass.
<!-- SECTION:FINAL_SUMMARY:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [x] #1 Tests or explicit verification were run for the changed behavior
- [x] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
