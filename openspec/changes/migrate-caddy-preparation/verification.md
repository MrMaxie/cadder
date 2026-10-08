## Summary

Roadmap 3.1-3.2 implementation and the focused task checks pass. The delivered
capability is trusted configuration selection -> pinned real-Caddy image ->
adapted JSON/hash -> validation. It does not apply configuration, start a server
or expose a new operator command. Released Rust 1.0.5 remains operational.

Base HEAD is `93942af84f555eab2bd68e0a4a9db487ff82d040`; this work and earlier
migration changes remain uncommitted. Existing dirty work was preserved.
The source additions are `src/caddy/{configuration,executable,image,resolver,
adapter,preparation,validation}.ts`, `src/platform/owned-command.ts`, four
`test/caddy-*.test.ts` files and three Caddy fixture files.

This is not archive-ready: the OpenSpec 1.5 strict delta validator rejects both
spec-free implementation changes. G1/G2/G3, native Unix/real-Caddy/channel proof
and final-product Windows Sandbox remain open. Checked tasks record the stated
slice checks, not complete requirement or product acceptance.

## Evidence

| Requirement ID | Task | Evidence | Result |
| --- | --- | --- | --- |
| REG-004, CADDY-001 | 1.1 | `nub run test -- test/caddy-configuration.test.ts`; included in final full check. Strict TOML, unknown/conflicting fields, UTF-8, released OS locations and override/portable/user/system/PATH priority with malformed-priority denial. | Pass for slice; Unix locations modeled |
| CADDY-001, SHIM-003 | 1.2 | `nub run test -- test/caddy-resolution.test.ts`; included in final full check. Real native fixture executables, npm/SEA inventories, wrappers, hardlinks/reparse points, safe/quoted PATH, fully qualified Windows paths, sticky evidence failure and image mutation/replacement. | Pass; built distribution acceptance pending |
| CADDY-005 | 1.3 | `nub run test -- test/caddy-owned-process.test.ts`: 12 pass, one Unix-only skip. Native argv/binary streams, exact bounds, overflow, spawn errors, timeout/abort, exited-parent inherited pipes, Job membership and unrelated survival. | Native Windows pass; Unix proof pending |
| REG-004, CADDY-005 | 2.1 | `nub run test -- test/caddy-preparation.test.ts`; 33 pass at stage handoff and included in final full check. Canonical/raw paths, source cwd, adapter default/metadata, complete JSON/hash, malformed/invalid UTF-8/nonzero output, both exact stream bounds and overflow. | Pass with disposable native fake Caddy |
| CADDY-001, CADDY-005 | 2.2 | Same preparation suite: typed validation-only signature, candidate integrity, real exclusive temp input, abort/timeout/descendant/spawn/pin failures and staging removal. Native external-file-lock rejection returns failure, never validated success. | Pass; persistent external locks remain explicit |
| REG-004, CADDY-001, CADDY-005, SHIM-003 | 3.1 | Final `nub run check`; strict main-spec/roadmap/schema checks; independent MiMo opinions and parent repairs below; whitespace and unstaged review. | Slice checks pass; global gate/archive blocked below |

Final parent Node gate: `nub run check` passes typecheck, ESLint, Prettier and V8
coverage: 26 test files, **352 tests passed, two Unix-native tests skipped**.
Own TS/TSX lines are **896/930 (96.34%)**. Final LCOV inspection confirms the
Caddy/owned-command slice has **435/464 covered lines (93.75%)**, including
adapter **14/14**, preparation **43/43**, validation **20/20** and owned-command
**93/106**. Fully covered adapter/preparation files are omitted from the compact
text table, not the coverage denominator. Embedded C# statements are not measured
by V8; their guarantees have native behavioral tests, not a claimed C# percentage.

Focused parent repair checks:

- `nub run test -- test/caddy-resolution.test.ts -t 'caller cancellation|resolver close|quoted absolute'`: three pass.
- `nub run test -- test/caddy-owned-process.test.ts`: 12 pass, one skip.
- `nub run test -- test/caddy-preparation.test.ts -t 'staging cleanup|orphan|descendant'`: six pass.
- `nub run test -- test/caddy-resolution.test.ts -t 'qualification|root-relative|caller cancellation|resolver close|quoted absolute|Scoop'`: 12 pass.

An initial new cancellation regression failed because the asynchronous Node
preload fixture still let Node interpret `version` as a script. The test-only
fixture now uses the existing asynchronous fixture's Module.runMain override.
Concurrent test promises are observed on failure. The corrected regression and
full gate above pass; the failed attempt is not counted as passing evidence.

## Independent review and repairs

