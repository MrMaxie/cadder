# Product implementation evidence

This slice delivers the released Caddyfile -> shim -> daemon -> operator journey
against accepted main requirements. Parent G1-G7 and Rust removal remain open.
The complete dated commands, candidate identities and failed/skipped checks are
recorded in `../reset-cadder-architecture/verification.md` under "Node product
implementation checkpoint (2026-10-20)". This slice is not archive-ready while
the schema-blind OpenSpec gate and final native acceptance remain unresolved.

| Tasks | Requirements | Executed evidence |
| --- | --- | --- |
| 1.1 | STO-001..004 | Actual SQLite schema creation/refusal, integrity, contention rollback, restart and protected worker tests; packed npm/SEA worker runs |
| 1.2 | OBS-001..004 | SQL row/retention/order assertions, before-write credential redaction, native line ingestion/attribution, overflow and terminal sanitization |
| 2.1 | REG-001..004, CADDY-003 | Existing real transaction queue with handler conflicts, session ownership, toggles, failed persistence, expiry and intent-only restart |
| 2.2 | CADDY-006, IPC-003, RUN-001..003 | Protected product daemon integration, initializer/readiness/teardown ordering and retained exclusion on failed ownership cleanup; native product evidence is qualified in the parent checkpoint |
| 3.1 | CLI-001, IPC-003, TUI-001..004, TOP-002 | Released argv catalog through actual Commander; typed shared client and Ink keyboard behavior; real Windows bare-help/TUI start/stop/quit |
| 3.2 | REG-001/003, SHIM-001..003 | Actual policy/registration/session tests; compiled multi-project managed shim in Windows Sandbox; complete native delegation fidelity remains parent G4 |
| 3.3 | INSPECT-001..004 | CLI boundaries and real host TCP ownership with changed-PID refusal; Linux/macOS branches remain unexecuted locally |
| 4.1 | DIST-004, QT-003/006 | Actual tsc/npm pack allowlist, local lifecycle and three local/global npm wrappers on Node 24.18.0 |
| 4.2 | DIST-001/002/005 | Pinned Node 26.12.0 build, worker asset, licenses and archive inventory/checksums; extracted Windows and supplementary WSL Linux native lifecycle without product Node PATH |
| 4.3 | DOC-001..004 | Exact Nub docs lock/tasks, explicit unpublished candidate instructions, Astro check and 14-page build |
| 5.1 | QT-003, STO-001, REG-001, IPC-003 | Fresh-eyes repair/review, removed redundant mock/declaration tests, 830 passing cases/two skips, 88.74% own-line coverage; failed aggregate and native checks remain recorded |

No acceptance is inferred from artifact existence or CI preparation. The parent
roadmap retains native TUI/error/cancellation, full channel parity, final account
boundaries, clean-revision acceptance, Rust cutover and release preparation.
