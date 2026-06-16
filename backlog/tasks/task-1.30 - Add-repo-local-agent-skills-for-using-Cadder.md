---
id: TASK-1.30
title: Add repo-local agent skills for using Cadder
status: To Do
assignee: []
created_date: '2026-06-16 15:25'
labels:
  - agent
  - documentation
  - skills
milestone: m-2
dependencies:
  - TASK-1.28
  - TASK-1.29
references:
  - .agents/skills
  - AGENTS.md
documentation:
  - docs/ARCHITECTURE.md
  - AGENTS.md
parent_task_id: TASK-1
priority: medium
ordinal: 25500
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Add repo-local agent skills that teach coding agents how to inspect, operate, and debug Cadder through the supported interfaces instead of guessing from repository structure or driving the TUI blindly. The skills should cover the preferred CLI and MCP workflows, safety boundaries, verification steps, and fallbacks when an interface is unavailable.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Repo-local skills are added under `.agents/skills` for the supported Cadder operator workflows, including state inspection, lifecycle control, logs and diagnostics, and common recovery actions.
- [ ] #2 The skills prefer documented CLI and MCP surfaces where available and explain fallback behavior when an interface is unavailable.
- [ ] #3 Skill instructions cover safety constraints, environment or setup expectations, and verification commands for the workflows they describe.
- [ ] #4 Example prompts or usage snippets are included so future agents can discover and invoke the skills consistently.
- [ ] #5 A documented check or automated test keeps the skill instructions aligned with shipped commands and tool names.
- [ ] #6 Agent-facing documentation explains where the skills live and how contributors should maintain them.
<!-- AC:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [ ] #1 Tests or explicit verification were run for the changed behavior
- [ ] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