MiMo Flash verified final-product Sandbox sequencing, retained Caddy/configuration
requirements and native cleanup constraints. The Windows taskkill orphan/pipe
reproducer confirmed its main concern. The approved narrow PowerShell/.NET Job
adapter creates the native child suspended and already assigned to its Job,
performs pre/post image verification before execution, preserves native argv and
binary streams, and kills only that Job. No addon, distributed launcher, generic
broker, dependency, tooling-policy or host privilege change was introduced.

MiMo Pro reviewed actual source/tests independently: PASS WITH NOTES, including
one P1. That verdict was not treated as unconditional acceptance. Parent fixes:

- P1 cancellation poisoning: shared probes now belong to a resolver-owned abort
  lifetime. A cancelled caller stops its wait without cancelling another caller
  or caching an abort as image evidence. Close cancels the owned pending probe.
  New native regressions cover mid-probe cancellation, concurrent/later callers
  and resolver shutdown; genuine failed version/modules remain sticky.
- P2 quoted PATH: Windows parsing preserves quoted absolute entries, including
  semicolons, without accepting relative entries. A native regression passes.
- P2 post-exit pipes: leader exit starts bounded stream settlement; lingering
  pipes fail with a stream diagnostic rather than the operation timeout. The
  escaped-Unix fixture is added but skipped on this Windows host; reclaiming an
  escaped group is not claimed. A subsequent primary failure clears the existing
  settle timer before replacing it, preserving the primary error without a
  leftover timer.
- P2 owned teardown/staging: the Windows helper checks the Job's active process
  count reaches zero within four seconds before reporting success. Staging uses
  three built-in removal retries with 100 ms linear backoff; external-lock
  failure remains bounded and never produces validated success.
- P2 scope: RUN-003 is removed from slice completion tags. Daemon/server shutdown
  remains roadmap 3.8. Validation's empty success diagnostics are not advertised
  as warning-diagnostics support.
- Fresh-eyes parity repair: Windows root-relative paths are not fully qualified
  absolute paths. Configuration, image and source-path checks now match that
  distinction; native and modeled regressions pass.

Parent fresh-eyes/simplification retained the single existing process helper and
made no new framework. Active LSP probes were not uniformly conclusive; auxiliary
lint rules also flagged intentional parser throws and stylistic constructs.
Malformed module/OS metadata parsing propagates through the owning async failure
path rather than yielding success. These are not an LSP-clean claim. Final
TypeScript compiler and repository lint checks pass.

## Repository and archive gate

- `openspec schema validate implementation`: pass.
- `openspec validate --specs --strict --no-interactive`: 15 pass.
- `openspec validate reset-cadder-architecture --strict --no-interactive`: pass.
- `git diff --check`: pass. `git diff --cached --name-only`: empty.
- `mise run check`: Rust formatting, workspace/all-target Clippy with `-D warnings`
  and all **300 Rust tests pass**, then the existing OpenSpec 1.5 implementation
  schema limitation blocks the combined gate: 17 items pass, two implementation
  changes fail. Docs/npm/cargo-dist steps after that failure were not reached.
- `openspec validate migrate-caddy-preparation --type change --strict --no-interactive`:
  fails with `Change must have at least one delta. No deltas found.` The accepted
  custom schema intentionally implements existing IDs without delta specs. No
  fake deltas, checker replacement or tool-policy workaround was added.

- [x] Every focused implementation task and its stated slice checks is complete.
- [x] In-scope requirement aspects have the passing evidence above.
- [x] Focused and full Node checks pass above the 85% line threshold.
- [x] Strict main-spec, roadmap and implementation schema checks pass.
- [x] Documentation distinguishes implementation from native/product acceptance.
- [ ] Global `mise run check` / strict implementation validation passes.
- [ ] Archive is authorized and validation-compatible.

## Residual risk

Native Linux/macOS image/group execution, real installed Caddy, packed npm/SEA,
other Windows/filesystem matrices and final-product Sandbox remain unverified.
Normal Windows owner tests do not establish owner-to-elevated/other-account
acceptance. Main CADDY-005 and RUN-003 are not closed as whole requirements.

Unix detached groups cannot reclaim descendants that deliberately leave the
group; the added stream-settle regression does not grant process enumeration or
prove escape cleanup. Image verification is check-and-detect, not atomic
pathname-exec immunity; Windows anchors detect mutation rather than reproducing
Rust share-mode prevention. PowerShell/Add-Type startup costs remain within each
command deadline. Persistent external locks can leave owned staging residue,
reported as a cleanup failure without terminating unrelated holders.

Child direct artifact writes were denied with `Blocked write (Arcantry project
boundary): user confirmation required but no UI available.` Complete reports
were delivered through native runtime persistence; no boundary bypass or alternate
CLI execution was used. This is an orchestration limitation, not a passing test.

Composition, apply/rollback/queue, mTLS/Admin API, server lifecycle, operator
surfaces, distribution and release remain the approved later roadmap. No commit,
staging, publication, Sandbox, UAC/account action or existing Rust product cutover
was performed in this slice.
