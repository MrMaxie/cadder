# Node migration verification

## Current scope correction

The user corrected functional scope to the released Rust 1.0.5 product in
TypeScript. Earlier planning results below describe the superseded broader plan
and are not validation of the corrected contract. Runtime evidence remains
historical implementation evidence only; this documentation change does not
re-run it or close any runtime gate. Current scope and checks are recorded in
`Scope correction verification` below.

The Node-only roadmap is now in tasks.md and its contract/evidence mapping is in
acceptance.md. Previous N1-N9 correspond to groups 1-9. Historical Rust checkbox
counts are not evidence; original planning/task text remains in Git and archives.
The results below predate this documentation-only planning completion and must
be rechecked for affected implementation changes. New planning validation is
recorded separately rather than being counted as application acceptance.

## Scope implemented

The Node implementation currently covers the runtime/IPC foundation, not a
shippable 2.0 application. Product handlers, Caddy, shim, state/log storage,
current CLI/TUI and npm/SEA distributions are still pending. History, IIS,
autostart and machine output are excluded, not unfinished parity tasks.
The released Rust 1.0.5 workspace and packaging are retained from `master`; older Rust behavior evidence
is pinned in `behavior-baseline.md` rather than restored into production code.

Implemented: isolated v2 paths and profiles, owner-only Unix modes and Windows
ACLs, SQLite `BEGIN EXCLUSIVE` lifetime lock, crash residue recovery only after
lock acquisition, protected persistent secret, protocol 3/security policy 2
mutual HMAC, fresh directional challenges and sequenced authenticated RPC.
Framing, connection count and timeouts are bounded. A local runtime coordinator
owns startup, listener cleanup and lock release. Diagnostic metadata is not lock
authority and cannot authorize another account.

## Current local evidence

- Node 24.18.0 on Windows x64: typecheck, ESLint, formatting and strict OpenSpec
  validation pass.
- Vitest: 29 tests pass, V8 own-code line coverage 90.30%. Runtime modules are
  included; subprocess coverage is not used to inflate the percentage.
- Packed ESM fixtures run on plain Node, with `NODE_OPTIONS` removed and without
  a TS loader. Windows x64 native gate passes.
- Linux x64 native gate passes in a local Linux environment: real Unix socket,
  real file permissions, child-process exclusion, forced crash and recovery,
  authenticated contact and orderly shutdown.
- Linux x64 privilege gate passes with a root daemon explicitly assigned to the
  normal user's runtime. The normal user authenticates successfully; an existing
  different account is denied; authenticated shutdown releases the runtime.
- Authentication tests cover fake server proof, wrong/reflected client proof,
  proof replay, RPC replay/tampering and RPC before authentication.
- Unsafe ACLs are rejected instead of repaired. A failed discovery publication
  leaves the pre-existing unsafe file unchanged and releases the listener/lock.

## Required remaining runtime gate

The execution-order restriction in this historical checkpoint is superseded by
`Approved final-product Sandbox sequencing` below. Its results and missing
privilege evidence are unchanged; no deferred test is recorded as passed.

