# Acceptance and scope ledger

This is the implementation navigation map, not test evidence. `tasks.md` owns
checkboxes; `verification.md` owns executed results. Read this alongside
`node-migration.md`, `design.md` and the delta specs. No completion percentage
substitutes for a missing requirement.

## Source of truth and dependencies

The current released Rust 1.0.5 product and main specs define functional scope.
The user corrected the migration to preserve that product in TypeScript, not
restore superseded history, IIS, autostart or machine-output workflows. Older
snapshots may explain retained behavior but cannot expand the accepted scope.

Main specs remain the 1.0 baseline until the reviewed migration deltas are synced
through the contract archive workflow before further product implementation.
Archiving those deltas accepts the target, not the unfinished implementation
roadmap. Preserve open tasks as planning. Future implementation slices reference
stable requirement IDs and record evidence rather than duplicating this roadmap.

| Approved scope | Contract and design owner | Tasks / exit |
| --- | --- | --- |
| One TS product; no Rust/Node IPC; daemon state owner | TOP-001/002, IPC-002/003; module table | 1.3-1.5, 5.10, 8.3-8.7 |
| Installation-specific, owner-protected v2 endpoints/databases | TOP-001, RUN-001; runtime identity | 1.3, 2.5, 7.2 |
| Equal npm and standalone SEA, three entries, same version | DIST-001/002/004/005 | 6.1-6.7, 7.6, 9.2-9.4 |
| npm Node 24 LTS >=24.18.0, separate pinned Node 26 SEA builder | DIST-004, QT-001; build policy | 1.2, 6.1-6.4 |
| TS/TSX ESM; no tracked generated JS; tsc/npm and esbuild/SEA in external staging | QT-003/006 | 6.1-6.5, 7.5, 8.6 |
| Exact dependencies, Nub, TS 6.0.3, Vitest/V8, Ink tests, ESLint/Prettier | QT-001/002/003; library decisions | 1.2, 6.8, 7.1 |
| Preserve released TOML/Caddyfiles, precedence and trusted selectors | REG-004, CADDY-001/005, RUN-004; behavior catalog | 1.3, 3.1-3.2, 5.6 |
| Owner-protected secret, protocol 3/security 2, HMAC, no unsafe ACL repair | IPC-001/002, RUN-005 | 2.1-2.5, 2.7-2.9, 7.3 |
| Lifetime SQLite exclusion, diagnostic metadata, recovery after actual lock | RUN-001; lock design | 2.2, 2.5-2.9 |
| Same-owner ordinary client/elevated daemon; explicit Unix owner; deny other users/remote | RUN-005, IPC-001 | 2.7-2.9, 7.3-7.4 |
| Retain exactly eight product operations with closed Zod schemas | IPC-003; current IPC catalog | 1.3, 1.5, 2.4, 5.5 |
| Serial config queue, last-known-good and ambiguous-apply reconciliation | CADDY-003, REG-002 | 3.3-3.5, 3.9, 7.2 |
| Loopback mTLS admin, internal CA, no OS trust/plain admin/project override | CADDY-006 | 3.6-3.7, 7.3 |
| Owned Caddy only, graceful then bounded forced shutdown | CADDY-002, RUN-003 | 3.8-3.9, 7.2 |
| Retained shim command policy, argv/streams/status and heartbeat cleanup | SHIM-001/002/003, REG-001/003 | 4.1-4.3, 4.10, 6.7, 7.2 |
| Operator port/Caddyfile inspection and explicit PID revalidation | INSPECT-001..004, CLI-001 | 1.3, 4.4, 4.10, 5.6 |
| One SQLite state/log worker; no old-data import or live-session restoration | STO-001..004, REG-003 | 5.1-5.2, 9.1 |
| Bounded redaction-before-write logs, one canonical stream, CLI-only diagnostics | OBS-001..004 | 5.3-5.4, 7.1 |
| Shared client/models; real state; current lifecycle/projects/domains/inspection | TOP-002, CLI-001, TUI-001/003 | 5.5-5.10, 6.7 |
| Human-readable output; state/inspection attach-only; explicit bounded launch paths | CLI-001, RUN-002 | 4.2, 5.6-5.7 |
| Bare help with/without TTY; cadder tui and --start-daemon; offline start | CLI-001, TUI-003/004 | 5.7-5.9 |
| Routes-first TUI, tree rows, quiet header, Space/Enter and terminal recovery | TUI-001..004 | 5.8-5.9 |
| Reject profiles, machine output, history, export, tail, watch, IIS and autostart | CLI-001, TOP-002; explicit non-goals | 1.3, 5.6, 7.4-7.6 |
| Single tarball; no optional native packages/install build/download/real Caddy | DIST-004, QT-006 | 6.1-6.2, 8.2-8.4 |
| Four SEA variants, native CLI/TUI/state-log worker/child smoke without Node/npm PATH | DIST-001/002, QT-006 | 6.3-6.7 |
| Astro/Starlight docs, Nub workflow, equal verified installation guidance | DOC-001..004 | 6.8-6.9, 8.5 |
| >=85% own line coverage; contracts/fakes and native runtime/product integration | QT-003/004 | 7.1-7.3 |
| Real Windows Sandbox UAC and account-boundary acceptance, no IIS/autostart mutation | RUN-005, IPC-001 | 2.8, 7.4 |
| Both channels pass identical current-scope scenarios; clean packaging | DIST-005, QT-006 | 6.7, 7.5-7.6, 8.6 |
| Remove Rust/Cargo/old packaging after parity; preserve unrelated untracked npm content | QT-005, TOP-001 | 8.1-8.7 |
| Stop old daemon, re-register; preserve old data/releases; RC then final; separate publication authority | DIST-003/005, STO-004, QT-007 | 9.1-9.4 |
| No Bun/WASM/native runtime addons/Web/Tauri/new installers | TOP-002; non-goals | 7.5, 8.7 |

