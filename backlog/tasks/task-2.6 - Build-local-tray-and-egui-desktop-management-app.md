---
id: TASK-2.6
title: Build local tray and GPUI desktop management app
status: To Do
assignee: []
created_date: '2026-06-11 16:35'
updated_date: '2026-06-16 15:43'
labels: []
milestone: m-2
dependencies:
  - TASK-1.18
documentation:
  - docs/ARCHITECTURE.md
parent_task_id: TASK-2
priority: medium
ordinal: 26600
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Build a local desktop management surface as a separate GPUI-based binary with a system tray presence and a management window that covers the same core local workflows as the TUI. For v1.0, this task is intentionally scoped to the existing local daemon model and should not assume remote daemon pairing, remote aggregation, or other v2.0-only capabilities. The desktop app should remain cross-platform, degrade cleanly where tray APIs are unavailable, and use the same daemon contracts as other Cadder clients. It should build on the detached-backend and multi-client runtime foundation from TASK-1.18 instead of introducing a separate ownership model. Remote-aware expansion of the desktop surface belongs to TASK-2.7 and later v2.0 work.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Cadder provides a separate GPUI desktop management binary with tray status/actions and a management window for local daemon status, entrypoints, domains, diagnostics, logs, and activation controls comparable to the TUI's core local workflows.
- [ ] #2 The GPUI app uses the existing daemon/client contracts instead of introducing a separate source of truth for Cadder state.
- [ ] #3 Unsupported or limited tray behavior on a platform is represented explicitly without breaking the management window or local-only workflows.
- [ ] #4 The v1.0 GPUI app remains fully usable without any remote daemon pairing model or remote profile support; remote-aware expansion is deferred to follow-up v2.0 tasks.
- [ ] #5 Automated tests cover desktop model state transitions, action routing, unsupported tray fallback behavior, and core local UI state rendering; manual smoke verification is documented for platform-specific tray behavior.
- [ ] #6 Architecture and user documentation explain GPUI app startup, platform behavior, tray actions, local management workflows, and how it relates to the TUI, while clearly deferring remote aggregation to later work.
<!-- AC:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [ ] #1 Tests or explicit verification were run for the changed behavior
- [ ] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->

## Comments

<!-- COMMENTS:BEGIN -->
author: Codex
created: 2026-06-16 15:41
---
Scope corrected during backlog intake: GPUI was moved into the v1.0 track, so this task now covers only the local desktop app surface. Remote-profile and paired-daemon UI behavior is intentionally deferred to TASK-2.7 and later v2.0 work.
---

author: Codex
created: 2026-06-16 15:43
---
Replaced the stale TASK-2.1 dependency with TASK-1.18 so the blocker matches the local multi-client runtime foundation rather than remote pairing work.
---
<!-- COMMENTS:END -->
