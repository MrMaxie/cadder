---
id: TASK-1.29
title: Expose Cadder through an MCP server for agents
status: Done
assignee:
  - '@agent'
created_date: '2026-06-16 15:25'
updated_date: '2026-06-16 19:39'
labels:
  - mcp
  - agent
  - automation
milestone: m-2
dependencies: []
references:
  - crates/cadder-daemon/src/ipc.rs
  - crates/cadder-protocol/src/lib.rs
  - crates/cadderd/src/main.rs
documentation:
  - docs/ARCHITECTURE.md
modified_files:
  - Cargo.toml
  - Cargo.lock
  - crates/cadder-operator/src/lib.rs
  - crates/cadder-operator/src/error.rs
  - crates/cadder-operator/src/view.rs
  - crates/cadder-operator/src/service.rs
  - crates/cadder-mcp/Cargo.toml
  - crates/cadder-mcp/src/lib.rs
  - crates/cadder-mcp/src/main.rs
  - crates/cadder-mcp/src/dto.rs
  - crates/cadder-mcp/src/redaction.rs
  - crates/cadder-mcp/tests/support/mod.rs
  - crates/cadder-mcp/tests/mcp.rs
  - crates/cadderctl/Cargo.toml
  - crates/cadderctl/src/app.rs
  - crates/cadderctl/src/error.rs
  - crates/cadderctl/src/view.rs
  - xtask/src/main.rs
  - README.md
  - docs/ARCHITECTURE.md
  - docs/site/astro.config.mjs
  - docs/site/src/content/docs/index.mdx
  - docs/site/src/content/docs/quick-start/getting-started.mdx
  - docs/site/src/content/docs/user-guide/how-to-use.mdx
  - docs/site/src/content/docs/user-guide/cadderctl.mdx
  - docs/site/src/content/docs/user-guide/cadder-mcp.mdx
parent_task_id: TASK-1
priority: high
ordinal: 25400
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Provide an MCP server so AI agents can inspect Cadder state and perform safe management actions through a stable tool surface instead of screen-driving the TUI. The MCP surface should expose daemon status, entrypoints, domains, diagnostics, logs, and carefully bounded control operations with redacted outputs and clear failure states.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 MCP tools can inspect daemon health, registrations, domains, diagnostics, recent logs, and configuration summaries without requiring the TUI.
- [x] #2 Safe management actions are exposed for the core non-interactive workflows, with typed results, redacted failures, and bounded output sizes.
- [x] #3 The MCP surface works against the local daemon model by default and documents any trust, authentication, or exposure boundaries required for agent use.
- [x] #4 Secrets and sensitive paths are redacted consistently from tool results, logs, and error messages.
- [x] #5 Automated tests cover tool contracts, unavailable-daemon behavior, redaction, and action routing.
- [x] #6 Agent-facing documentation explains how to start the MCP server, what tools it exposes, and when to prefer CLI or TUI instead.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
# MCP v1 Plan

## Goal
Expose Cadder to local AI agents through a minimal, stable MCP server that reuses the existing `cadderctl` and daemon IPC model instead of introducing a parallel backend contract or screen-driving the TUI.

## Milestone Fit
- Keep this task within milestone `m-2` by building on the already shipped `cadderctl` command model and current daemon IPC contracts.
- Preserve the local per-user daemon trust model. MCP v1 must remain a local `stdio` server that talks to the same daemon runtime directory model as `cadderctl`.
- Do not expand this task into remote access, authentication systems, full TUI parity, or a new daemon-side streaming protocol.

## Scope Boundaries
Included in v1:
- Local MCP server binary and workspace wiring.
- Agent-facing inspect tools for daemon overview, entrypoints, domains, diagnostics, and recent logs.
- Bounded management tools for explicit daemon start plus entrypoint and domain activation toggles.
- Shared redaction rules for tool results, errors, and sensitive path exposure.
- Contract and integration tests for tool schemas, unavailable-daemon behavior, redaction, and action routing.
- Agent-facing documentation for startup, trust boundaries, tool usage, and when to prefer CLI or TUI.

Explicitly out of scope for this task:
- Remote TCP or HTTP MCP transport.
- New daemon IPC messages unless implementation proves an existing contract is insufficient.
- IIS handoff MCP control.
- `watch` and `logs tail` streaming MCP tools.
- Daemon shutdown through MCP v1.
- Full TUI parity or unrestricted proxying to the real Caddy admin API.

