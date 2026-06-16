---
id: TASK-1.27
title: Introduce durable runtime state and history storage
status: To Do
assignee: []
created_date: '2026-06-16 15:25'
labels:
  - runtime
  - persistence
  - storage
milestone: m-2
dependencies: []
references:
  - crates/cadder-daemon/src/state.rs
  - crates/cadder-daemon/src/logs.rs
  - crates/cadder-daemon/src/paths.rs
  - crates/cadder-protocol/src/lib.rs
documentation:
  - docs/ARCHITECTURE.md
parent_task_id: TASK-1
priority: medium
ordinal: 25200
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Improve how Cadder stores time-based state so long-lived daemon sessions, diagnostics, logs, and future automation surfaces can retain useful history across restarts. Evaluate SQLite as the default per-user backing store and adopt it if it is the safest fit; otherwise document and implement the chosen alternative with clear migration and retention rules.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Cadder persists core runtime state and selected history across daemon restarts with explicit retention and redaction rules.
- [ ] #2 The storage backend decision is recorded for future contributors; if SQLite is adopted, schema and migration rules are documented, and if not, the chosen alternative and rationale are documented.
- [ ] #3 Daemon startup and upgrades handle missing, existing, and incompatible on-disk state safely with actionable recovery guidance.
- [ ] #4 Query surfaces can retrieve recent historical registrations, diagnostics, or logs without depending on the original shim process still being alive.
- [ ] #5 Automated tests cover cold start, restart recovery, retention cleanup, migration or compatibility behavior, and secret redaction.
- [ ] #6 Architecture and user documentation explain storage location, lifecycle, retention, backup or reset behavior, and platform considerations.
<!-- AC:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [ ] #1 Tests or explicit verification were run for the changed behavior
- [ ] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
