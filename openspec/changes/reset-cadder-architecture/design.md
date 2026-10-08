## Context

This is the executable design for the approved Cadder 2.0 migration. The approved
scope is summarized in `node-migration.md`; normative deltas live in `specs/`.
`tasks.md` is the only implementation checklist and `acceptance.md` is the
requirements-to-evidence ledger. `verification.md` contains actual evidence, not
future acceptance claims.

The retained operational baseline is Rust 1.0.5 from master. The released CLI/TUI
and main specs define functional scope. `behavior-baseline.md` distinguishes
current evidence from superseded historical behavior; the historical snapshot
is not authority to restore removed features. The original Rust reset and its
checkboxes remain recoverable in the archived change and Git snapshot `f850418`;
they are not Node progress. Current Node code supplies runtime/IPC fixtures,
not product handlers, CLI/TUI or releasable distributions.

## Goals / Non-Goals

**Goals:** Replace the product, not add a permanent second implementation;
preserve project and operator workflows; have one daemon state owner; keep both
distribution channels equally functional; simplify by using focused libraries
and Node standard APIs; record proof at each end-to-end gate.

**Non-Goals:** Rust/Node interoperability, old-data import, Bun, custom WASM,
native runtime addons, Web/Tauri surfaces, new system installers, history, IIS,
autostart, machine output, profiles, export, watch, continuous tail, rewriting
unrelated tools or publishing during implementation. No fixture is a substitute
for the actual application or real Windows privilege acceptance.

## Decisions

### Module ownership and contracts

| Module | Owns | Boundary and tests |
| --- | --- | --- |
| protocol | Zod request/result/error schemas, protocol 3, HMAC framing | Pure schema/authentication tests; no storage or process imports |
| daemon | Runtime exclusion, registries, mutation queue, snapshots, RPC handlers | Fake Caddy/platform/storage ports plus real local IPC tests |
| caddy adapter | Trusted executable resolution, adapt/validate, composition, mTLS admin, owned child | Fake process/admin fixtures, followed by real Caddy integration |
| platform adapters | Filesystem permissions, Windows ACL, Unix owner and operator socket/PID inspection | Small OS-specific adapters; fake policies then native acceptance |
| storage | One application-state/diagnostic-log SQLite worker and separate lifetime lock connection | Transaction, recovery, retention and close tests |
| client service | Typed RPC, shared models, selectors and lifecycle coordination | Fake transport/model tests; no database or Caddy admin access |
| CLI | Commander grammar, formatting, attach-only policy, exit status | Consumer argument/stream/output tests |
| TUI | Ink/React rendering and user interaction through the client service | ink-testing-library fixtures and real terminal smoke |
| packaging | Staging manifest, tsc npm emission, esbuild/SEA, archives/checksums | Packed-consumer verification; no application policy duplicated here |

Freeze method names, typed payloads, error categories, command arguments and
exit codes in a behavior/contract fixture catalog before adding product handlers.
Retain current status, daemon, projects, domains, port, Caddyfile, diagnostics,
bounded logs and TUI commands from released Rust 1.0.5. Preserve bare help on
TTY and non-TTY, explicit cadder tui --start-daemon, human-readable output and
unsupported-command rejection. The eight-operation released IPC catalog remains
the product boundary. Historical history/IIS/autostart and machine-output code
is superseded, not compatibility authority. Unknown or ambiguous current behavior
requires a recorded decision before implementation rather than an undocumented
choice.

Zod validates all method payloads and results. Responses correlate request IDs
and contain exactly one result or typed error. The existing generic runtime
fixture schema must be tightened into the product catalog before it is treated
as a completed RPC contract. Authentication, transport, unsupported protocol,
config/conflict and storage failures remain distinct.

### Source and dependency policy

Track TS/TSX, tests and tool configuration. Keep ESM; do not confuse it with a
second JS application. `tsc --noEmit` checks types; Vitest/V8 measures own source
lines; `tsx` is development-only for TSX/TUI. Consumer installs need no loader,
compiler, postinstall download or build.

Nub manages the root and documentation dependencies with exact versions and
locks. TypeScript is 6.0.3. Use Commander, Zod, smol-toml, Ink/React and
ink-testing-library for their narrow CLI/schema/config/rendering jobs instead of
custom equivalents. Use @peculiar/x509 with Node WebCrypto for certificate
creation and verification: Node provides crypto/TLS, while the library avoids
hand-written ASN.1/X.509 generation. Use node:sqlite instead of a native addon
or custom durable format; its release-candidate status is accepted. ESLint,
Prettier and Vitest/V8 own their respective checks. Do not add tsup.

The npm runtime is Node 24 LTS, minimum 24.18.0. The SEA builder is a separately
pinned Node 26 patch. Verify and record that patch when the SEA packaging task
is executed; do not guess a patch or change the npm minimum to match it. The
same source tree and runtime schemas feed both builds. No new framework or
general-purpose task runner is needed. Retain current mise/Rust tooling while
the baseline is needed, then remove obsolete responsibilities, not blindly
translate them into custom JS.

