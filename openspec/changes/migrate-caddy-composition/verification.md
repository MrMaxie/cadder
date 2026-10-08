## Summary

Roadmap 3.3 and the focused implementation checks pass. Existing adapted project
JSON and registration/domain activation now produce a deterministic guarded route
plan, or bounded diagnostics with no usable plan. The new modules are
src/caddy/domains.ts and src/caddy/composition.ts; tests are in
test/caddy-composition.test.ts. The plan is not a complete runnable CaddyConfig.
No validation/apply wiring, Admin API, server or runtime-state mutation is added.

Evidence applies to the uncommitted checkout based on HEAD
93942af84f555eab2bd68e0a4a9db487ff82d040. Earlier dirty work is preserved.
No staging, commit, publication, dependency, Rust production, Sandbox or host
privilege/account changes were performed by this slice.

This is not archive-ready. The known OpenSpec 1.5 delta validator rejects the
three spec-free implementation changes. G1/G2/G3, real-Caddy/native/distribution
proof and final-product Sandbox remain open; checked tasks do not close whole
requirements or product acceptance.

## Evidence

| Requirement ID | Task | Evidence | Result |
| --- | --- | --- | --- |
| REG-002, REG-004 | 1.1 | `nub run test -- test/caddy-composition.test.ts`: canonical case/IDN/trailing dots, numeric literal and URL-delimiter differences, all registration/domain activation states, duplicate registration IDs, canonical self-deduplication, sorted competing-owner diagnostics and bounded paths. | Pass for composition boundary; exhaustive IDNA parity not claimed |
| REG-004 | 1.1 | Same suite: complete/hash/UTF-8/32 MiB carrier checks, malformed route shapes, sorted server extraction, first real route-position upstream and opaque payload rejection as metadata. | Pass; no Caddyfile parser or adapter execution added |
| CADDY-002, CADDY-004, REG-004 | 2.1 | Same suite: multi-project ordering, registration host guards, mixed/disabled-only matcher pruning, nested and separate hostless siblings, exact IPv4/IPv6 loopback listeners on 80/443, HTTP-only ID copies, immutable inputs and root admin/listener/TLS injection ignored. | Pass for pure route plan; server/apply acceptance pending |
| CADDY-002, CADDY-004, REG-004 | 2.1 | Terminal-parent regression retains chain position and matchers with an empty handler list after disabled subroutes prune; opaque proxy-shaped payload regression preserves JSON and yields no fake upstream. | Pass after parent repair; actual Caddy chain execution remains 3.9 |
| CADDY-002, CADDY-004, REG-002, REG-004 | 3.1 | Independent MiMo Flash opinion, MiMo Pro source review and targeted repair re-review; parent full Node/schema/main/roadmap checks, simplification/fresh-eyes pass and diff inspection below. | Slice checks pass; global/archive gates blocked |

Final parent `nub run check` passes TypeScript, ESLint, Prettier and V8 coverage:
**415 tests pass, two existing Unix-native tests skip, 27 test files pass**.
Own TS/TSX lines are **1063/1098 (96.81%)**. Coverage-summary inspection includes
composition **62/62** and domains **105/106**, together **167/168 (99.40%)**;
composition's fully covered file is omitted from the compact text table, not the
coverage denominator. The uncovered domain line is the comparator's equal return.

Final focused suite: `nub run test -- test/caddy-composition.test.ts`, **63 pass**.
An earlier focused run with `-t 'terminal chain|opaque non-route'` deliberately
reproduced the terminal regression: one failed, one passed. The repaired focused
and final full checks above pass; that initial failure is not passing evidence.

## Review and repairs

MiMo Flash identified plan-level risks around emptied host matchers, opaque host
properties, canonical equality and route-plan/full-config separation. The final
source and fixtures preserve those boundaries. A matcher emptied by filtering
is removed as an impossible OR alternative, never converted to a catch-all;
hostless siblings remain inside the registration's outer active-host guard.
Only actual route matcher and subroute positions contribute hosts/upstreams.

MiMo Pro initially returned OK with notes, no P0/P1, and two P2 findings:

- Terminal chain position: deleting a parent after all subroute handlers prune
  could expose the following route. The parent reproduced this, then retained
  terminal parents with their matchers, terminal flag and empty handle array.
  Nonterminal empty parents and disabled-only host matcher routes still prune.
  Regression fixtures cover active-host and hostless terminal parents.
- Opaque upstream metadata: narrower traversal than Rust's broad JSON walk is
  intentional. A new fixture pins that a proxy-shaped plugin payload does not
  contribute a domain/upstream and remains unchanged in the composed routes.

The same MiMo Pro reviewer inspected those repairs and returned **OK**, both P2
items resolved and no new defect in their blast radius. The follow-up's earlier
pending-full-gate caveat is superseded by the final passing parent gate above.
Parent simplification replaced a nested comparator ternary with explicit returns;
fresh-eyes inspection found no further substantive issue.

The original Flash run failed with an upstream inference-generation error;
the original Pro run timed out without a verdict. Exact-run native resumes
produced the completed opinion, review and targeted follow-up. Failed receipts
remain failures, not successful reviews. A child direct report write was boundary
blocked; native runtime capture persisted the final response without a bypass or
alternate CLI. No orchestration failure is counted as product evidence.

## Gate

- [x] Every focused task and its stated slice checks is complete.
- [x] Every in-scope requirement aspect has evidence above.
- [x] Focused and full Node checks pass above 85% own-code lines.
- [x] `openspec schema validate implementation` passes.
- [x] `openspec validate --specs --strict --no-interactive`: 15 pass.
- [x] `openspec validate reset-cadder-architecture --strict --no-interactive` passes.
- [x] `git diff --check` passes; staged content is empty; no generated JS/DLL/EXE appears in the affected source/test tree.
- [ ] Global `mise run openspec-check` passes: currently 17 items pass and three spec-free implementation changes fail the missing-delta validator.
- [ ] Strict focused change validation passes: currently fails with `Change must have at least one delta. No deltas found.`
- [ ] Archive is authorized and validation-compatible.

No fake deltas, checker replacement or validation-policy workaround was added.
Active LSP probes did not uniformly certify clean results; auxiliary rules retain
non-null assertion/filter-map warnings and runtime-type-check hints. Maintained
TypeScript and lint checks pass; this is not an LSP-clean claim. OpenSpec artifact
instructions also emitted the known custom-schema `specs` rule warning while
returning verification instructions; it was not treated as product failure or
silently repaired. The combined Rust/docs/npm/cargo-dist gate was not rerun for
this Node-only slice; the earlier preparation evidence remains separately dated.

## Residual risk

Pure JSON assertions do not prove execution by real Caddy, empty-handler terminal
semantics or every adapter output shape. Native Caddy, protected mTLS assembly,
queue/apply/rollback, lifecycle, distribution and final-product Sandbox remain
later roadmap work. Canonicalization fixtures cover observed Node/Rust IDNA
edges, not exhaustive Unicode/version parity. Pending/committed-state isolation
and preservation of the currently committed owner during a runtime mutation
belong to 3.4-3.5; this slice only rejects the complete conflicting route plan.
