## Summary

Roadmap 3.4-3.5 orchestration and the focused implementation checks pass.
src/daemon/configuration-transactions.ts owns one queue from preparation through
internal committed-snapshot publication. Dedicated tests are in
test/configuration-transactions.test.ts. Required adapters perform actual
validation, runtime apply/readback and atomic DesiredState persistence; there are
no successful production defaults or mock backends.

ConfigurationTransactions exposes submit, reconcile, readCommitted, readFence
and close. Initial state must already be independently observed and durably
committed. This is not startup readiness, a registration handler, a Caddy process,
a protected Admin API implementation or a SQLite application worker.

Evidence applies to the uncommitted checkout based on HEAD
93942af84f555eab2bd68e0a4a9db487ff82d040. Existing dirty work is preserved.
The source/test additions and OpenSpec planning/evidence are the only surfaces
changed by this slice. No shared-port/RPC, dependency, Rust production, version, packaging,
host privilege/account, Sandbox, staging, commit or publication change occurred.

This is not archive-ready. OpenSpec 1.5 still rejects the four spec-free
implementation changes with the known missing-delta mismatch. CADDY-003 as a
whole and G1/G2/G3 remain open pending backend/product/native acceptance.

## Evidence

| Requirement ID | Task | Evidence | Result |
| --- | --- | --- | --- |
| CADDY-003 | 1.1 | `nub run test -- test/configuration-transactions.test.ts`: explicit phase order, controlled concurrent promises, preparation against the latest committed snapshot, in-queue pure composition consumption, settled queue tail and close/drain ordering. | Pass for orchestration; no real Caddy execution |
| CADDY-003 | 1.2 | Same suite: pending reads stay old during every phase; initial/caller/adapter/return snapshots are private and frozen; strict candidate shape/hash/byte checks; validator hash mismatch; rejection/throws redact exception text and preserve the tail. | Pass; publication is one internal assignment after persistence |
| CADDY-003 | 2.1 | Same suite: rejected/thrown/invalid atomic persistence results restore complete last-known-good or idle/null, independently observe it and never publish the rejected candidate; rollback failure matrix sets a fence while retaining the storage failure. | Pass under STO-002 atomic-failure contract; SQLite backend not implemented |
| CADDY-003 | 2.2 | Same suite: explicit definite rejection versus ambiguous/throw/unknown/receipt-shaped apply outcomes, observation errors/mismatches, FIFO fenced refusal, explicit queued reconciliation, restoration failures and verified post-recovery changes. | Pass under truthful independent readback contract |
| CADDY-003 | 2.2 | Added first-apply regression: candidate is actually live in the fake adapter, unavailable observation remains fenced despite idle last-known-good, later truthful readback restores/verifies idle without publishing/persisting the abandoned candidate. | Pass; null means positively observed idle, never unknown |
| CADDY-003 | 3.1 | MiMo Flash plan critique, MiMo Pro source/test review, parent contract clarification/fresh-eyes pass and final focused/full Node/schema/main/roadmap/diff checks below. | Slice checks pass; global/archive/backend gates remain open |

Final focused suite: **65 tests pass**. Worker increments passed 22 then 64 tests;
the parent added the idle/unknown-observation case and reran the full focused suite.

Final parent `nub run check` passes TypeScript, ESLint, Prettier and V8 coverage:
**480 tests pass, two existing Unix-native tests skip, 28 test files pass**.
Own TS/TSX coverage is **1155/1191 lines (96.97%)**. Coverage-summary inspection
confirms configuration-transactions.ts has **92/93 lines (98.92%)** and
**50/50 branches (100%)**. The uncovered line is the queue-tail rejection
continuation, not a claimed 100% line/function result. Generated coverage remains
outside the checkout.

## Independent review and disposition

MiMo Flash raised three plan-level concerns:

- Reconciliation admission: the implementation has an explicit queued reconcile
  method, not automatic retries. FIFO work before reconciliation is refused while
  fenced; a later operation runs only after verified clearing. The design now
  states who calls/enqueues reconciliation and does not introduce another RPC.
  Flash's proposed automatic recovery was not adopted as product authority.
- Unknown observation versus idle: MiMo Pro confirmed one P2 wording omission.
  The source interface and design now require null only for positively observed
  idle; undeterminable state must be an observation failure. The added regression
  exercises that failure while an ambiguous first candidate is live and prevents
  false fence clearing. No backend implementation or algorithm widening was added.
- Typed apply failures: only an explicit definitely-rejected outcome guarantees
  no transition. Throws, malformed/receipt-shaped results and outcomes without
  that guarantee are ambiguous; ProtocolError kinds are not certainty evidence.
  Source schema/tests already enforce this, and design wording is now explicit.

MiMo Pro independently read completed source/tests and returned **OK with notes**,
no P0/P1 and the single P2 contract-wording item above. The parent resolved that
item and tested it; this does not claim a separate later reviewer verdict.
The reviewer confirmed single flight, immutable candidates, independent readback,
DesiredState-only persistence, publication point, initial idle rollback, fencing
and bounded error settlement. Both reviews completed in the native workflow.

Parent simplification/fresh-eyes inspection retained one domain-specific owner
rather than splitting a coherent transaction across a generic framework. No
additional substantive defect was found. All fake adapters live only in tests;
no test-only seam became a production success path.

## Gate

- [x] Every focused task and its stated orchestration checks is complete.
- [x] In-scope CADDY-003 aspects have evidence above.
- [x] Focused and full Node checks pass above 85% own-code lines.
- [x] `openspec schema validate implementation` passes.
- [x] `openspec validate --specs --strict --no-interactive`: 15 pass.
- [x] `openspec validate reset-cadder-architecture --strict --no-interactive` passes.
- [x] `git diff --check` passes; nothing is staged; no generated JS/DLL/EXE appears in src/test.
- [ ] `mise run openspec-check` passes: currently 17 items pass and four spec-free implementation changes fail strict delta validation.
- [ ] Archive is authorized and validation-compatible.

OpenSpec verification instructions returned the known custom-schema unknown
`specs` rule warning along with the usable artifact instructions. No fake deltas,
checker replacement or validation-policy workaround was added. Active LSP probes
were not uniformly conclusive and auxiliary rules reported runtime-type and
large-class hints; maintained TypeScript/lint checks pass, not an LSP-clean claim.
The combined Rust/docs/npm/cargo-dist gate was not rerun for this Node-only slice;
previous Rust baseline evidence remains separate, not a new passing combined gate.

## Residual risk

Actual secure Caddy apply/readback and idle transitions must implement the internal
runtime boundary in 3.6-3.9. Hash readback must be independent and comparably
normalized; unavailable observation cannot be reported as idle. The future storage
worker must prove STO-002 atomic failure, including thrown/rejected persistence;
these test fakes do not prove SQLite commit or durability.

The daemon caller must establish the independently observed/durable initial state
and explicitly enqueue reconciliation when recovering from a fence. Future
registration/heartbeat handlers must join or coordinate with the same ownership
boundary; this slice does not prove live lease merging or product RPC integration.
A fenced diagnostic describes recovery, but is not permission to automatically
retry an uncertain user operation. Phase adapters own bounded settlement: the
queue does not start later side effects while earlier work remains active.

Native product, real Caddy, mTLS denial, distribution and final-product Windows
Sandbox acceptance remain open. The released Rust 1.0.5 operational baseline is
unchanged. No migration-complete or release-readiness claim is made.
