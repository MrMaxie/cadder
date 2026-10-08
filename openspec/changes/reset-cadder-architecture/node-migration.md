# Cadder 2.0 Node migration

This is the approved scope summary. `design.md` defines module contracts and
decisions, `specs/` contains normative deltas, `tasks.md` is the Node-only roadmap,
and `acceptance.md` maps the approved plan to requirements, gates and evidence.
`release.md` records planned consumer outcomes; `verification.md` records only
executed results. The original Rust design is historical, not the active target.

## Preserved contracts

The daemon owns state, configuration, diagnostic logs and only the Caddy process it
starts. Preserve the current released Rust 1.0.5 scope: Caddyfiles, `cadder.toml`,
command policy, human-readable output, project registration, heartbeat, domain
controls, diagnostics, bounded CLI logs and port/Caddyfile inspection. Preserve
separate installation runtimes and the current eight-operation IPC catalog.
State/inspection CLI commands attach without starting the daemon; explicit daemon
start/restart, managed `caddy run` and `cadder tui --start-daemon` retain their
bounded launch paths. CLI and TUI share a client service. Bare `cadder` prints
help on TTY and non-TTY; `cadder tui` opens the routes-first workspace with explicit
offline start and no separate activity/log views. There are no mock production
statuses. Superseded historical behavior is not the migration target.

## Changed contracts

- One product with protocol, daemon, Caddy, platform, client, CLI and TUI modules.
- Sources stay `.ts`/`.tsx`, ESM; `tsc --noEmit` checks types. Vitest/V8 covers at
  least 85% of own lines including runtime modules. `tsx` is development-only.
- npm is a single `cadder` tarball with three bins, Node 24 LTS minimum 24.18.0.
- GitHub Releases supplies equal standalone SEA archives: Windows x64, Linux
  x64, macOS x64 and arm64. All contain `cadder`, `cadderd`, `caddy`, assets and
  checksums. Both channels have the same version and functionality.
- npm uses `tsc` with relative import extension rewriting; SEA uses esbuild and
  pinned Node 26. All emission and packaging occurs in system/runner temporary
  staging outside checkout. No install-time compilation, binary download,
  platform optional packages, or bundled real Caddy.
- Nub manages exact dependency versions and lockfile, including Astro/Starlight
  docs. Commander, Zod, smol-toml, Ink/React, @peculiar/x509/Node WebCrypto and
  node:sqlite replace custom glue. No tsup.
- Runtime is isolated in `v2`, protocol 3, security policy 2. Unix sockets and
  Windows named pipes carry bounded NDJSON. Owner-protected secret material
  supports mutual HMAC authentication with fresh challenges and direction-bound
  transcripts. Secret bytes never travel over IPC; RPC follows authentication.
- Refuse unsafe permissions/ACLs rather than silently weakening security.
  Unix root startup requires an explicit runtime owner and runtime directory.
  Same-owner user clients can contact an elevated daemon; other users cannot.
- A separate SQLite database holds `BEGIN EXCLUSIVE` for process lifetime.
  Metadata is diagnostic only. Crash recovery starts after acquiring this lock,
  never from PID checks or expiry. Application state and diagnostic logs use one
  dedicated SQLite worker.
- One mutation queue prepares, validates, applies, reconciles and commits config.
  Rejection keeps last-known-good; ambiguous outcomes block further mutations
  until active Caddy state is reconciled.
- Caddy local plaintext admin is disabled. Loopback mTLS uses an internal CA and
  protected client certificate; no system trust-store installation. Project
  configuration cannot override admin policy. Shutdown is graceful then forced
  against the owned process only.

## Deferred and excluded

No history, IIS, autostart, machine-output (JSON/JSONL), profiles, export, watch,
continuous tail, expanded log filters or separate TUI activity/log views. These
are not missing parity tasks; they are outside the current product scope.
No mixed Rust/Node IPC, old-data import, Bun, custom WASM, native runtime addons,
Web UI, Tauri or new system installers. SEA experimental status concerns its
packaging mechanism, not distribution priority.

## Verification and transition gates

Follow implemented module dependencies while tracking open acceptance evidence.
The user approved groups 3-6 implementation against the delivered contract/runtime
boundaries without first executing Windows Sandbox. Sandbox tests the completed
product under tasks 2.8/7.4; G1/G2 acceptance remains mandatory before G7, Rust
removal or release. Deferred execution is not a security waiver or a passed gate.
Cover concurrent starts, crash recovery, authentication failures, replay, fake
endpoints, unsafe ACLs and same-owner elevated access. Also cover multi-project
reload/rollback, heartbeat, Ctrl+C, shutdown, actual-product native privilege
boundaries and identical npm/SEA consumer scenarios. Test SEA without
Node/npm on PATH on each native platform, including the routes TUI, state/log
SQLite worker and subprocesses. Packaging must leave tracked files unchanged
and no generated JS in checkout.

Retain Cargo, Rust code and existing release tooling until functionality and both
distributions pass their gates. Do not delete the pre-existing untracked `npm/`.
Before user transition, stop the Rust daemon. Do not delete old runtime data;
re-register existing projects.
Prepare 2.0.0-rc.1 in both channels, then 2.0.0 after acceptance. Old releases stay
available for rollback. Commit, push, tag and publication require separate
authorization. Checkboxes record progress, not independent implementation proof.