Unmodified requirements remain binding: RUN-004, CADDY-002/004/005, REG-002/003,
OBS-001..004, TUI-002/004, DOC-003, INSPECT-001..004 and conditional QT-008.
Changing a runtime does not waive current security, provenance or audience rules.
Former IIS/AUTO/OBS-005 additions are withdrawn, not accepted product contracts.

## Gate order and stopping rules

| Gate | Requires | Evidence required to proceed |
| --- | --- | --- |
| G1 | Reviewed corrected plan | Complete contracts/design, released-behavior fixtures and reproducible TS tooling |
| G2 | G1 foundation | Native exclusion/recovery/authentication and owner/elevated/other-account checks, including actual Windows Sandbox |
| G3 | G2 | Real Caddy multi-project transactions, mTLS denial, rollback/reconcile and owned teardown |
| G4 | G3 | Shim fidelity, session cleanup and native operator inspection/PID revalidation |
| G5 | G4 | Real Node application journey, state/log persistence and current routes CLI/TUI parity |
| G6 | G5 | Actual packed npm and every native SEA target, matching functionality/version and verified docs |
| G7 | G6 | Complete coverage, security/system matrix and one-revision requirements ledger |
| G8 | G7 | Node-only checkout/consumers pass again after removing Rust and obsolete packaging |
| G9 | G8 | Matched RC artifacts and transition evidence; final version only after RC acceptance |

Fake adapters prove policy/contract behavior. Native child fixtures prove runtime
mechanics. Actual packed-product tests prove distribution behavior. Interactive
Sandbox proves UAC/account boundaries. None may be relabeled as another.
If a gate lacks execution authority or a native runner, record the gap and do
safe work within that gate; do not bypass dependencies or mark it complete.
Material contract changes return for an explicit user decision.

## Reusable functional catalog

For npm and SEA, run the same versioned scenario data and compare observable
results, exit status, streams and final daemon state. Both must cover:

1. Three entrypoint help/version and correct packaged daemon launch.
2. Bare help on TTY/non-TTY, cadder tui with/without --start-daemon, offline start
   and terminal cleanup, with no implicit TUI on bare invocation.
3. Multi-project managed run, heartbeat, domain conflict/activation and detach.
4. Invalid update, verified reload, last-known-good, post-apply storage failure
   and ambiguous apply reconciled before a later mutation.
5. Shared CLI/TUI state, human-readable output, routes-only TUI and bounded CLI
   diagnostic logs; removed command/option rejection.
6. Trusted Caddy resolution with wrappers/SEA on PATH and no recursion.
7. Owner/elevated access, wrong proofs/replay/spoofed endpoint, unsafe permissions
   and Caddy admin refusal without an authorized certificate.
8. Independent installation runtimes and state/log persistence without restoring
   live leases or importing/deleting old Rust data.
9. Concurrent start/crash recovery, Ctrl+C, bounded stop/restart and no unrelated
   process termination, including guarded operator PID semantics.
10. Node-free SEA workers/children and clean local/global npm consumer installs.

## Evidence format and closeout

Record requirement IDs and task/gate, exact source revision, OS/architecture,
runtime/builder/package version, command or manual scenario, artifact checksum,
actual result and unresolved/skipped cases in verification.md or a linked
implementation artifact. Do not copy private workstation/account details into
shared evidence. Recheck stale evidence after affected source changes.

The final gate requires no unresolved current-scope parity/security/distribution
case, no Cargo runtime/build dependency and no tracked generated JS. OpenSpec
artifact status "done" means the document exists, not that the application works.
No automatic sync, archive, release-version bump or publication occurs while
correcting this plan.

## Source-to-release outcome ledger

| Planned outcome | Category / visibility | Components / impact | Source |
| --- | --- | --- | --- |
| Equal single-package npm/Node and Node-free downloads | Changed / developers and operators | distribution; major installation contract | DIST-001/002/004/005 |
| Explicit isolated-runtime transition without old-data import | Changed / operators | daemon/storage; major IPC/data transition | IPC-002, STO-004, DIST-003 |
| Protected Caddy administration and verified rejected-update recovery | Fixed / developers and operators | Caddy/runtime | CADDY-003/006 |

Current CLI/TUI behavior is retained, not an Added outcome. Internal Rust
replacement is not a separate consumer changelog entry. release.md describes
these planned outcomes; release/version/changelog writes remain deferred to G9.
Current Cargo and legacy npm version sources remain unchanged.
