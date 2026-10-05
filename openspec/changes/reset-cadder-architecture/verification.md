# Node migration verification

## Scope implemented

The Node implementation currently covers the runtime/IPC foundation, not a
shippable 2.0 application. Product handlers, Caddy, shim, history, CLI/TUI, IIS,
autostart and npm/SEA distributions are still pending. Rust remains intact.

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
