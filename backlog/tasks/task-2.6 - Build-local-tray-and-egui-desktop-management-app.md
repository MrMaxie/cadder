---
id: TASK-2.6
title: Build local tray and GPUI desktop management app
status: To Do
assignee: []
created_date: '2026-06-11 16:35'
updated_date: '2026-06-16 15:23'
labels: []
milestone: m-2
dependencies:
  - TASK-2.1
documentation:
  - docs/ARCHITECTURE.md
parent_task_id: TASK-2
priority: medium
ordinal: 26600
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Build a local desktop management surface as a separate GPUI-based binary with a system tray presence and a management window that covers the same core workflows as the TUI. The desktop app should remain cross-platform, degrade cleanly where tray APIs are unavailable, and use the same daemon contracts as other Cadder clients.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Cadder provides a separate GPUI desktop management binary with tray status/actions and a management window for daemon status, entrypoints, domains, diagnostics, logs, and activation controls comparable to the TUI's core workflows.
- [ ] #2 The GPUI app uses the existing daemon/client contracts instead of introducing a separate source of truth for Cadder state.
- [ ] #3 Unsupported or limited tray behavior on a platform is represented explicitly without breaking the management window or local-only workflows.
- [ ] #4 Remote daemon pairing status can be displayed when remote profiles exist, while the app remains useful with only the local daemon configured.
- [ ] #5 Automated tests cover desktop model state transitions, action routing, unsupported tray fallback behavior, and core UI state rendering; manual smoke verification is documented for platform-specific tray behavior.
- [ ] #6 Architecture and user documentation explain GPUI app startup, platform behavior, tray actions, window workflows, and how it relates to the TUI and web dashboard.
<!-- AC:END -->



## Definition of Done
<!-- DOD:BEGIN -->
- [ ] #1 Tests or explicit verification were run for the changed behavior
- [ ] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
