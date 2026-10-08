## 1. Owned lifecycle adapter

- [x] 1.1 Reuse the existing pinned-image/owned-command boundary for bounded-start, long-lived Caddy without lifetime stream accumulation; preserve command-mode defaults and native cleanup semantics (CADDY-001/002, RUN-003/004). Native Windows Node fixtures cover startup notification, binary streams/footer, full native exit codes, exact/zero/overflow bounds and Job cleanup; see verification.md.
- [x] 1.2 Implement secure start/reload/readback, real idle stop, unexpected-exit diagnostics and close/admission settlement as ConfigurationRuntime, without duplicating the transaction queue (CADDY-003/006, RUN-003). The bounded conflict check refuses an existing authenticated endpoint; explicit queue reconciliation restores committed state only with independent final proof. Full product/native acceptance remains open.

## 2. Verification

- [x] 2.1 Test lifecycle success/failures, start cancellation, bounded forced teardown, immutable policy, uncertain observations and unrelated-process survival with fake adapters/native fixtures (CADDY-002/003/006, RUN-003/004). Include transaction idle rollback/reconciliation regressions. Parent post-review focused suites passed 96 tests; native ownership and HTTPS fixtures remain separate from actual Caddy lifecycle acceptance.
- [x] 2.2 Integrate independent review, simplification/fresh-eyes checks and the full Node gate; record exact executed evidence and retain native/product/archive gaps (CADDY-001/002/003/006, RUN-003/004). F1/F2 corrected and accepted by the targeted independent follow-up; parent final nub run check passed 780 tests with 2 skips and 96.14% own-source line coverage. Planning check remains blocked by implementation-only missing deltas; see verification.md.
