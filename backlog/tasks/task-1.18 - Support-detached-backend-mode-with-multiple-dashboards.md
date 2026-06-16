---
id: TASK-1.18
title: Support detached backend mode with multiple dashboards
status: To Do
assignee: []
created_date: '2026-06-10 12:04'
updated_date: '2026-06-16 15:23'
labels: []
milestone: m-2
dependencies:
  - TASK-1.11
  - TASK-1.12
references:
  - docs/ARCHITECTURE.md
parent_task_id: TASK-1
priority: medium
ordinal: 18000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Add the future runtime architecture where entrypoints talk to a single per-user backend daemon and zero or more dashboard/TUI clients can attach independently. The current default remains the simpler single instance dashboard+backend model; this task should introduce the explicit detached backend/dashboard mode when the product is ready for it, including a user-facing detached startup option instead of requiring dashboard ownership.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Cadder supports a backend-only daemon mode that can run without owning an interactive dashboard lifecycle.
- [ ] #2 Multiple dashboard/TUI clients can connect to the same backend concurrently, query state, subscribe to changes, and exit without stopping the backend unless they explicitly request shutdown.
- [ ] #3 The default startup path remains dashboard+backend unless the user opts into detached backend mode.
- [ ] #4 CLI/config options clearly distinguish default dashboard+backend startup, dashboard-only attach, and backend-only detached startup, including an explicit detached flag or equivalent user-facing option.
- [ ] #5 Tests cover zero-to-many entrypoints with zero-to-many dashboard clients and verify dashboard disconnects do not remove entrypoint registrations or stop the backend.
- [ ] #6 Architecture and user documentation explain both runtime modes and when to use each one.
<!-- AC:END -->



## Definition of Done
<!-- DOD:BEGIN -->
- [ ] #1 Tests or explicit verification were run for the changed behavior
- [ ] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
