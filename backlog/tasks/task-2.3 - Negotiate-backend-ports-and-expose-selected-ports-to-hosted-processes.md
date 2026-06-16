---
id: TASK-2.3
title: Negotiate backend ports and expose selected ports to hosted processes
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
priority: high
ordinal: 26300
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Add dynamic backend port negotiation so multiple projects that prefer the same backend port can still be hosted by Cadder. Projects should negotiate an available port before hosting, Cadder should route Caddy to the negotiated backend without requiring Caddyfile rewrites, and hosted processes should receive the selected values through documented `CADDER_*` environment variables.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Projects can request a preferred backend port and receive either that port or a deterministic available fallback when the preferred port is already reserved.
- [ ] #2 Cadder records active port reservations with ownership, route, daemon provenance, and lifecycle state so reservations are released when the owning registration ends.
- [ ] #3 Generated Caddy routing uses the negotiated backend endpoint without requiring users to rewrite their source Caddyfile after negotiation.
- [ ] #4 Hosted processes can read documented `CADDER_*` environment variables for the negotiated port and related route metadata.
- [ ] #5 Conflicts, exhausted ranges, stale reservations, and unsupported remote capabilities return typed diagnostics instead of silently choosing unsafe ports.
- [ ] #6 Automated tests cover preferred-port success, same-port conflict fallback, reservation release and reuse, stale owner cleanup, Caddy routing to negotiated ports, and environment variable contents.
- [ ] #7 Architecture and user documentation explain the negotiation flow, environment variables, local and remote behavior, and migration path for projects that currently hardcode backend ports.
<!-- AC:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [ ] #1 Tests or explicit verification were run for the changed behavior
- [ ] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
