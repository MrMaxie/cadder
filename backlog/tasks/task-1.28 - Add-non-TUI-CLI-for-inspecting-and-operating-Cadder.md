---
id: TASK-1.28
title: Add non-TUI CLI for inspecting and operating Cadder
status: To Do
assignee: []
created_date: '2026-06-16 15:25'
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
- [ ] #1 The CLI can inspect daemon status, entrypoints, domains, diagnostics, and recent logs without launching the TUI.
- [ ] #2 The CLI supports common management actions such as starting or reconnecting to the daemon, refreshing state, and enabling or disabling managed domains using non-interactive commands.
- [ ] #3 The CLI offers both human-readable output and a machine-readable mode suitable for scripts and agents.
- [ ] #4 Permission-sensitive or unavailable-daemon operations return typed errors, stable exit codes, and actionable guidance instead of interactive UI prompts.
- [ ] #5 Automated tests cover core commands, output modes, exit codes, and unavailable-daemon behavior.
- [ ] #6 User documentation explains command structure, scripting patterns, and how this CLI relates to the TUI and future agent integrations.
<!-- AC:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [ ] #1 Tests or explicit verification were run for the changed behavior
- [ ] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
