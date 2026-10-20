# Verification

## Scope and source

The initial leaf owned-Caddy lifecycle increment was verified on the migration
branch at base commit `93942af84f555eab2bd68e0a4a9db487ff82d040` with then-uncommitted
source changes. It was subsequently committed as `31b88e1` and pushed. The Windows
creation-owner follow-up below was verified against that commit plus the exact
hashed production delta. Neither result accepts a tagged release candidate,
whole roadmap 3.8, G3, final product or either distribution. Existing Rust remains.

Delivered `src/caddy/runtime.ts`, extensions to the existing administration and
owned-command boundaries, and direct regression tests. A small correction to
`ConfigurationTransactions.reconcile` connects explicit recovery to this backend;
there is no new queue, launcher, process registry, automatic restart or RPC.

## Requirement and task evidence

| Requirement / task | Executed evidence | Boundary |
| --- | --- | --- |
| CADDY-001, RUN-004 / 1.1 | Existing resolver/image regression suite; immutable environment/candidate tests; verified native-start notification | PinnedCaddyImage is reused unchanged; no distribution recursion acceptance inferred |
| CADDY-002, RUN-003 / 1.1, 1.2, 2.1 | Native Windows Node argv/binary stream/exit/footer, immediate limits, cancellation/orphan cleanup and unrelated-child survival; lifecycle graceful/forced stop and close tests | Native process ownership and fake lifecycle tests are separate, not real-Caddy acceptance |
| CADDY-003 / 1.2, 2.1 | Independent observed hashes; ambiguity/rejection; actual idle rollback through ConfigurationTransactions; explicit crash recovery and failed-cleanup fencing | Application storage remains a fake atomic port; no handler or registration recovery claim |
| CADDY-006 / 1.2, 2.1 | Existing actual Node HTTPS TLS tests plus new bounded stop/cancellation; security-envelope refusal and startup conflict regressions | No plaintext fallback; prior native administration proof is not full lifecycle proof |
| All in-scope IDs / 2.2 | Independent review and accepted F1/F2 corrections; parent focused/full gates; fresh-eyes/simplification and planning checks | Failed planning and unexecuted native/product gates remain explicit |

## Checks

Worker final focused command:

```sh
nub run test -- test/caddy-runtime.test.ts test/caddy-owned-process.test.ts test/caddy-admin-client.test.ts test/caddy-preparation.test.ts test/caddy-resolution.test.ts test/configuration-transactions.test.ts
```

It passed 271 tests with 2 existing Unix-native skips. This result predates the
parent's F1/F2 corrections and is not the final increment gate.

Parent post-review correction checks:

```sh
nub run test -- test/caddy-runtime.test.ts test/configuration-transactions.test.ts
nub run typecheck
nub run check
```

- Focused lifecycle/queue: 96 passed in two files; typecheck passed.
- Final Node gate: TypeScript, ESLint, Prettier and V8 coverage passed; 34 files,
  780 tests passed and 2 existing Unix-native tests skipped.
- Own lines: 1,721/1,790 (96.14%); statements 94.73%, branches 90.48%, functions
  96.78%. Lifecycle module: 92.48% lines and 86.72% branches. The Windows-host
  owned-command module measured 80.57% lines; its unexecuted Unix/native paths
  are not excluded or claimed covered. The workspace line gate is 85%.
- Active LSP probes reported no compiler errors but retained auxiliary style
  heuristics and silent-on-clean uncertainty. The passing compiler is not a
  universal clean-diagnostics claim.
- Coverage and validation logs remained outside checkout; no generated Node
  output was added to source/test trees. Existing maintained JS tooling remains.
- `git diff --check` passed; the staged file list was empty.
- OpenSpec doctor, implementation schema and all 15 strict main specs passed;
  reset roadmap validation passed. `mise run openspec-check` reported 17 passed
  and 6 failed: implementation-only changes, including this slice, are rejected
  for missing deltas by the known strict validator behavior. Archive readiness
  remains blocked; no fake requirements or tooling changes were added.

## Independent review and corrections

The initial review found two concrete integration defects. F1: native resume
alone allowed an existing endpoint with the reused TLS identity to masquerade as
the new owned execution. Start now performs a bounded read-only conflict check
before validation/staging/launch and refuses that endpoint without adopting or
stopping it. Readiness still waits for verified native start. F2: the transaction
queue refused restoration whenever its initial observation was unavailable,
preventing explicit recovery after a clean owned exit. Reconciliation now attempts
last-known-good restoration in that case and still requires successful apply plus
independent final proof. Uncertain native cleanup remains fenced and cannot start
a replacement.

The same reviewer inspected only these corrections and their blast radius,
accepted both findings as resolved, and identified no new material P0/P1/P2 issue.
Verdict: OK with notes. Native acceptance is separate: the preflight conflict
check is not an atomic endpoint-to-process identity proof.

