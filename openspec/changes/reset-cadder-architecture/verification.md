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
