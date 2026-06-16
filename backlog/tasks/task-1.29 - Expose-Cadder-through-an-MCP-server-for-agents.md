---
id: TASK-1.29
title: Expose Cadder through an MCP server for agents
status: To Do
assignee: []
created_date: '2026-06-16 15:25'
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
- [ ] #1 MCP tools can inspect daemon health, registrations, domains, diagnostics, recent logs, and configuration summaries without requiring the TUI.
- [ ] #2 Safe management actions are exposed for the core non-interactive workflows, with typed results, redacted failures, and bounded output sizes.
- [ ] #3 The MCP surface works against the local daemon model by default and documents any trust, authentication, or exposure boundaries required for agent use.
- [ ] #4 Secrets and sensitive paths are redacted consistently from tool results, logs, and error messages.
- [ ] #5 Automated tests cover tool contracts, unavailable-daemon behavior, redaction, and action routing.
- [ ] #6 Agent-facing documentation explains how to start the MCP server, what tools it exposes, and when to prefer CLI or TUI instead.
<!-- AC:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [ ] #1 Tests or explicit verification were run for the changed behavior
- [ ] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