## Key Modules And Files
- `Cargo.toml`
- `crates/cadder-daemon/src/ipc.rs`
- `crates/cadder-daemon/src/lib.rs`
- `crates/cadder-daemon/src/logs.rs`
- `crates/cadder-protocol/src/lib.rs`
- `crates/cadderctl/src/app.rs`
- `crates/cadderctl/src/view.rs`
- `crates/cadderctl/src/error.rs`
- New `crates/cadder-mcp/` crate for the MCP server
- `README.md`
- `docs/ARCHITECTURE.md`
- Docs site pages under `docs/site/src/content/docs/`
- `xtask/src/main.rs` if packaging or verification expectations need updating

## Implementation Steps
1. Extract a shared Cadder operator service layer from `cadderctl` so MCP and CLI can reuse the same attach, query, toggle, log-target resolution, daemon start, and typed error-mapping behavior without shelling out to `cadderctl`.
2. Keep the existing `CadderClient`, `CadderSession`, `StateSubscription`, and daemon IPC request and response types as the primary transport boundary. Reuse existing `query-state`, `query-logs`, `set-entrypoint-enabled`, `set-domain-enabled`, and explicit daemon start behavior rather than designing a second internal control path.
3. Create a new local-only `cadder-mcp` crate and binary that serves MCP over `stdio`. Accept runtime configuration inputs equivalent to the CLI runtime selection model, but do not expose any network listener in v1.
4. Define a minimal safe MCP tool surface using agent-facing DTOs rather than returning the raw daemon snapshot shape. The initial tool set should be:
   - `cadder_get_overview`
   - `cadder_list_entrypoints`
   - `cadder_list_domains`
   - `cadder_show_diagnostics`
   - `cadder_get_logs`
   - `cadder_start_daemon`
   - `cadder_set_entrypoint_enabled`
   - `cadder_set_domain_enabled`
5. Keep inspect tools attach-only. `cadder_start_daemon` is the only MCP tool that may launch `cadderd`. All other tools must return typed unavailable-daemon failures with guidance instead of causing side effects.
6. Reuse or adapt `cadderctl` view-building logic so MCP returns compact, stable, task-oriented payloads for overview, entrypoints, domains, diagnostics, and logs. Avoid exposing raw internal fields that are unnecessary for agent workflows.
7. Add bounded inputs and outputs for management and inspect tools. At minimum, validate log limits, selector ambiguity, and runtime path handling, and cap log result sizes so tool responses remain predictable for agents.
8. Implement MCP-specific redaction on top of the existing daemon log redaction. Preserve the current token-like log scrubbing, then additionally redact or normalize sensitive path and process detail fields before they are emitted through MCP.
9. Define path-redaction behavior explicitly. Prefer workspace-relative paths when the target is inside the repo or selected runtime context; otherwise return a reduced or redacted representation that remains useful for diagnostics without leaking unnecessary host-specific details.
10. Apply the same redaction discipline to tool success payloads, diagnostics, and typed failures. Error messages, guidance, configuration path summaries, and command metadata must not leak raw secrets or unnecessarily sensitive absolute paths.
11. Document the MCP trust boundary clearly: local `stdio` MCP server, same-user daemon assumptions, no remote exposure in v1, explicit daemon start semantics, and the intended division of responsibilities between MCP, `cadderctl`, and `cadder-tui`.
12. Update release and operator documentation so contributors and agents can discover the MCP binary, understand the tool names, and know when to use MCP versus CLI or TUI.

## Redaction Requirements
- Reuse daemon-side redaction for retained log messages from `crates/cadder-daemon/src/logs.rs`.
- Redact token-like values, credentials, authorization fragments, and similar sensitive substrings from any MCP-visible free-text field.
- Redact or normalize absolute paths, executable paths, command lines, admin endpoints, and similar host-sensitive fields unless they are strictly necessary for safe agent operation.
- Ensure tool errors and guidance go through the same redaction path as normal tool results.
- Add regression tests that prove the same secret or path pattern is not exposed through logs, diagnostics, or failure payloads.

## Contract Test Requirements
Add automated coverage at three levels:
1. Unit tests for shared operator service behavior:
- selector resolution
- bounded log option validation
- typed unavailable-daemon and rejected-operation mapping
- redaction helpers for strings and paths

