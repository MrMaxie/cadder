## Why

Preserve the existing project Caddyfile -> managed shim -> daemon -> operator
journey, including project/domain activation and rejected configuration changes.
The next capability serializes a complete configuration transaction through typed
adapters and exposes only a verified, durably committed snapshot. Failures retain
last-known-good or fence later mutations until runtime reconciliation succeeds.

## Requirement IDs

- `CADDY-003`: Serialized preparation, validation, apply, independent verification,
  persistence and publication; rejection, rollback and ambiguous-outcome fencing.

CADDY-002/004/006 and REG-002/004 supply composition and runtime dependencies;
STO-002 requires atomic persistence failure. This slice does not close their
whole-product acceptance or implement their missing backends.

## Scope

Implement roadmap 3.4-3.5 in a small daemon-owned transaction module with dedicated
concurrency/fault tests. Reuse CaddyConfig, DesiredState, registration DTOs, typed
ProtocolError/PortResult and existing validator/storage signatures. Introduce only
an internal runtime transaction boundary for explicit apply outcomes, authoritative
active observation and the idle/null target needed by first-apply rollback.

Preparation is a domain callback executed inside the same queue, so adaptation,
composition and handler checks cannot overlap a preceding transaction. Publication
is one internal committed-snapshot assignment after verification and persistence,
not an external notification callback that can fail after durable commit.

Completed protocol, preparation and composition changes remain unarchived because
of the known custom-schema validation blocker. This module consumes their leaf
contracts without modifying those sources. Only one source writer uses the
checkout; parent-owned planning/evidence is separate from worker-owned new files.

## Non-goals

No RPC/catalog/shared-port changes, product handlers, new Caddy parser, HTTP/Admin
API, certificates, secure config assembly, server process/lifecycle implementation,
SQLite application worker/schema, real installed Caddy, CLI/TUI, dependency,
packaging/tooling changes, Sandbox or host privilege/account actions. No production
mock or apply-success stub. Actual backends and product journeys remain later work.

## Success criteria

- Concurrent submissions serialize every phase and prepare against the newest
  committed snapshot; one failed operation cannot poison the queue tail.
- Caller/adapters cannot mutate queued, pending or committed state by sharing
  objects. Reads retain the old snapshot until verified persistence succeeds.
- Prepare/validation rejection and definite apply rejection preserve last-known-good.
- Successful apply is followed by independent active-state observation, never
  receipt-only success. Storage failure restores and verifies last-known-good,
  including the initially idle runtime, without publishing the rejected candidate.
- Unknown apply results, failed verification and failed rollback fence subsequent
  mutations. Reconciliation uses the same queue, observes/restores/verifies the
  last committed runtime target and alone may clear the fence.
- Focused tests cover phase order, controlled concurrent promises, all failures,
  null/first-apply rollback, read isolation, poisoning, fencing and recovery;
  full relevant Node checks pass above 85% own-code lines and MiMo findings are
  reconciled without claiming actual backend/native/release acceptance.

## Impact

New daemon transaction source and dedicated tests; focused OpenSpec planning and
evidence, then existing roadmap 3.4-3.5 completion rows only after their checks.
No new requirements, public commands, RPC methods or publication authority.
G1/G2/G3 and final-product Sandbox remain open.