### Runtime ownership and security

Use an owner-protected, installation-specific v2 directory separate from the
Rust runtime and a separate SQLite lock database. Keep separate installation
identities and verify independent endpoints/databases; owner-specific protection
must not collapse distinct installations into one runtime. A dedicated lock
connection holds `BEGIN EXCLUSIVE` for the daemon lifetime; it is not the state/log
worker. Diagnostic PID metadata,
timestamps or leases never grant ownership. Acquire the actual lock before
crash-residue recovery, then validate/create protected runtime files, bind the
endpoint and publish readiness. Track resources acquired during startup so any
failure closes only those resources and releases the lock.

The secret comes from a cryptographic generator and stays in owner-protected
storage. Unix uses owner-only modes; Windows validates owner SID and protected
ACLs through the small PowerShell/.NET adapter. Reject unsafe existing paths,
links, owner or ACL rather than weakening/repairing security implicitly. Root on
Unix must receive an explicit owner UID and runtime directory. Same-owner
ordinary clients can authenticate to an elevated daemon; other accounts cannot.
No remote clients or cross-owner access are supported.

NDJSON travels over Unix sockets or Windows named pipes. Enforce bounded frames,
connections and timeouts, exact protocol 3/security policy 2 and mutual HMAC
using fresh direction-bound challenges. Secret bytes never cross IPC. Validate
authenticated frame sequences and reject replay/reflection/tampering or RPC
before authentication. Do not add capability negotiation or Rust compatibility.

The delivered contract/runtime boundaries support Caddy integration implementation
while G1/G2 acceptance evidence remains open. Under the user-approved sequencing,
Windows Sandbox runs on the completed product, not midway through implementation.
G2 must still prove native exclusion/crash recovery and both privilege models
before G7 acceptance, Rust removal or release. The existing Windows Sandbox
guide/package is preparation only; UAC approval, cancellation, same-owner elevated
contact and different-account denial require real isolated execution, not mocked
claims or host policy changes.

### Configuration transactions and Caddy

Preserve `cadder.toml`, Caddyfiles, argument parsing and documented precedence;
classify older executable selectors against the released trusted-source policy
in the fixture catalog. Do not reintroduce a project-controlled Caddy executable
override to emulate obsolete code. Resolve the real executable from trusted
explicit daemon configuration or safe PATH. Detect npm wrappers, SEA entrypoints
and aliases/file identity to prevent recursion. Pin the selected executable for
the daemon lifetime. Keep adaptation bounds and exact active-host/loopback guards
from CADDY-004/005; do not lose master's security fixes during the port.

One queue owns prepare -> adapt/compose -> validate -> apply -> verify -> persist
-> publish. Project/domain activation, registration changes and reload must enter
that ownership boundary. Pure reads may use the last committed snapshot;
they cannot publish a pending state.
Validation/conflict rejection leaves the previous committed model/config intact.
If apply may have succeeded but its result is unknown, fence subsequent changes,
query the active protected Caddy state and reconcile before accepting another
mutation. If persistence fails after apply, restore and verify last-known-good;
a failed restore stays fenced and reports a recoverable diagnostic.

Create protected root/intermediate CA and authorized client material using
WebCrypto/X.509. Caddy's native internal issuer obtains and renews the admin
server certificate from that CA; do not inject a server leaf into CertMagic
storage or add a proxy/custom issuer. Keep Caddy's identity storage inside the
owner-protected runtime boundary. Bind the admin channel to loopback with mTLS,
validate server identity and require the authorized client. Do not install this
CA in OS trust.
Override/reject project admin settings before validation; no plaintext admin
listener may remain. Verify no-cert/wrong-cert denial with real Caddy. Lifecycle
control uses only the daemon's owned child handle: graceful stop, bounded wait,
then force only that child. Never scan for or kill unrelated Caddy processes.

### Shim and platform boundaries

Keep managed/read-only/passthrough/unsupported command policy. Preserve argument
arrays, stdin/stdout/stderr and exit codes; use direct spawning, not shell-built
commands. Managed `caddy run` may start a missing daemon, but never independent
Caddy. Session nonces own registration, heartbeat/reconnect and cleanup on
Ctrl+C, lost connection, clean exit or daemon restart. Stale sessions cannot
detach or mutate a replacement owner.

Platform adapters are limited to runtime permissions/identity and current
operator socket/PID inspection. Same-owner contact with an explicitly elevated
daemon remains a runtime-security requirement, not an IIS feature. Real UAC,
cancellation and other-account denial acceptance runs in Windows Sandbox, never
by silently changing host accounts or policy. No IIS mutation helper, restore
metadata or autostart provider is part of this migration.

Preserve INSPECT-001 through INSPECT-004: operator port/socket inspection and
explicit PID-revalidated termination stay in the operator process, behind small
platform adapters without native runtime addons. This does not grant the daemon
ownership of unrelated processes.

### Storage, logs and operator surfaces

