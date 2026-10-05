# Cadder 2.0 Node migration

## Preserved contracts

The daemon owns state, configuration, logs, history and only the Caddy process it
starts. Preserve Caddyfiles, `cadder.toml`, command policy, output modes, project
registration, heartbeat, domain controls, both privilege models, IIS preview and
restore, and explicit autostart. CLI attaches without starting the daemon except
for `daemon start`; managed `caddy run` may start it. CLI and TUI share a client
service. Bare `cadder` opens TUI on a TTY and help otherwise; offline TUI exposes
an explicit start action. There are no mock production statuses.

## Changed contracts

- One product with protocol, daemon, Caddy, platform, client, CLI and TUI modules.
- Sources stay `.ts`/`.tsx`, ESM; `tsc --noEmit` checks types. Vitest/V8 covers at
  least 85% of own lines including runtime modules. `tsx` is development-only.
- npm is a single `cadder` tarball with three bins, Node 24 LTS >=24.18.0 <25.
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
  never from PID checks or expiry. History uses one dedicated SQLite worker.
- One mutation queue prepares, validates, applies, reconciles and commits config.
  Rejection keeps last-known-good; ambiguous outcomes block further mutations
  until active Caddy state is reconciled.
- Caddy local plaintext admin is disabled. Loopback mTLS uses an internal CA and
  protected client certificate; no system trust-store installation. Project
  configuration cannot override admin policy. Shutdown is graceful then forced
  against the owned process only.
- IIS mutation uses a short elevated PowerShell helper for a normal daemon;
  explicitly elevated daemon mode remains supported. Preserve cancellation.

## Deferred and excluded

No mixed Rust/Node IPC, history import, Bun, custom WASM, native runtime addons,
Web UI, Tauri, or new system installers. SEA experimental status concerns its
packaging mechanism, not distribution priority.

## Verification and transition gates

Follow migration tasks in order. Runtime/IPC system tests gate Caddy porting.
Cover concurrent starts, crash recovery, authentication failures, replay, fake
endpoints, unsafe ACLs and same-owner elevated access. Later cover multi-project
reload/rollback, heartbeat, Ctrl+C, shutdown, IIS/UAC/autostart in Windows Sandbox,
and identical npm/SEA consumer scenarios. Test SEA without Node/npm on PATH on
each native platform, including TUI, SQLite workers and subprocesses. Packaging
must leave tracked files unchanged and no generated JS in checkout.

Retain Cargo, Rust code and existing release tooling until functionality and both
distributions pass their gates. Do not delete the pre-existing untracked `npm/`.
Before user transition, restore old IIS handoff, disable old autostart and stop
the Rust daemon. Do not delete old runtime/history; re-register existing projects.
Prepare 2.0.0-rc.1 in both channels, then 2.0.0 after acceptance. Old releases stay
available for rollback. Commit, push, tag and publication require separate
authorization. Checkboxes record progress, not independent implementation proof.