Fresh-eyes review checked unchanged immutable snapshots, cancellation/close
ordering, protected staging retention and strict uncertain-cleanup behavior.
Simplification retained the existing launcher/queue and fixed-size stream buffers,
without adding process enumeration, a generic lifecycle framework or monitoring.

## Failures and limitations

The first worker exceeded its 30-minute harness deadline. Parent inspected and
preserved the partial sources, then revived that exact retained child through
the same subagent protocol. The timeout is not successful verification. A report
write required unavailable user confirmation; the worker stopped the write and
returned its final report for normal runtime persistence. No alternate-path or
permission workaround was used.

A pre-fix four-byte footer delayed immediate stdout overflow; a bounded payload
counter in the same native pump corrected it without changing normal stream
limits or native exit codes. One subsequent worker run transiently accepted the
32-MiB-plus-one adaptation stderr fixture. Its targeted rerun, complete focused
rerun and both parent full gates passed without suppression or fixture changes.
The transient cause remains unproved and is retained as a diagnostic limitation,
not declared fixed or attributed to the environment.

## Windows native creation-owner follow-up

A real Windows Sandbox run first failed before Caddy launch: SQLite's native
DELETE journal had the Administrators group as owner beneath a sole-user DACL.
The strict read-only descendant check correctly rejected it. A minimized native
reproduction confirmed the difference between explicitly protected paths and the
journal. No existing owner or ACL was repaired or weakened.

A guest-only default-owner prototype then proved the mechanism. After correcting
an administration-key path error in that temporary scenario, real SQLite and the
full Caddy lifecycle probe passed. Two intervening Sandbox frontend exits without
guest reports were not accepted as successful execution. A guest logon marker
made subsequent startup and scenario execution observable; the earlier missing
reports have no proved cause.

The production correction now targets the Node primary token through the
existing bounded PowerShell bridge. It requests query access first, conditional
TOKEN_ADJUST_DEFAULT only for a mismatched TokenOwner, and separate query-only
readback. The lock's existing-journal preflight still precedes normalization;
SQLite admission follows verified normalization. Unix and runtimeOwner remain
unchanged. No groups, privileges, elevation, identity or existing ACLs change.

The parent ran the production source delta in a fresh isolated Windows Sandbox
using Node 24.18.0 and separately installed Caddy 2.11.4. Input and source hashes
were verified; no prototype mutation helper was included. The guest's actual
token was elevated. Independent read-only token observation proved the default
owner changed from Administrators to the same TokenUser, with unchanged identity
and elevation. A repeat production helper call retained that owner.

Executed assertions passed:

- Real runtime-lock acquisition and user-owned native SQLite journal.
- Real pinned Caddy adaptation, guarded composition and secure initial startup,
  followed by independent matching configuration readback.
- Read-only protection checks for native-generated web CA and administration keys.
- HTTP and verified HTTPS for two projects after reload; unknown and disabled
  hosts did not receive a project response.
- Real malformed candidate rejection and independently observed last-known-good
  retention.
- Idle stop and free listeners; restart with reused administration
  material and web CA, followed by independent readback and HTTPS routing.
- Owned execution settlement, lock release, runtime removal and final free
  listeners. The guest report and bootstrap both passed; scenario exit was 0.

Runtime data and credentials stayed in guest temporary storage. Only bounded
reports were exported to task-owned system temporary storage. Input was mapped
read-only; report output was the only writable mapping. Networking, clipboard,
GPU, camera, microphone and printer sharing were disabled. No real Caddy ran on
the host, no unrelated process was stopped, and no system trust was installed.

The four-file production/test correction received independent static review with
no material finding. Mocked helper and lock-failure tests passed 63/63, covering
malformed/mismatched proof, helper failure, Unix no-op, preflight ordering and the
pending-normalization admission barrier. Script-string and mocked tests do not
execute native C# branches; the real Sandbox run supplies separate native proof.

The final `nub run check` passed TypeScript, ESLint, Prettier and V8 coverage:
35 files, 812 tests passed, 2 existing Unix-native skips. Own-source line coverage
was 1,728/1,797 (96.16%); statements 94.75%, branches 90.60%, functions 96.79%.
Coverage measures TypeScript, not C# embedded in the Windows bridge. Active LSP
probes found no diagnostics but could not confirm clean files on a silent-on-clean
server; the passing compiler is the type-check evidence. Reports stayed outside
the checkout.

Still open:

- Real-Caddy lifecycle/HTTP/HTTPS integration on Linux/macOS and native startup
  conflict scenarios. The delivered Windows leaf scenario does not exercise
  ordinary-client/elevated-daemon IPC or different-account acceptance.
- Full daemon/handler/shim Ctrl+C, shutdown, registration and recovery journeys.
- Linux/macOS execution of the changed native group-settlement path.
- Completed-product Windows privilege/Sandbox, G1/G2/G3/G7 and npm/SEA parity.
- Existing implementation-only archive/validator blocker.

Roadmap 3.8 and 3.9 remain unchecked. This increment supplies the backend needed
for those gates; no Rust removal, tagging or publication was authorized.
