## Why

Cadder's approved migration replaces the Rust product with TypeScript/Node to
reduce language-boundary and implementation overhead. Preserve the existing
project Caddyfile -> managed shim -> daemon -> operator journey, with the current
released Rust 1.0.5 command scope and routes-first TUI, while delivering the same
product through npm and standalone downloads.

The delivered foundation supplies authenticated, exclusively owned Node runtime
boundaries. The next capability selects trusted real Caddy and adapts/validates an
existing project Caddyfile with bounded child cleanup. The user approved testing
Windows Sandbox on the completed product; its acceptance remains mandatory before
G7, Rust removal or release, not before Caddy integration implementation.

## What Changes

- **BREAKING** Replace the Rust application, Cargo workspace and native npm
  launcher packages with one TypeScript product after parity and release gates.
- npm requires Node 24 LTS >=24.18.0; GitHub Releases provides equally required
  Node SEA standalone archives without an installed Node/npm prerequisite.
- Keep the three entrypoints, project configuration, Caddyfiles, shim policy,
  owner/elevated access models and installation isolation. Preserve the current
  CLI/TUI scope and human-readable output through one client service, including
  bare help and explicit cadder tui --start-daemon; do not restore removed features.
- **BREAKING** Isolate runtime v2 and protocol 3/security policy 2. Use mutual
  HMAC over local NDJSON and a lifetime SQLite exclusion lock. Do not support
  Rust/Node IPC or import old runtime data.
- Apply Caddy updates transactionally through an owned process and a protected
  loopback mTLS admin channel, with last-known-good and ambiguity reconciliation.
- Build only into outside-checkout staging. Both channels must pass the same
  consumer scenarios before Rust or its existing release path is removed.

## Capabilities

### New Capabilities

- `caddy-shim-integration`: explicit command policy, transport/exit fidelity and
  npm/SEA-aware recursion prevention (SHIM-001 through SHIM-003).

### Modified Capabilities

- `product-topology`: one Node product and daemon-owned state (TOP-001/002).
- `daemon-lifecycle`: SQLite ownership, managed launch, bounded teardown and
  explicit owner/elevated access (RUN-001/002/003 and RUN-005).
- `local-control-plane`: mutual authentication and the retained eight-operation
  RPC catalog without Rust compatibility (IPC-001/002/003).
- `caddy-runtime`: verified transactions and protected admin transport while
  retaining trusted resolution, local routes and bounded adaptation
  (CADDY-001/003/006; CADDY-002/004/005 remain required).
- `project-registration`: live session ownership and preserved project formats
  (REG-001/004; REG-002/003 remain required).
- `runtime-storage`: Node SQLite state/log worker, schema validation and no
  old-data migration or cleanup (STO-001 through STO-004).
- `operator-cli`: released command, human-output and invocation parity (CLI-001).
- `operator-tui`: real shared-model workflows and explicit offline start
  (TUI-001/003; TUI-002/004 remain required).
- `distribution-and-upgrades`: single npm tarball, four equal SEA downloads and
  explicit transition (DIST-001 through DIST-005).
- `quality-tooling`: Nub/TS tooling, native gates, release security and verified
  Rust removal (QT-001 through QT-007; conditional QT-008 remains required).
- `documentation-experience`: verified equal installation paths and shared
  contributor workflow (DOC-001/002/004; DOC-003 remains required).

## Impact

The implementation touches protocol, daemon, Caddy adapter, platform adapters,
client service, CLI/TUI, packaging, CI and Astro/Starlight documentation. It
removes Cargo, Rust crates, legacy native platform packages and old release
orchestration only at the final cutover. Existing 1.0.5 releases, runtime and
database remain available for rollback.

No history, IIS, autostart, machine-output, profiles, export, watch, continuous
tail, expanded log filters or separate TUI activity/log views are included.
No Bun, custom WASM, native runtime addons, Web UI, Tauri, new system installers
or unrelated feature expansion is included. OBS-001 through OBS-004 remain
unchanged; older historical code is evidence, not authority to restore features.
Port/Caddyfile inspection and guarded operator process control retain INSPECT-001
through INSPECT-004. The reviewed contract deltas are synchronized into main
specs without archiving this open roadmap. That accepts the target, not migration
completion. Released versions, commits, pushes, tags and publication remain
separate actions.
