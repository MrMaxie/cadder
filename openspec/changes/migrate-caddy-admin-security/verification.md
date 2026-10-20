# Verification

## Approved issuance split

The user approved correcting roadmap 3.6 and the reset design: Node
WebCrypto/X.509 creates protected root/intermediate CA and client material;
Caddy's standard internal issuer obtains and renews the admin server leaf.
CADDY-006 and final acceptance requirements are unchanged.

Pinned Caddy 2.11.4 source inspection and independent MiMo Flash opinion support
this standard configuration path. Current official documentation was indexed
through grounded-docs without labelling it an exact-version snapshot.

## Native feasibility status

The initial delegated attempt stopped before execution. Its external probe
script write required Arcantry confirmation unavailable in the headless child.
No bypass was attempted. No Caddy process, certificates or listener were created;
only public source references remained in disposable temporary storage.

Parent independently checked the native executable version (2.11.4), SHA-256
and relevant module presence. This prerequisite inspection is not startup,
permission, TLS denial, reload or teardown proof. Roadmap 3.6–3.7 remain open.

## Protected material increment

Delivered src/caddy/certificates.ts and test/caddy-certificates.test.ts.
loadOrCreateAdminMaterial(directory, owner) creates or reuses six fixed protected
root/intermediate/client PEM files, validates complete profiles, signatures,
validity and key matches, and returns internal TLS credentials plus client leaf
DER for Caddy access control. The caller retains runtime-lock ownership.

Parent checks after review fixes:

- Focused suite: 70 tests passed, including actual Windows owned-temp creation,
  reuse and owner-only ACL checks for all six files. Native Linux/macOS execution
  and generated Caddy server-material protection remain unproved.
- nub run check: typecheck, ESLint, Prettier and coverage passed; 29 test files,
  550 passed and 2 existing Unix-native tests skipped.
- Own-source lines: 1,283/1,319 (97.27%). Certificates module: 128/128 lines,
  92/92 branches and 8/8 functions (100%). Coverage output stayed in runner temp.
- Active LSP probe had no TypeScript errors; six auxiliary style findings and
  push-only clean-check uncertainty remain. This is not a universally clean
  diagnostics claim.
- Strict main specs: 15 passed; reset roadmap and implementation schema valid;
  git diff --check passed and nothing was staged.
- mise run openspec-check: 17 passed / 5 failed. Existing implementation-schema
  missing-delta failures now include this focused slice; no duplicate requirements
  or validation-tool changes were added to manufacture archive acceptance.

MiMo Pro's initial material review found no P0/P1 and three P2 notes. Parent
required critical client EKU to match the generated fixed profile, added explicit
PEM-header and available-O_NOFOLLOW regression assertions, and clarified that
I/O/cleanup failure can leave partial files. The shared helper's AggregateError
has no cleanup-phase discriminator: the code does not guess a precise cleanup
classification or claim ownership of its residual path. MiMo Pro's targeted
follow-up accepted all three dispositions with no new defect in their blast
radius. Task 1.2 is complete within its tested increment. Roadmap 3.6/3.7 and
complete-slice acceptance remain open; these checks prove the CA/client increment,
not Caddy mTLS or server renewal.

## Secure policy increment

Delivered src/caddy/admin-policy.ts and test/caddy-admin-policy.test.ts.
prepareSecureAdminPolicy assembles the guarded route plan with explicit native
loopback mTLS administration, imported admin CA paths and internal web issuance.
It prepares protected runtime directories and confines child data/config/home/temp
environment paths. Wildcard subjects retain the existing composition contract.
normalizeCaddyConfig deterministically orders object keys while preserving arrays
and opaque JSON data, with complete-body, depth and traversal bounds.

- Worker focused checks: 91 policy tests; 224 combined policy/material/composition
  tests, typecheck, scoped lint and formatting passed.
- Parent nub run check after integration: typecheck, ESLint, Prettier and coverage
  passed; 30 files, 641 passed / 2 existing Unix-native skips.
- Own-source lines: 1,421/1,457 (97.52%). Admin-policy module: 100% lines/functions
  and 99.23% branches. Outputs remained in runner temporary storage.
- Actual owned-temp directory/material preparation and reuse passed on Windows;
  Linux/macOS policy checks are modeled, not native acceptance.

MiMo Pro accepted the policy with one test-gap note. Parent added a regression
for non-empty TLS subjects with fully pruned project routes, without requiring
subjects to equal the outer-guard union. The optional permission-error kind note
is deliberately unchanged: failure remains bounded and fail-closed, with no new
classification requirement accepted. Task 2.1 is complete as an assembly increment.
Real Caddy startup/readback/denial, generated descendant-file protection and server
renewal remain open. No Caddy process was launched by these tests; the parent
roadmap is not marked complete merely because assembly tests pass.

## HTTPS client increment

