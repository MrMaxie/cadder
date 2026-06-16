---
id: TASK-2.7
title: 'Aggregate local and paired remote entities across TUI, web, and desktop UI'
status: To Do
assignee: []
created_date: '2026-06-11 16:35'
updated_date: '2026-06-16 15:41'
labels: []
milestone: m-3
dependencies:
  - TASK-2.1
  - TASK-2.5
  - TASK-2.6
documentation:
  - docs/ARCHITECTURE.md
parent_task_id: TASK-2
priority: high
ordinal: 26700
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Extend Cadder management clients, including the v1.0 local GPUI desktop app, to resolve and display both local and paired remote entities in one coherent model. TUI, web, and desktop surfaces should show local and remote daemons, entrypoints, domains, logs, diagnostics, DNS and certificate status with clear provenance, and route actions to the daemon that owns each entity.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 TUI, web, and desktop management surfaces can display local and paired remote daemons, entrypoints, domains, logs, diagnostics, and relevant DNS/certificate status in a combined view.
- [ ] #2 Every entity shown in a management client includes provenance that distinguishes local state from each paired remote daemon.
- [ ] #3 Actions such as toggling a domain, refreshing logs, viewing diagnostics, or managing DNS/cert state are routed to the daemon that owns the selected entity.
- [ ] #4 Unreachable, unauthorized, stale, and version-incompatible remotes remain visible with clear degraded states instead of disappearing silently.
- [ ] #5 Duplicate or conflicting route hosts across local and remote daemons are surfaced clearly enough for operators to understand which daemon owns each route.
- [ ] #6 Shared UI model tests cover aggregation, provenance labels, action routing, stale/unreachable remotes, duplicate-domain clarity, and local-only behavior with no remote profiles.
- [ ] #7 Architecture and user documentation describe dynamic local/remote resolving, supported management surfaces, provenance rules, and degraded remote states.
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
Clarified relationship during backlog intake: TASK-2.6 now delivers the local-only GPUI surface for v1.0, while this task remains the follow-up for remote-aware aggregation and paired-daemon UI behavior in v2.0.
---
<!-- COMMENTS:END -->