One worker owns the Node SQLite application-state/diagnostic-log connection,
migrations, bounded jobs and orderly close. Validate schema/integrity before
mutations;
rollback transactions on failure, settle pending jobs before teardown and keep
the exclusion connection alive until resources are released. Store stable
intent and bounded diagnostic logs, never resurrect sessions or process ownership
as live. Do not read, migrate, delete or clean up the Rust database/runtime
implicitly.

Redact before persistence or response and retain deterministic bounded streams
(baseline: 1,000 per stream, 5,000 global; queries bounded to 200). Each query
selects one canonical runtime, entrypoint/project or domain stream and returns
the newest requested rows in ascending sequence order. Preserve
actual domain attribution, not presentation-only labels. Do not add all-log,
source/severity, cursor, paging, tail, subscription, history or export filters.
Bounded logs remain explicit CLI diagnostics, not a TUI view. Terminal rendering
neutralizes control characters.

Commander CLI and Ink/React TUI share the client service and models. Production
never uses mock data or random status. Implement current lifecycle, projects,
domains, bounded CLI logs/diagnostics and inspection commands. Preserve
human-readable output and stable error/exit behavior;
reject machine output and the other removed commands. State/inspection commands
are attach-only. Explicit daemon start/restart, managed run and cadder tui
--start-daemon retain their bounded launch paths. Bare cadder prints help without
starting TUI or daemon, on TTY and non-TTY. Ordinary cadder tui opens the routes
workspace with explicit offline start. Preserve project/domain tree rows, separate
daemon/Caddy header state, Space/Enter toggles, offline Enter-to-start, confirmed
stop and ordered restart. No separate activity, logs, history, IIS or autostart
views are added. Screens stay quiet, keyboard-operated and resize/exit safe.

### Packaging and CI

Every Node artifact is generated into system/runner temp outside checkout.
For npm, `tsc` emits JS with relative import-extension rewriting and a staging
manifest exposes the three bins. Pack one tarball with no platform
optionalDependencies, install scripts, generated source-side JS or real Caddy.
Test the actual tarball via local/global installs and PATH fixtures outside
checkout; publication must consume the exact artifact that passed this process.

For SEA, esbuild bundles the same modules/entrypoints and a pinned Node 26 builds
three standalone entries plus required assets. If dispatch/shared assets are
used, prove all three invocation names and owned child/worker launches from the
archive. No spawned Node/npm lookup may be required by standalone runtime. Test
SQLite state/log workers, TUI and child processes on Windows x64, Linux x64 and
both macOS architectures with Node/npm absent from PATH. Write checksums and
retain release provenance/attestation and least-privilege staging safeguards.

CI must reuse non-publishing pack commands for verification, enforce >=85% own
line coverage including hard runtime modules, run Windows/Linux/macOS native
integration, and gate all four SEA variants. Use one functional scenario catalog
for npm and SEA. Astro/Starlight docs move from Bun/legacy workflows to Nub and
describe both channels equally only after the documented flows pass. Artifact
identity includes exact source commit/version; npm and SEA versions match.

## Risks / Trade-offs

- Node SQLite and experimental SEA are accepted dependencies, not permission
  to waive coverage, native worker or distribution parity gates.
- Uncertain Caddy apply needs active-state reconciliation; retries alone are
  unsafe and must not turn a lost response into a second uncoordinated mutation.
- Windows runtime ACL behavior needs real UAC and another account. Fake adapter
  tests cannot close that gate. Missing execution authority blocks that test,
  not permission to weaken policy or mark it complete.
- Removing Rust too early would leave no working product. Keeping it permanently
  would fail the migration. G8 requires both complete Node channels, then proves
  operation from a checkout and consumer environment without Cargo/Rust.

## Migration Plan

G1 establishes contracts/fixtures and tooling. Implemented contract/runtime
boundaries permit G3 Caddy work while remaining G1/G2 evidence stays open. G3
ports and verifies transactional Caddy. G4 ports shim and current inspection/platform
boundaries. G5 completes persistence and real CLI/TUI. G6 builds and verifies both
channels and docs. Complete G1/G2 acceptance, including actual-product Windows
Sandbox, before G7 closes cross-platform/security/parity acceptance. G8 removes
Rust and legacy packaging and reruns the Node-only gates. G9 prepares matching
release candidates without publishing. See task dependencies and evidence in
`acceptance.md`; a checkpoint does not close the whole migration.

Before a user switches, stop the Rust daemon. Re-register projects from existing
files. Leave old runtime data and releases intact. No IIS/autostart transition
steps apply to the released product. Downgrade uses the old matched release set,
with the Node daemon stopped; no automatic old-data import or deprecation occurs.
Commit, push, tag, registry publication and GitHub release need separate authority.

## Open Questions

There is no unresolved choice about language, ESM/compilation, Node minimum,
equal distribution rank, privilege models or the final removal of Rust. Remaining
execution-time discoveries are the exact released 1.0.5 command/DTO fixture catalog,
the verified Node 26 builder patch and native SEA asset/worker layout. Tasks 1.3,
6.3 and 6.4 own these decisions and their evidence. A discovery that changes the
approved public contract or requires a different runtime/dependency policy must
return for a user decision before implementation.
