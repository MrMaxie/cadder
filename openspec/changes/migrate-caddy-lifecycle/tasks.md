## 1. Owned lifecycle adapter

- [x] 1.1 Reuse the existing pinned-image/owned-command boundary for bounded-start, long-lived Caddy without lifetime stream accumulation; preserve command-mode defaults and native cleanup semantics (CADDY-001/002, RUN-003/004). Native Windows Node fixtures cover startup notification, binary streams/footer, full native exit codes, exact/zero/overflow bounds and Job cleanup; see verification.md.
- [x] 1.2 Implement secure start/reload/readback, real idle stop, unexpected-exit diagnostics and close/admission settlement as ConfigurationRuntime, without duplicating the transaction queue (CADDY-003/006, RUN-003). The bounded conflict check refuses an existing authenticated endpoint; explicit queue reconciliation restores committed state only with independent final proof. Full product/native acceptance remains open.

## 2. Verification

- [x] 2.1 Test lifecycle success/failures, start cancellation, bounded forced teardown, immutable policy, uncertain observations and unrelated-process survival with fake adapters/native fixtures (CADDY-002/003/006, RUN-003/004). Include transaction idle rollback/reconciliation regressions. Parent post-review focused suites passed 96 tests; native ownership and HTTPS fixtures remain separate from actual Caddy lifecycle acceptance.
- [x] 2.2 Integrate independent review, simplification/fresh-eyes checks and the full Node gate; record exact executed evidence and retain native/product/archive gaps (CADDY-001/002/003/006, RUN-003/004). F1/F2 corrected and accepted by the targeted independent follow-up; the initial parent gate passed 780 tests with 2 skips and 96.14% own-source line coverage. Planning check remains blocked by implementation-only missing deltas; see verification.md.

## 3. Windows native creation-owner correction

- [x] 3.1 Preserve the actual user as default owner of future native-created files before SQLite admission; verify the Node primary token, request adjustment only on mismatch, require independent readback, and refuse unsafe existing journals first (RUN-005, CADDY-006). Keep Unix and runtimeOwner observation unchanged; no privilege or existing ACL changes.
- [x] 3.2 Exercise the production correction with real SQLite and Caddy 2.11.4 in an isolated Windows Sandbox: generated journal/key protection, HTTP/HTTPS multi-project routing, reload, malformed rejection with last-known-good retention, idle stop and material-reused restart (RUN-005, CADDY-002/003/006). Guest identity was elevated; this is leaf integration, not completed-product privilege or distribution acceptance.
- [x] 3.3 Add mocked fail-closed/admission-order regressions, independent static review and the final Node gate: 812 passed, 2 existing Unix-native skips, 96.16% own-source lines (RUN-005, CADDY-006). Native C# success paths are exercised separately by the Sandbox scenario, not by TypeScript coverage.