Delivered src/caddy/admin-client.ts and test/caddy-admin-client.test.ts.
CaddyAdminClient targets only the prepared loopback endpoint with explicit CA,
authorized client credentials and normal localhost certificate verification.
It refuses noncanonical candidates or changes to the pinned admin/PKI envelope,
bounds headers and complete UTF-8 response bodies, and aborts owned requests at an
absolute deadline. Completed HTTP 400 load rejection is distinct from uncertain
transport or other-status failures. GET /config/ independently normalizes observed
configuration; unavailable readback never becomes idle or a cached receipt.
Owned-process idle/lifecycle integration remains roadmap 3.8.

- Worker: 60 focused client tests and 216 neighboring regressions passed;
  typecheck, scoped lint and formatting passed.
- Parent nub run check: typecheck, ESLint, Prettier and coverage passed; 31 files,
  701 tests passed / 2 existing Unix-native skips. Own lines: 1,511/1,551 (97.42%).
  Client coverage: 95.74% lines, 100% functions and 94.59% branches.
- Actual disposable Node HTTPS fixtures exercised CA/client/server-name/expiry
  denial, complete independent readback, bounds, deadlines, disconnects and queue
  rollback/fencing. They are not evidence of native Caddy authorization or renewal.
- git diff --check passed; nothing staged. Active LSP probes reported auxiliary
  heuristic findings, not TypeScript compiler errors, and silent-on-clean coverage
  remained inconclusive. JSON.parse findings in both leaf helpers have catch
  boundaries at their callers; no blanket clean-diagnostics claim is made.
- The worker's direct external report write was refused by Arcantry. It stopped
  that attempt and returned its final report for workflow-managed persistence;
  no alternate write mechanism or permission override was used.

MiMo Flash accepted the client with one explicitness note: idle/null refusal
previously depended on catching property access failure. Parent added an explicit
pre-network null guard and a dedicated no-request regression, preserving the
existing rejection outcome. No lifecycle stub or queue contract change was added.

Final checks after both local review corrections:

- Focused policy/client suites: 153 passed (92 policy, 61 client).
- nub run check: typecheck, ESLint, Prettier and coverage passed; 31 files,
  703 tests passed / 2 existing Unix-native skips; 1,511/1,551 own lines (97.42%).
  Client branches: 94.73%; other reported module percentages remain unchanged.
- Fresh-eyes review checked the null guard's local blast radius and the composed
  pruned-route fixture. Simplification review retained the single transport and
  reused normalization boundary rather than introducing another validator/queue.
- The two auxiliary unchecked-JSON.parse findings were marked false positives:
  private throwing validators are called inside their owning catch boundaries.
  No inline suppression or broader diagnostics-cleanliness claim was added.
- git diff --check passed; nothing staged.
- Strict main specifications: 15 passed; reset-cadder-architecture valid;
  OpenSpec doctor passed. mise run openspec-check still reported 17 passed /
  5 failed. Focused validation confirmed the unchanged missing-delta requirement
  for this implementation-only change; no duplicate requirements were added.

Tasks 2.1/2.2 are complete within their assembly/client scope. Native task 2.3,
complete-slice acceptance, roadmap 3.6/3.7, generated server-key protection and
final-product Sandbox remain open. No real Caddy process was launched by this
increment. The user subsequently authorized temporary probes in isolated private
repository-local staging and allows Sandbox testing now. The private directory is
excluded locally; the remote default branch does not track it, and existing test,
typecheck and formatting input globs do not consume it. No shared tool or product
may acquire a dependency on that staging. At that increment native execution and Sandbox results were still pending;
partial-product tests do not close final privilege/G7 acceptance.

## Ordinary-owner correction and native administration evidence

The user authorized isolated private probes and promotion of this sanitized
summary. No private paths, account identifiers, credential bytes or private
staging dependencies are included in shared sources.

The Windows protection helper previously requested owner reassignment even when
an exclusively new path already had the expected owner. An ordinary-token
reproducer distinguished that write from DACL-only protection. protectCreated
now reads the actual owner and requests reassignment only when it differs;
assertProtected remains strict. Five native ordinary-token regressions cover
new directories/files under Modify-only inheritance, unsafe existing paths,
owner-read failure cleanup and refusal to assign a new file to another account.
MiMo Pro accepted the fix and its completed differing-owner coverage.

Parent nub run check passed after that correction: 32 test files, 708 passed,
2 existing Unix-native skips, 1,511/1,551 own lines (97.42%), plus typecheck,
ESLint and Prettier. This predates the newly approved descendant-validation
increment and is not its final gate.

Pinned stock Caddy 2.11.4 ran once on an ordinary Windows token using existing
product material/policy/client/image/owned-command boundaries. Unmodified full
factory configuration passed native validation without web listener binding.
The running admin-only candidate retained the exact admin/PKI policy, with
plaintext disabled, explicit loopback administration and no trust installation.
Authorized GET, complete POST/load and independent actual-config hash readback
passed. Missing clients, a distinct client issued by the same intermediate and
an incorrect server root were refused. Online unknown-SNI denial was EPROTO;
a separate stock offline name check against the normally verified peer leaf
returned ERR_TLS_CERT_ALTNAME_INVALID. This is not a mismatched-leaf network
receipt. Caddy issued its server leaf from the imported product intermediate.
Authorized stop returned 200; the owned child exited 0, streams settled and the
exclusively acquired private runtime was removed. MiMo Pro verified these
phase claims against the isolated evidence.

