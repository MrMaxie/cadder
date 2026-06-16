---
id: TASK-1.26
title: Add OS-native autostart modes for daemon-only and daemon+tray startup
status: To Do
assignee: []
created_date: '2026-06-16 15:24'
labels:
  - startup
  - runtime
  - desktop
milestone: m-2
dependencies:
  - TASK-1.18
  - TASK-2.6
references:
  - crates/cadder-daemon/src/runtime.rs
  - crates/cadderd/src/main.rs
  - crates/cadder-tui/src/main.rs
documentation:
  - docs/ARCHITECTURE.md
  - docs/site/src/content/docs/reference/runtime-configuration.mdx
parent_task_id: TASK-1
priority: medium
ordinal: 25100
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Add operator-configurable autostart that uses native per-user startup mechanisms on supported platforms. Cadder should support starting only `cadderd` for background use and starting `cadderd` together with the tray/GUI app for users who want resident controls, while keeping setup, removal, and degraded-platform behavior explicit.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Users can inspect the current autostart mode and enable, disable, or change it without manually editing startup files or registry entries.
- [ ] #2 Cadder supports two explicit per-user autostart modes: daemon-only and daemon plus tray/GUI.
- [ ] #3 Autostart setup uses each platform's native startup mechanism where supported and cleans up stale entries when the mode changes or is disabled.
- [ ] #4 Unsupported or partially supported platform behavior returns clear diagnostics and never silently reports success.
- [ ] #5 Automated tests cover configuration or state transitions and startup-registration generation; manual smoke verification is documented for platform-specific startup behavior.
- [ ] #6 Architecture and user documentation explain the available modes, platform differences, and how autostart interacts with detached daemon mode.
<!-- AC:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [ ] #1 Tests or explicit verification were run for the changed behavior
- [ ] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