2. MCP contract tests:
- stable tool names and descriptions
- stable input validation behavior
- stable result envelope shapes for inspect and action tools
- typed error payloads for daemon unavailable, ambiguous selectors, unsupported operations, and rejected actions

3. Integration tests using a local harness similar to `cadderctl` tests:
- inspect tools return expected data from the existing daemon IPC model
- management tools route to the correct daemon actions
- unavailable-daemon cases remain non-interactive and typed
- redaction is preserved in logs, diagnostics, and failures
- one `stdio` MCP smoke path covers initialize, tool discovery, and at least one inspect plus one action call

## Validation
Run focused tests while implementing, then the normal repository gates before closeout:
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo run -p xtask -- check`
- `cargo run -p xtask -- coverage`

## Risks And Watchpoints
- Avoid duplicating `cadderctl` logic and drifting tool semantics between CLI and MCP.
- Avoid leaking host-specific or secret-bearing fields through apparently harmless diagnostics.
- Keep v1 tool scope intentionally small so tool names and schemas can stay stable.
- If implementation reveals that a current IPC response shape is too raw for safe MCP exposure, adapt through shared view or service DTOs first before proposing new daemon messages.
- If a necessary workflow would require broader actions such as shutdown or IIS mutation, stop and request an explicit scope decision instead of silently expanding the task.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Plan approved and recorded before implementation. MCP v1 is intentionally limited to inspect plus bounded management actions on top of existing `cadderctl` and daemon IPC contracts; daemon shutdown, streaming tools, and IIS control remain out of scope unless explicitly approved later.

Extracted a shared `cadder-operator` layer from `cadderctl` so CLI and MCP reuse the same daemon attach/query/toggle/log-target resolution logic and typed error mapping instead of drifting through separate implementations.

Implemented `cadder-mcp` as a local stdio MCP server with bounded inspect and management tools, MCP-specific path/text redaction, and explicit daemon-start semantics limited to `cadder_start_daemon`.

Updated release packaging and documentation to include `cadder-mcp`, plus validation for the portable layout and agent-facing docs about trust boundaries and when to prefer MCP versus CLI or TUI.

Validation run after the final code changes: `cargo test -p cadder-operator`, `cargo test -p cadder-mcp`, `cargo test -p cadderctl --lib`, `cargo test -p xtask`, `cargo run -p xtask -- check`, `cargo run -p xtask -- coverage`, `bun run check`, and `bun run build` in `docs/site`. Workspace line coverage remained above the gate at 85.65% in `target/llvm-cov/coverage-summary.json`.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
## Summary
Added a new local stdio `cadder-mcp` server so agent hosts can inspect Cadder state and perform bounded management actions through a stable MCP tool surface instead of shelling out to `cadderctl` or screen-driving the TUI.

## What Changed
- extracted a shared `cadder-operator` crate from `cadderctl` for daemon attach/query/toggle/log-target resolution, daemon start policy, typed error mapping, and shared state/view shaping
- refactored `cadderctl` to reuse the shared operator layer instead of keeping its own copies of the same logic
- added the `cadder-mcp` crate and binary with overview, entrypoint, domain, diagnostics, logs, daemon-start, and activation-toggle tools over local stdio
- added MCP-specific redaction for paths, free-form text, endpoints, and bounded log responses
- updated workspace packaging and `xtask` verification so portable layouts include `cadder-mcp`
- documented the MCP trust boundary, startup flow, tool surface, and when to use MCP versus CLI or TUI in the README, architecture notes, and docs site

## Why
This gives AI agents a stable, structured local control surface for Cadder while preserving the existing per-user daemon model, keeping daemon startup explicit, and avoiding duplicate business logic between CLI and MCP clients.

## Validation
- `cargo test -p cadder-operator`
- `cargo test -p cadder-mcp`
- `cargo test -p cadderctl --lib`
- `cargo test -p xtask`
- `cargo run -p xtask -- check`
- `cargo run -p xtask -- coverage`
- `bun run check` in `docs/site`
- `bun run build` in `docs/site`

## Risks / Follow-ups
MCP v1 intentionally excludes streaming watch/tail tools, daemon shutdown, and IIS handoff control. Those should remain separate follow-up scope so the initial tool names and schemas stay stable.
<!-- SECTION:FINAL_SUMMARY:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [x] #1 Tests or explicit verification were run for the changed behavior
- [x] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