The prior blanket strict audit failed honestly: 39 paths had the expected owner
and no foreign DACL grants or links, but 17 inherited owner-only DACLs lacked an
explicit protection bit. Fifteen belonged to native Caddy storage; two were a
probe-created intermediate directory and a SQLite journal, not Caddy files.
Product root and six PEM controls passed strict checks. An earlier elevated
Sandbox run also contained differently owned generated files; those remain
unsafe and are not reclassified by the ordinary-user result.

The user approved the simpler acceptance boundary in design.md: strict Cadder
roots/credentials/databases; read-only owner/access/link validation for native
Caddy storage and SQLite journals beneath those protected roots, allowing safe
Windows inheritance without repairing ACLs. Implementation and a new native
proof were pending at that decision; their subsequent results follow below.
Earlier failed evidence remains retained rather than rewritten as a pass.

## Inherited native storage increment

Delivered assertRuntimeDescendant in src/platform/runtime-security.ts and known
DELETE-journal checks in src/daemon/runtime-lock.ts, with independent automated
coverage in test/runtime-descendants.test.ts and test/runtime-lock-faults.test.ts.
The validator inspects only the requested descendant and its in-root directory
chain beneath a strictly verified root. It refuses lexical escapes, links,
wrong kinds/owners and unsafe access without changing permissions. Cadder-created
roots, PEMs and primary databases retain strict validation. Unix UID and mode
requirements are unchanged.

The journal is checked before SQLite opens the database and again while runtime
exclusion is held, before metadata publication. Only absence of that exact
journal leaf is tolerated; missing roots/ancestors and inspection failures remain
fatal. Unsafe existing journals are neither consumed nor removed. There is no
new scan, monitor, repair pass or storage framework.

A fixture-only ACL mistake initially prevented cleanup of a task-created test
directory. After explicit approval, one DACL-only restoration on that exact
owned fixture permitted cleanup; the fixture inheritance was corrected. No
production, parent-directory or host-policy repair was performed.

### Current native evidence

A new ordinary-token Windows run used pinned stock Caddy 2.11.4 and the existing
product material, policy, HTTPS client and owned-command boundaries. The full
unmodified policy passed native validation; actual execution remained admin-only,
not a full web-runtime acceptance test. Authorized GET, complete POST/load and
independent normalized-config readback passed. Missing clients, a different
client from the same intermediate, a wrong server root and unknown SNI were
refused. Unknown-SNI EPROTO and the separate stock offline wrong-name check of
the normally verified peer leaf retain their distinct scope.

Caddy issued the localhost server leaf from the product intermediate. A finite
probe audit found 39 paths with the expected owner, no foreign access grants and
no links: 18 strict product paths, 5 strict probe-scaffold paths and 16 safely
inherited native paths (15 Caddy paths and one SQLite journal). Product validation
failed on none. The generated server key passed descendant validation while
legacy assertProtected still refused its inherited DACL, confirming that strict
validation was not relaxed. No ACLs were repaired.

Authorized stop returned 200; the owned child exited 0, its Job and streams
settled, and the exclusively acquired runtime was removed after ownership
revalidation. No trust installation, host elevation or Sandbox run was needed.
The earlier elevated generated-owner mismatch remains unsafe and unresolved.

### Review and final checks

MiMo Pro verified source, tests and the named native evidence independently and
returned OK with notes, with no blockers. Simplification/fresh-eyes review kept
one read-only validator and one known-journal wrapper. Two minor observations
remain explicit: the real-journal test pins the current SQLite creation timing;
the pre-existing strict check's treatment of InheritOnly FullControl was outside
this approved delta. Product-created strict ACLs use propagation-None rules.
Neither observation was converted into an unsolicited hardening task.

Parent nub run check passed after the final source changes:

- TypeScript typecheck, ESLint and Prettier passed.
- 33 test files passed; 727 tests passed and 2 existing Unix-native tests skipped.
- Own-source lines: 1,535/1,575 (97.46%); branches: 1,104/1,207 (91.46%).
- Runtime security and runtime lock: 100% line/function coverage. Coverage
  artifacts remained in runner temporary storage.
- git diff --check passed; nothing was staged. Active diagnostics retained
  auxiliary heuristic findings and push-only silent-check uncertainty; the
  passing TypeScript compiler is not a universal LSP-cleanliness claim.
- OpenSpec doctor and all 15 main specifications passed; the maintained planning
  check reported 17 passed / 5 failed. Focused strict validation again refused
  this implementation-only change because it has no delta. That archive blocker
  remains open; no duplicate requirements or validator changes were introduced.

Tasks 2.3/3.1 are complete within this approved ordinary-Windows/admin-only
increment, including recording the blocked planning gate. This does not close
roadmap 3.6/3.7, complete migration or archive acceptance. Full web execution,
owned lifecycle 3.8, renewal/restart persistence, native Linux/macOS, elevated
owner handling, final completed-product privilege/Sandbox/G7 and distribution
acceptance remain open. Shared sources depend only on product code and ordinary
test fixtures, not on private probe scripts or results.
