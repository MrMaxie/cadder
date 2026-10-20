## 1. Durable application state

- [x] 1.1 Implement bounded worker-owned SQLite state/log storage, exact schema/integrity validation and safe close (STO-001..004; verify real database restart, refusal and worker integration).
- [x] 1.2 Redact before insertion, enforce deterministic retention and ordered canonical-stream queries; keep terminal controls inert (OBS-001..004; verify actual retained/query rows and output).

## 2. Product integration

- [x] 2.1 Implement current registration/heartbeat/expiry and activation through the existing transaction queue, retaining durable intent without live lease restoration (REG-001..004, CADDY-003; verify handler journeys, conflicts, stale sessions and restart).
- [x] 2.2 Join protected storage, secure Caddy and IPC in the product daemon with owned shutdown (CADDY-006, IPC-003, RUN-001..003; verify compiled product start/contact/shutdown and failure cleanup).

## 3. Current operator and shim journey

- [x] 3.1 Implement shared lifecycle/state client, released Commander grammar and routes Ink controls (IPC-003, CLI-001, TUI-001..004; execute the frozen argv catalog and keyboard interaction tests, plus Windows native terminal smoke).
- [x] 3.2 Implement managed nonce/heartbeat/reconnect/cleanup and trusted delegation (REG-001/003, SHIM-001..003; execute policy/session tests and actual multi-project shim registration in Sandbox).
- [x] 3.3 Implement platform port inspection and expected-PID revalidation (INSPECT-001..004; verify CLI boundaries and actual Windows socket ownership/refusal). Linux/macOS branches require their parent native gates.

## 4. Non-publishing distributions and documentation

- [x] 4.1 Pack compiled npm with a consumer-only manifest and three bins (DIST-004, QT-003/006; verify actual local lifecycle and local/global wrappers without a TS loader).
- [x] 4.2 Pin the independently verified Node 26 builder and build SEA entries with the embedded worker, source identity, licenses and checksums (DIST-001/002/005; verify extracted Windows archive without Node/npm on product PATH). The remaining native variants stay in parent G6.
- [x] 4.3 Move docs dependencies/tasks to exact Nub locks and document the unpublished candidate's equal channels and transition (DOC-001..004; run Astro check and build).

## 5. Verification and simplification

- [x] 5.1 Replace declaration/self-mock tests with behavior evidence, review changed boundaries, run the final Node check and record exact results and remaining native gates (STO-001, REG-001, IPC-003, QT-003). Keep failed overall validation and unexecuted acceptance open; do not archive or remove Rust.