The [native CI matrix](https://github.com/MrMaxie/cadder/actions/runs/37324338555)
passes on Windows x64, Linux x64 and macOS arm64. It verifies typechecking,
linting, formatting, native compiled-JS IPC/SQLite/process smoke and coverage.
Own-code line coverage is 90.30% on Windows and 87.95% on Linux and macOS, with
runtime modules included. Unix runners also pass real same-owner normal-user
contact with a root daemon and different-account denial.

Real same-owner user-to-elevated contact and different-user rejection remain
unverified on Windows, including Windows Sandbox. The interactive fixture and
read-only Sandbox package are prepared; preparation is not execution evidence.
Follow `windows-runtime-gate.md`. Do not mark N2 complete or port Caddy before
the Windows privilege gate passes. No host UAC policy, accounts, IIS bindings
or autostart settings are changed by preparing the package.

## Commands

```sh
nub ci
nub run typecheck
nub run lint
nub run format:check
nub run coverage
node scripts/prepare-runtime-gate.ts
```

The last command prints an outside-checkout staging directory containing test
fixtures. Run `node <stage>/runtime-smoke.mjs <stage>/runtime-child.mjs` with the
native platform Node 24.18.0. These fixtures are not npm or SEA release artifacts.
On Unix, run `node <stage>/runtime-privilege-smoke.mjs <stage>/runtime-child.mjs
<stage>/runtime-denied-client.mjs` as a non-root user with noninteractive sudo.
Only disposable test runtime files and fixture processes are used.
Coverage output also stays in system/runner temp. No publication is performed.

## Prior planning completion - 2026-10-05 (superseded scope)

This earlier checkpoint changed only OpenSpec planning documents and closed the
then-current task 1.1, not G1, G2 or the migration. Its broader functional scope
has since been withdrawn. Current task 1.1 requires validation of the corrected
plan; the counts and results below are historical, not current-plan acceptance.

- Compared the approved seven-stage migration and transition rules with the
  acceptance ledger, nine dependency groups and 69 uniquely numbered tasks.
  Historical Rust completions no longer count as Node implementation progress.
- Reviewed the shared Arcantry configuration and local OpenSpec schema/templates.
  The major-impact release artifact records planned consumer outcomes, without
  changing released version sources, manifests or the managed changelog.
- Verified requirement references against main and delta specs, exact rename
  provenance, absence of duplicate IDs/task numbers and roadmap entry links.
- Verified that `openspec show reset-cadder-architecture --json --deltas-only`
  retains all 45 full requirement statements across 15 capabilities, plus four
  explicit rename deltas. Requirement paragraphs use one physical line because
  the installed parser otherwise exposes only their first line.
- `openspec doctor`, both local schema validations, strict change validation and
  `openspec validate --all --strict --no-interactive` pass: 16 items, zero failures.
  `git diff --check` passes. Artifact readiness means documents exist, not that
  implementation or consumer distributions have passed acceptance.

The configured local Arcantry launcher cannot run because its Windows platform
package is missing. Configuration/templates were reviewed directly; no Arcantry
CLI release validation, installation or release writes were performed.

No application tests, typechecks, builds or runtime execution were performed at
this documentation-only checkpoint. Runtime code, dependencies, CI and released
versions are unchanged. No spec sync/archive, commit, push, tag or publication
was performed.

## Scope correction verification

This documentation-only correction follows the user's decision to preserve the
current released product, not restore superseded features. It was checked on the
working tree based on `f850418a40991bb54c45e5fb56df1232d286c746`; the corrected
OpenSpec files are uncommitted. There are no new runtime or artifact results.

- Compared current main CLI/TUI, IPC, observability, topology and storage specs
  with the migration deltas and the released CLI grammar. Preserved bare help,
  explicit tui --start-daemon, routes-only TUI, human output, eight operations,
  bounded canonical log streams and independent installation runtimes.
- Withdrew IIS/AUTO/OBS-005 deltas and former tasks 4.5-4.9; removed their parity,
  documentation and transition claims. Historical planning evidence above is
  explicitly superseded rather than relabeled as current acceptance.
- Reviewed this correction against a pre-edit working-tree snapshot, preserving
  unrelated existing edits. Updated design, roadmap, acceptance and planned
  release outcomes consistently; no main-spec sync or archive was performed.
- `openspec doctor`, `openspec schema validate implementation` and
  `openspec schema validate spec-driven` pass.
- `openspec validate reset-cadder-architecture --strict --no-interactive` and
  `openspec validate --all --strict --no-interactive` pass; the whole view reports
  16 items, zero failures. `git diff --check` passes.
- `openspec show reset-cadder-architecture --json --deltas-only` exposes 39
  requirement statements across 12 capabilities and three rename deltas, with
  nonempty requirement text and scenarios. An in-memory scope check confirms
  unique requirement IDs and 65 unique task IDs, no withdrawn delta directories
  and no IIS/AUTO/OBS-005 task references.
- Active Markdown LSP probes were unavailable because no server was ready;
  they are not a clean diagnostic result. OpenSpec validation and manual review
  are the completed documentation checks.

This closes corrected planning task 1.1 only. Tasks 1.3-1.5, G1/G2 and all product
acceptance gates remain open. No application tests, builds, Sandbox execution,
version changes, commits, pushes, tags or publication were performed.

## Reviewed contract synchronization and compatibility draft

Source baseline: `93942af84f555eab2bd68e0a4a9db487ff82d040` on
`feat/node-runtime-migration`, with uncommitted spec/catalog changes. These are
working-tree checks, not exact-revision G2 acceptance or a released artifact.

- Applied all 39 accepted delta blocks into main specs: 33 modified and six
  added requirements, including three title renames. Main specs now contain 60
  requirements. The active migration roadmap was not archived.
- Independent MiMo review compared every changed block to its delta and confirmed
  unchanged requirement content. Appending new requirements adds only a blank
  separator after three previously final blocks; their content is unchanged.
- `openspec validate --specs --strict --no-interactive` passes: 15 specs.
  `openspec validate --all --strict --no-interactive` passes: 17 items before
  creation of the focused RPC implementation slice.
- Parent `nub run check` passes: 40 tests, 90.3% own-code line coverage,
  typecheck/lint/format checks pass. The synchronized CLI text anchor was updated
  without changing the retained behavior catalog.
- Built current untouched Rust 1.0.5 with
  `cargo build --locked -p cadder-client -p cadder-shim`. No Rust source or Cargo
  manifest/lock changes occurred. Redirected Windows x64 process captures show
  bare help and missing nested subcommands exit 0 on stderr; explicit help and
  version exit 0 on stdout; removed inputs exit 2 on stderr. Seven exact output
  snapshots are retained in `test/compatibility/cli-output.json`.
- After correcting those measured stream/exit expectations, the focused catalog
  suite passes 12 tests; typecheck, fixture formatting and git diff --check pass.
  No TTY capture, populated process-output snapshots, real shim stream test or
  Caddyfile adaptation result is claimed by these source-backed fixtures.
- `close-node-rpc-catalog` is the focused IPC-001..003 implementation slice.
  It does not duplicate normative requirements or modify the separate retained
  Rust release implementation change.

After the focused slice was created, `mise run openspec-check` reports 17 passes
and one failure: OpenSpec 1.5 requires at least one delta for the configured
spec-free implementation schema. `cargo xtask openspec-check` is unavailable in
this checkout (no cargo alias or xtask manifest). No fake delta or new validator
was added. This is an open tooling limitation, not a failed main-spec sync.
The latest parent `nub run check` passes 41 tests with 90.3% line coverage.

Task 1.4 is delivered. Task 1.3 remains open for missing compatibility evidence;
1.5, 2.4, 2.5, 2.8, 2.9 and G1/G2 remained open at that checkpoint. No Caddy port,
Sandbox privilege execution, version change, commit, push, tag or publication was
included.

## Closed RPC verification

Source baseline remains `93942af84f555eab2bd68e0a4a9db487ff82d040`; Node/spec
changes are uncommitted. Windows x64 uses Node 24.18.0 and Nub 0.7.5. This is
working-tree evidence, not one-revision native G2 acceptance.

- The catalog admits exactly eight operations with strict Zod payload/result/error
  DTOs and method-indexed client types. Unknown fields/methods do not invoke
  handlers. Requests, nested results/errors and envelopes correlate IDs, with
  exactly one result or error. Business rejection remains accepted:false.
- Decode accepts missing/null serde Option inputs. Canonical skip-serializing
  fields omit null; other nullable fields are normalized as designed. Log input
  default/clamping policy remains separate from the CLI default.
- Runtime fixtures use query-state and shutdown. Recovery is fixture stdout, not
  a product snapshot property. No product handler, Caddy adapter, storage worker,
  Rust interoperability or production mock was introduced.
- Independent MiMo Pro review passed the complete RPC seam. A subsequent parent
  review found oversized valid responses could lose the correlated outcome.
  The repair preserves send sequence on synchronous failure and sends one small
  correlated frame error. MiMo Flash's resumed narrow review passed the repair;
  its earlier timed-out attempt produced no acceptance evidence.
- Parent `nub run check` passes 67 tests in 12 files; own-code line coverage is
  91.78%. Typecheck, lint and formatting pass. The typecheck resolves the
  reviewer's speculative unused-ts-expect-error concern: no TS2578 is reported.
- Parent `node scripts/prepare-runtime-gate.ts`, followed by
  `node <stage>/runtime-smoke.mjs <stage>/runtime-child.mjs`, passes on win32/x64
  without a TS loader. Emission and disposable runtime files remain outside
  checkout. This smoke is a fixture runtime gate, not npm/SEA product acceptance.
- Parent `openspec validate --specs --strict --no-interactive` passes 15 specs;
  git diff --check passes. The spec-free implementation validation mismatch is
  unchanged and is not hidden by adding duplicate deltas.

This delivers task 2.4 only. Four port declarations exist but task 1.5 awaits
final review. Catalog gaps in 1.3, runtime failures/isolation in 2.5, real Sandbox
in 2.8, exact-revision/native reconciliation in 2.9 and G1/G2 remain open. A
non-frame send-failure test remains a review-noted coverage gap; source inspection
shows it fails closed. No Caddy port or external delivery action is authorized.

## Ports and lifecycle cleanup verification

At the same source baseline with intentional working-tree changes, the parent
reported `nub run check` passing 130 tests with 96.85% own-code line coverage.
Independent MiMo Flash review passed the repaired four-port contract: project
and domain activation round-trip, full adapted JSON carrier, method/result/error
types and import allowlist. This closes task 1.5, not the pending handler or
adapter implementation.

Independent MiMo Pro review passed the lifecycle cleanup repairs. Parent tests
cover failed startup and shutdown cleanup for the runtime lock, protected files,
listener and discovery publication. These results are supplied review/parent
evidence; they are not a new native privilege or full-disk gate.

## Installation identity and native filesystem boundaries

The working tree still starts from `93942af84f555eab2bd68e0a4a9db487ff82d040`.
Before the identity change, a Windows source reproducer supplied two real
installation directories and measured identical directory, endpoint, lock and
application-database paths. The repaired default retains the per-user OS base
and adds `v2/<16-hex digest>` of the canonical physical installation root.
Physical aliases agree. npm derives the nearest cadder package anchor above the
shared module, not the shared Node executable, argv shim or working directory.
SEA identity follows the real executable's directory. Missing roots/anchors fail
closed. Explicit runtime-directory precedence, empty overrides and profile
behavior remain unchanged; the root-owner startup policy is untouched.

Windows x64 source tests prove distinct endpoint, secret, SQLite lock, metadata,
discovery and application-database paths for two roots. Two real SQLite lifetime
locks and local listeners run simultaneously with authenticated clients; stopping
and restarting one leaves the other's client contact and exclusion intact.
Secret bytes and published diagnostics remain installation-specific. Application
storage independence is **path-only** until task 5.1; the two live databases in
this proof are lock databases, not application storage.

Native Windows tests reject direct directory-junction/reparse substitutions at
the runtime directory, secret, lock, metadata and discovery boundaries. File
artifact substitutions use directory junctions, not privileged file symlinks.
Direct protection checks classify each link as unsafe; startup rejects the
metadata-directory substitution earlier with native EISDIR. Broad and inherited
DACLs are denied at all five boundaries. Existing sentinel content and ACL
snapshots are unchanged after denial. Owner checks read real ACLs against a
mismatched expected SID without creating accounts or transferring ownership;
this is not different-account privilege acceptance. Unix mode/link/UID branches
are present in the tests but were not executed on native Unix in this run.

The first focused attempt reported 27 passes and 12 failures: ten test-fixture
Set-Acl operations requested SeSecurityPrivilege, one empty-environment setup
selected the retained cwd override semantics, and one junction assertion
expected the protection error rather than the earlier native EISDIR. After
approval, fixture-only DACL updates use System.IO access/owner sections without
SACL operations or privilege changes. One broad and one inherited ACL case then
passed; the focused suite passed all 39 tests before the empty-override regression
was added. Final focused validation passes 40 tests in four files. Final
`nub run check` passes 162 tests in 20 files, with 98.85% own-code line coverage,
typecheck, lint and formatting passing. Fresh-eyes review repaired the initial
empty-override append condition and added a regression; simplification removed
redundant test assertions. `git diff --check` passes. No production ACL adapter
change was needed for these direct cases.

Plain-Node source reproduction initially hit unsupported TypeScript parameter
property syntax. Installed tsx was used for that source reproducer; native gates
use esbuild-compiled ESM without a loader and with NODE_OPTIONS removed. The
existing crash/restart/shutdown smoke and new simultaneous-installation smoke
both pass on win32/x64. Bundles and disposable runtimes are outside checkout and
were removed afterward. These are fixture gates, not npm/SEA release artifacts.

Remaining blockers keep task 2.5, 2.8, 2.9 and G2 open:

- The reproduced ancestor-junction escape is denied by the ancestor preflight
  follow-up below. This is a snapshot link check, not atomic path identity or
  arbitrary ancestor ACL hardening. Check-to-use races and same-owner path swaps
  remain residual risks.
- Unix sockets reject paths over 103 UTF-8 bytes before runtime creation, with an
  actionable shorter-runtime-directory error. Tests retain 103/104-byte and
  multibyte coverage. The socket-budget follow-up removes only the cosmetic
  `install-` prefix on every platform, retaining the full 16-hex digest, per-user
  base, v2 and dev suffix. The modeled normal macOS default drops from 107 to 99
  bytes; long/multibyte homes and long dev-profile paths still fail closed. The
  placement-only Darwin fixture skip is removed, but native macOS default
  usability remains unproven, not inferred from Windows tests.
- Native Unix reruns, all-native isolation matrices, actual npm/SEA distributions,
  application storage behavior, full-disk cases and Windows Sandbox same-owner
  elevation/different-account privilege acceptance remain unproven here. No host
  accounts, UAC, IIS, security policy or privileged host operations were changed.

### Socket-budget follow-up

MiMo Pro passed the delivered identity/boundary slice without closing task 7 or
G2. This follow-up removes eight cosmetic bytes, not hash entropy, and declares
`installationRoot` in the internal runtime-start options. Overrides are unchanged;
there is no public flag, socket relocation or platform-specific production namespace.

Windows x64 focused validation passes 42 tests in four files. Final
`nub run check` passes 164 tests in 20 files with 98.85% own-code line coverage;
typecheck, lint and formatting pass. Actual-platform path coherence, sibling
resolver agreement, normal modeled macOS placement and long/multibyte denial are
covered. Both external compiled Windows smokes pass again, including simultaneous
locks/listeners, authenticated contact and independent stop/restart. Emitted
bundles and disposable runtimes were removed. `git diff --check` passes.

The Darwin integration fixture is prepared with an exclusively created short
synthetic home, restored environment and cleanup after all runtime finalizers.
It never writes to the real user's home. This test-only setup is not native
macOS user-default acceptance or an exception to unresolved ancestor-link policy.
One initial full check rejected a cleanup throw under no-unsafe-finally; the
fixture now awaits all finalizers before propagating rejection and removing its
owned root. Fresh-eyes/simplifier review found no further scoped defect at that
checkpoint. Ancestor escape and task 2.5/2.8/2.9 were still open then; the ancestor
follow-up below delivers only the reproduced link-boundary repair.

## Compatibility fixture follow-up — released 1.0.5 source and executable evidence

Baseline: `93942af84f555eab2bd68e0a4a9db487ff82d040` on
`feat/node-runtime-migration`, Windows x64. This follow-up changes only
`test/compatibility/**` plus this evidence ledger; no product implementation,
Caddy port, Sandbox/UAC/account setup, or host runtime settings were changed.

The compatibility catalog now includes concrete `cadder.toml` TOML cases,
multiline supported and malformed Caddyfile cases, captured CLI snapshots,
eight reusable request/result payload pairs for the eight-operation Node DTO
catalog, one concrete fixture per operation, nested registration identity,
optional fields, RPC correlation, CLI/RPC log defaults, and populated output
rows. Supported TOML snippets are parsed with installed `smol-toml`; all eight
request/result fixtures are parsed with the existing closed `src/protocol/rpc.ts`
Zod schemas. No duplicate Caddyfile parser or precedence engine was added.

Focused released Rust 1.0.5 tests (all passed; exactly 67 tests) were run as
source/parser/fixture authority:

- `cargo test -p cadder-ipc`: 17 tests (13 unit + 4 wire compatibility).
- `cargo test -p cadder-daemon --lib config::tests`: 6 tests.
- `cargo test -p cadder-shim --bin cadder-shim command_policy::tests`: 1 test.
- `cargo test -p cadder-shim --bin cadder-shim registration::tests`: 4 tests.
- `cargo test -p cadder-client --bin cadder-client cli::output::tests`: 6 tests.
- `cargo test -p cadder-daemon --lib caddy::tests::trusted_caddy_source`: 18 tests.
- `cargo test -p cadder-daemon --lib caddy::tests::mock_adapter`: 2 tests.
- `cargo test -p cadder-daemon --lib caddy::tests::adapter_`: 5 tests.
- `cargo test -p cadder-shim --test shim_binary`: 8 tests, including success and nonzero released-shim passthrough.

The initial package-target attempts using `--lib` for `cadder-shim` and the
wrong `cadder` bin name for `cadder-client` were target-selection errors, not
zero-test evidence; corrected commands above supplied the evidence. No filtered
zero-test run is counted.

Node fixture checks passed with Nub: `nub exec vitest run
 test/compatibility/catalog.test.ts` (14 tests), `nub run typecheck`, `nub run
lint`, and Prettier. Final `nub run check` passes 227 tests in 22 files with
98.92% own-code line coverage; typecheck, lint and formatting pass. Full
`cargo test --locked --workspace` passes 300 tests; workspace clippy with
`-D warnings`, Rust formatting, and `git diff --check` pass.

Actual CLI evidence remains the seven redirected non-TTY captures in
`test/compatibility/cli-output.json`, built from fresh 1.0.5 `cadder-client`
and `cadder-shim` targets: bare invocation and missing nested groups exit 0 on
stderr, explicit help/version exit 0 on stdout, and rejected arguments exit 2
on stderr. This follow-up adds released-shim passthrough proof through the
existing native `cadder-test-process` fixture: copied fixture argv preserves
`fmt`, spaces, Unicode and quotes; piped stdin bytes are recorded; stdout and
stderr markers remain independent; and child exit codes 0 and 23 are returned
unchanged. The test uses an isolated copied released shim plus trusted portable
`cadder.toml`, with a hermetic child-only PATH and temp paths; stdin is
explicitly closed before a bounded ten-second child wait with kill/wait cleanup;
no daemon or real Caddy is launched. No TTY execution is claimed. The Windows Sandbox
package and launcher were prepared and launched before the user's deferral;
UAC, account-boundary acceptance, task 2.8 and G2 remain explicitly deferred
and open. Task 1.3 remains open pending actual TTY evidence. Tasks 1.4 and 1.5,
2.4 and 2.5 are not changed by this entry.

## Runtime ancestor preflight

The IPC-001 repair preserves installation-root alias identity while denying
arbitrary linked runtime ancestors before directory creation, protected-file
open or leaf trust. Only actual Darwin top-level /tmp, /var and /etc links owned
by UID 0 and resolving to their corresponding fixed /private directories are
recognized. Nested, user-owned, wrong-target or other root-owned links are denied.
Ordinary real parents retain their existing owner/mode/ACL policy; there is no
new anchor, port input, namespace, persistent trust state or metadata authority.
The inspection cursor may follow only those verified OS aliases; original
runtime, endpoint and diagnostic path spelling remains unchanged.

Native Windows red testing first showed a protected leaf through an ancestor
junction was accepted. The initial guard then passed 63 focused cases, but
fresh-eyes review reproduced a missing/../junction escape in owned temp paths:
both directory and file creation incorrectly succeeded. After approval, the same
helper was replaced by a finite component cursor that inspects each prefix before
consuming later dot/dot-dot components and continues after missing prefixes.
Both regressions are now green, with no target content/ACL change, no listener or
publication, and no mkdir/open through the linked parent. Ordinary real-directory
dot/dot-dot paths still work. Direct leaf link/ACL denial is retained.

Five existing fault tests initially failed because once-only lstat mocks injected
leaf-phase errors into new ancestor checks; a sixth case passed prematurely.
Authorized path-specific corrections in protected-file-faults.test.ts and
unix-security-policy.test.ts preserve the original failure/cleanup expectations
and explicitly pin the intended leaf, write/sync/close and mkdir/protection phases.
No production bypass was added to accommodate mock ordering.

Final Windows x64 focused validation passes 95 tests in eight files. Final
`nub run check` passes 209 tests in 22 files, with 98.91% own-code line coverage;
typecheck, lint and formatting pass. Modeled Unix/Darwin alias cases and modeled
Windows drive/UNC/mixed-separator cases are not native macOS/Linux or network-share
acceptance. External compiled Windows gates pass for crash/restart/shutdown,
simultaneous installation locks/listeners/authentication and ancestor denial,
without a TS loader. Disposable runtimes and compiled bundles were removed.
Fresh-eyes/simplifier review found no further scoped defect; git diff --check passes.

This does not close task 2.5/2.9 or G2. Native Unix matrices, full-disk cases and
actual npm/SEA distributions remain unproven; application database independence
is still path-only until task 5.1. Windows Sandbox privilege acceptance remains
user-deferred, not executed. Check-to-use races, including same-owner swaps, remain;
no atomic openat or inode/metadata binding is claimed. No host accounts, UAC,
security policy, privileges, Rust or normative requirements were changed.

### Trailing-separator leaf correction

MiMo Pro passed the ancestor slice and identified a trailing-separator leaf issue:
POSIX lstat can follow a directory link when the leaf ends in a slash. Modeled
POSIX prepareRuntime/assertProtected regressions both failed before correction;
native Windows junction cases already denied the same spellings, so no Windows
escape or native Unix execution is claimed from that red run.

The correction trims only trailing leaf separators for parent computation and
leaf lstat, preserving POSIX, drive and UNC roots and every dot/dot-dot component.
Caller, ACL and diagnostic spelling remains unchanged. Reconsideration after the
local defects retained the existing necessary finite link preflight; this is a
small syntax correction, not another trust layer or algorithm redesign. Darwin
alias recognition and missing/dot-dot denial remain intact.

Final Windows x64 focused validation passes 113 tests in eight files; full
`nub run check` passes 227 tests in 22 files with 98.92% own-code line coverage,
typecheck, lint and formatting passing. Native junction target content/ACLs are
unchanged after denial; ordinary real directories still work. Root controls are
modeled, without modifying host root permissions. External compiled Windows
runtime, installation and ancestor/trailing-leaf smokes pass without a TS loader;
disposable runtimes/bundles were removed. Fresh-eyes/simplifier review found no
further scoped defect; git diff --check passes. TOCTOU/same-owner swaps, native
Unix, application-storage/distribution proof and the remaining task7/G2 gates
remain open; Windows Sandbox remains user-deferred.

## Integrated nine-task checkpoint

The parent verified the final working-tree source/test set over base HEAD
`93942af84f555eab2bd68e0a4a9db487ff82d040`. This is uncommitted evidence,
not acceptance of a new committed revision. The SHA-256 fingerprint is
`fe62f295286fd9af23425ad3d16bde28da884f219329faf51c3bc67afbcc4141`.
It covers 198 source, test, script, main-spec and manifest/configuration files.
The receipt lists each repository-relative path and its file SHA-256; the
aggregate hashes sorted `path<TAB>lowercase SHA256<LF>` entries as UTF-8.
Planning/evidence documents, generated artifacts and private files are excluded.
The receipt is retained outside checkout and introduces no project tooling.

Final parent checks for this source set:

- `nub run check`: 227 tests in 22 files; 98.92% own-code line coverage;
  typecheck, lint and formatting pass.
- `cargo test --locked --workspace`: 300 tests pass. `cargo fmt --check` and
  workspace/all-target Clippy with `-D warnings` pass. Rust production behavior
  and release manifests remain unchanged; two additional passthrough tests and
  modes in the existing native fixture supply baseline evidence.
- Final `cargo test --locked -p cadder-shim --test shim_binary`: 8 tests pass
  after the stdin/deadline/hermetic-PATH correction, including exact argument
  arrays, input bytes, separate output streams and child exit codes 0 and 23.
  These are actual redirected binary runs, not TTY acceptance. An earlier long
  tool notification was not counted as a successful execution by itself.
- A newly compiled plain-JS runtime smoke passes on win32/x64 without a TS
  loader. Its disposable staging directory was removed.
- Strict main-spec validation: 15 pass. Strict all-item validation: 17 pass,
  one fails for the existing spec-free implementation change's missing-delta
  mismatch. No duplicate requirements or validation-policy workaround was added.
- `git diff --check` passes; nothing is staged. An active LSP probe did not
  conclusively certify all 12 source files; auxiliary AST findings are not a
  replacement for the passing TypeScript compiler and maintained lint checks.

The nine approved outcomes are reconciled individually:

| Task | Outcome at this checkpoint | Remaining evidence or decision |
| --- | --- | --- |
| 1: CLI/shim catalog | Partial: argument/output fixtures and native passthrough proof exist; the follow-up below adds populated Windows renderer captures | Actual TTY remains unproven; renderer captures are not full operator journeys; 1.3 stays open |
| 2: configuration/model catalog | Partial: TOML/Caddyfile examples, released parser/resolver tests and all eight typed payload fixtures exist | Caddyfile examples are not a claim of a newly implemented Node parser or full adaptation parity; full 1.3 closure remains open |
| 3: spec synchronization | Delivered; strict main specs and independent delta-fidelity review pass | Roadmap remains open, not archived as completed migration |
| 4: four ports | Delivered; both material contract defects were repaired and independently reviewed | Production handlers/adapters are later work |
| 5: closed RPC | Delivered; eight methods, strict schemas, correlated outcomes and oversized-response handling are tested/reviewed | No product-handler or Rust/Node interoperability claim |
| 6: startup/teardown cleanup | Delivered for current runtime phases, including SQLite/file I/O faults and subsequent finalizers | Application-storage worker and native full-disk proof are not supplied by this slice |
| 7: installation/filesystem boundaries | Partial: live independent lock DBs/listeners and Windows leaf/ancestor/ACL denial pass | Application DB independence is path-only; current-source native Unix matrices and remaining storage evidence are missing |
| 8: Windows Sandbox privilege gate | Explicitly deferred by the user, not passed | A package and launcher were prepared/started before deferral; no real UAC or different-account acceptance was obtained |
| 9: one-revision G1/G2 reconciliation | Evidence collected and gaps identified, not gate acceptance | Compatibility catalog, native/security/storage proof and a finalized source revision remain incomplete |

G1 and G2 remain open. The user-deferred Sandbox is not silently waived from the
accepted gate. Caddy porting, staging, commits, tags, pushes and publication were
not performed. This checkpoint does not declare the nine-task stage or the Node
migration complete.

Final independent MiMo Pro review passes the integrated closed-RPC/four-port
slice and the nine-row evidence ledger without completion inflation. Only
`close-node-rpc-catalog` final verification task 3.1 is now complete; its own
verification artifact retains the archive/tooling blockers. The review also
records that teardown may close a received-but-undispatched request without a
correlated shutdown envelope. This is transport cancellation, not a guaranteed
RPC delivery result; no shutdown redesign was introduced. Broader tasks and
G1/G2 remain open.

### Current-source Linux fixture evidence

The parent additionally ran the existing compiled runtime smoke and exported
installation-runtime smoke on linux/x64 using the already-cached
`node:24.18.0-bookworm-slim` image. Node version 24.18.0 was checked for the
installation run. No image was pulled and no tools or packages were installed.
Each disposable container used UID/GID 1000, disabled networking, dropped all
capabilities, enabled no-new-privileges and a read-only root/input mount, and
kept runtime files in a writable temporary `/tmp`. Containers and external
compiled staging were removed after execution.

Both gates pass: real Linux Unix sockets, SQLite exclusion, child crash/restart,
authenticated catalog RPC and shutdown; and two simultaneous installation
locks/listeners with independent authenticated contact and stop/restart. This
is current-source Linux fixture evidence, not npm/SEA product acceptance.
It does not supply the full Unix security/fault matrix, privileged-owner gate,
macOS native execution or application-storage behavior. The local WSL runner
had no available Node and the cached container had no sudo; neither environment
was modified to manufacture privilege evidence. The source fingerprint is
unchanged. G1/G2 and the remaining approved-task gaps stay open.

## Executed Windows human-output catalog

The continuation preserves the released operator's human-readable output; it
freezes renderer results for later Node parity without changing production Rust
or implementing a Node operator. Base HEAD remains
`93942af84f555eab2bd68e0a4a9db487ff82d040`; all migration work is uncommitted.

`test/compatibility/human-output.json` contains four executed renderer captures:

| Exact Rust test name | Covered output | UTF-8 bytes, including final LF |
| --- | --- | --- |
| list_and_status_outputs_cover_empty_and_populated_snapshots | Empty/populated status, projects and domains | 702 |
| port_output_covers_socket_and_registration_availability | Socket owners, matched routes and unavailable/empty states | 805 |
| route_outputs_cover_registered_unregistered_and_socket_notes | Registered/unregistered Caddyfiles, domains and socket notes | 2058 |
| diagnostics_and_logs_cover_empty_and_detailed_reports | Empty/populated diagnostics and bounded logs | 757 |

The capture command for each name is:

```sh
cargo test --locked -p cadder-client --bin cadder-client cli::output::tests::<name> -- --exact --nocapture --test-threads=1 --color never
```

The parent repeated all four commands on Windows x64 and compared the renderer
stdout between libtest markers with each fixture, preserving whitespace and the
final LF. All four comparisons pass. The initial worker captures omitted that
last LF; this was corrected before acceptance. The fixture file SHA-256 is
`eafad435c510eccb77febd4046dcbd1cbbd69f2d557e3328870832a6c3ab3bfa`.
These are renderer-test process captures, not operator IPC journeys or real TTY
runs. Windows path separators are part of the goldens; cross-platform parity
requires native captures or explicit path normalization, not reuse as portable
byte-exact output.

The only Rust change in this continuation fixes existing test-input timestamps
to 2026-01-01; it is inside cfg(test). The Node catalog consumes the captures,
checks their final LF and absence of actual CR characters, and ties status/log
expectations to captured output. Independent MiMo Pro review identified the CR
guard and unverified status/log entries; the parent repaired them. The parent
also added explicit citations for precedence and Caddyfile examples. The retained
hostless sibling composition test proves adapted-route composition, not acceptance
of the example Caddyfile. Mock malformed-input tests and local/remote upstream
inspection tests likewise prove their named aspects, not real-Caddy parser parity.
No duplicate parser, renderer, terminal harness or validation-policy workaround
was added.

Parent validation after these repairs:

- `cargo test --locked -p cadder-client --bin cadder-client cli::output::tests -- --nocapture --test-threads=1`: six tests pass.
- Four exact capture commands above: one selected test passes per command and all fixture comparisons pass.
- `nub run check`: 227 tests in 22 files, including 14 catalog tests; own-code line coverage 98.92%; typecheck, lint and formatting pass.
- `cargo test --locked --workspace --quiet`: 300 tests pass; Rust formatting and workspace/all-target Clippy with `-D warnings` pass.
- Strict main-spec validation: 15 pass. Strict all-item validation: 17 pass and the same spec-free implementation missing-delta failure remains.
- Active LSP checks on the three changed code files were inconclusive, not certified clean; compiler/lint results above supply the completed checks.

The updated source/test fingerprint uses the same receipt format as the integrated
checkpoint, adding the new human-output fixture: 199 files, SHA-256
`cf86d7bfbf49057b24552304cd08c265dc872468f6a9fea432d2c95ec02181aa`.
The receipt stays outside checkout. Earlier runtime smokes were not rerun for
these test-only changes and are not relabeled as new native acceptance.

MiMo Flash's independent scope assessment separates G1 catalog freezing from
later Node product execution. No bounded existing Windows TTY capture mechanism
was available for this run; source inspection is not a substitute for actual
terminal execution. Actual bare-help TTY acceptance remains missing, so 1.3/G1
stay open. Full Node operator journeys and platform/channel parity remain later
gate obligations. G2 still lacks its remaining native/security/storage evidence;
Windows Sandbox remains user-deferred, not waived. No Caddy integration migration,
staging, commit, publication or host privilege/account changes were performed.

Final retained MiMo Pro re-review: PASS. All five earlier findings are resolved;
the reviewer independently checked the fixture hash, byte lengths, CR/LF endings,
source citations and test-only Rust boundary and found no new defect. This closes
the rendered-catalog repair slice only. The complete evidence carrier is this
ledger entry, not the incomplete superseded worker report. Actual TTY, 1.3/G1
and G2 remain open.

## Approved final-product Sandbox sequencing

The user approved completing the product before testing Windows Sandbox, and
explicitly authorized the next configuration/Caddy-preparation implementation.
This is a planning-order change, not executed security evidence or a change to
owner/elevated access requirements. Actual TTY and incomplete native/storage
checks remain open; G1/G2 have not passed.

Updated roadmap, acceptance, design, Node scope, proposal, roadmap README and
agent guidance permit groups 3-6 implementation against delivered contract/runtime
boundaries. Tasks 2.8 and 7.4 share actual completed-product Sandbox evidence;
2.9 records final-source G2 acceptance before G7, Rust removal or release. The
retained Sandbox guide is labeled as a reference runtime-fixture procedure,
not a current implementation prerequisite or final-product proof.

The next bounded slice preserves existing project Caddyfiles and trusted
cadder.toml selectors: resolve real Caddy, adapt project configuration and
validate JSON through bounded owned child processes. Composition, apply/rollback,
mTLS server setup and Caddy server lifecycle remain later tasks. No Sandbox,
UAC/account action, requirement waiver, public command, dependency, version,
commit or publication change is included in this sequencing decision.

## Node Caddy preparation implementation (3.1-3.2)

The focused `migrate-caddy-preparation/verification.md` records configuration,
trusted image pinning, bounded adaptation/validation, Windows Job ownership and
independent MiMo review followed by parent repairs. Roadmap 3.1-3.2 implementation
checks pass: final `nub run check` has 352 passing tests, two Unix-native skips and
96.34% own TS/TSX lines; the Caddy/owned-command slice has 93.75% lines. Final LCOV
includes adapter/preparation even though the compact text table hides those fully
covered files. Embedded C# has behavioral evidence, not V8 statement coverage.

The shared-pin caller-cancellation defect and quoted Windows PATH regression
were repaired; bounded stream settlement, complete owned Job termination and
transient staging retries were verified on Windows. Root-relative Windows path
qualification was also corrected. No apply/server/CLI integration is claimed.

`mise run check` passes Rust formatting, Clippy and 300 tests, then remains blocked
by the known OpenSpec 1.5 missing-delta failure for the two spec-free implementation
changes. Strict main specs (15), roadmap and implementation schema pass; downstream
docs/npm/cargo-dist steps were not reached. Both completed source slices remain
unarchived; no fake deltas or validation-policy workaround were introduced.

G1/G2/G3, RUN-003 server shutdown and whole CADDY-005 acceptance remain open.
Native Unix/group-escape behavior, real Caddy, built npm/SEA and final-product
Sandbox proof remain pending. Existing dirty work is preserved and nothing was
staged, committed, published or cut over from Rust.

## Node Caddy route composition implementation (3.3)

The focused `migrate-caddy-composition/verification.md` records pure canonical
ownership and guarded route-plan composition in src/caddy/domains.ts and
composition.ts, with 63 dedicated JSON tests. Final `nub run check` passes
415 tests in 27 files, with two existing Unix-native skips and 96.81% own TS/TSX
line coverage; the new modules cover 167/168 lines (99.40%).

MiMo Flash checked retained behavior and boundary risks. MiMo Pro independently
reviewed source/tests and identified two P2 notes, no P0/P1. The parent reproduced
and repaired terminal-parent pruning so it preserves chain position after its
subroute handlers disappear; a second fixture confirms opaque proxy-shaped
payloads do not contribute upstream metadata. The same reviewer rechecked those
changes and returned OK. Original upstream/timeout failures remained failures;
exact-run native resumes supplied the completed opinions without protocol fallback.

Roadmap 3.3 closes its composition checks only. A route plan is not runnable
CaddyConfig: project admin/listener/TLS policy cannot enter it, and no validation,
apply, server or active-state integration is added. Real Caddy terminal execution
and exhaustive Node/Rust IDNA parity are not inferred from these fixtures.

Strict main specs (15), roadmap, schema and whitespace checks pass. The maintained
OpenSpec gate still reports 17 pass and three spec-free implementation changes
rejected by the known missing-delta validator; no fake deltas or tooling-policy
workaround was added. Focused changes remain unarchived. G1/G2/G3, transactional
queue/rollback, protected mTLS, lifecycle, native/distribution acceptance and
final-product Sandbox remain open. Nothing was staged, committed or published.

## Node configuration transaction orchestration (3.4-3.5)

The focused `migrate-caddy-transactions/verification.md` records one daemon-owned
queue in src/daemon/configuration-transactions.ts: preparation against the latest
committed snapshot, validation, explicit apply outcomes, independent observation,
DesiredState-only atomic persistence and internal publication. Pending data remains
isolated. Persistence failure restores and verifies last-known-good, including
idle; uncertainty/failed rollback fences mutations until explicit queued recovery.

The final parent `nub run check` passes 480 tests in 28 files, with two existing
Unix-native skips and 1155/1191 own TS/TSX lines (96.97%). The dedicated suite has
65 passing tests; the new module has 92/93 lines (98.92%) and 50/50 branches.
MiMo Flash critiqued the plan and MiMo Pro independently reviewed actual source/tests:
no P0/P1, one P2 wording item. The parent clarified that null observation means
positively observed idle, never unknown/unavailable state, and added a first-apply
regression proving failed readback stays fenced while a candidate is live.

Roadmap 3.4-3.5 closes orchestration checks, not real runtime/storage integration.
Required production adapters have no successful mock defaults. Secure Caddy
readback/idle transitions, the SQLite worker's atomic failure contract, independently
observed initial state and registration/heartbeat ownership integration remain
later dependencies. Whole CADDY-003 and G1/G2/G3 acceptance remain open.

Strict main specs (15), roadmap, implementation schema and whitespace checks pass.
The maintained OpenSpec gate reports 17 pass and four spec-free implementation
changes failing the known missing-delta validator. No fake delta or checker
workaround was introduced; focused changes remain unarchived. mTLS, owned server
lifecycle, native/distribution and final-product Sandbox proof remain pending.
No stage, commit, publication, real Caddy or host privilege/account action occurred.
