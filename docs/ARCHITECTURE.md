# Cadder Architecture

Cadder v1.0 is scoped around a small, testable runtime topology:

- `cadderd`: per-user daemon and runtime owner.
- `caddy`: PATH-facing Caddy-compatible shim.
- `cadder`: operator executable with CLI and TUI workflows.

The product contract is limited to the daemon, shim, operator CLI, and TUI. Future Web and Tauri GUI surfaces require a new OpenSpec change and reuse the daemon protocol and shared operator view-model contracts. The specifications in `openspec/specs/` are authoritative when this document and OpenSpec disagree.

## Process Roles

- `cadderd` owns registrations, local IPC, Caddyfile adaptation, effective Caddy config composition, the Cadder-owned real Caddy process, runtime diagnostics, durable history, native autostart state, IIS handoff metadata, and bounded log storage.
- `caddy` intentionally shadows Caddy for managed `caddy run` commands. It attaches to an already running daemon, registers the caller's config, heartbeats while alive, and unregisters on exit. Non-`run` commands are delegated to the safely resolved real Caddy binary.
- `cadder` is the only operator-facing v1 binary. Its CLI supports daemon lifecycle, runtime status, entrypoints, domains, logs, diagnostics, history, IIS handoff, autostart, settings, and watch flows. The current TUI slice covers runtime status, entrypoints, domains, retained logs, and explicit daemon recovery. Both surfaces attach through the daemon protocol and render from mockable operator view models.
- Real Caddy is an external binary. Cadder never embeds Caddy and must not recursively execute its own shim.

All release-facing Cadder binaries expose `--help` and `--version`. Runtime installers and portable archives contain only `cadderd`, `cadder`, `caddy`, and `cadder.toml`.

## Runtime Model

Cadder is portable: each executable resolves the runtime directory as its own parent directory. The release binaries `cadderd`, `caddy`, and `cadder` must stay together, and Cadder does not use the current working directory for runtime state.

The daemon owns:

- a lockfile guarded by `fs4`, preventing multiple daemons for the same runtime directory;
- a local IPC socket name derived from the runtime directory;
- an effective generated Caddy JSON config file;
- ephemeral daemon metadata and bounded in-memory state.

Durable Cadder data lives in `data/` under the portable runtime directory, alongside the daemon coordination files.

Direct `cadderd` execution is the foreground diagnostic path. Explicit client-triggered starts use the detached background launch contract, redirect stdio away from the caller, and wait for the runtime socket before reporting success. Restart flows first request shutdown, then wait for the previous owner to release both the socket and runtime lock before launching the next daemon.

Development workflows can select `CADDER_CADDY_BACKEND=mock` or `--caddy-backend mock`. The mock backend prepares registrations and effective config state without invoking `caddy adapt`, `caddy run`, `reload`, or `stop`, and it never binds HTTP or HTTPS ports.

## Operator Surfaces

The CLI is the stable automation surface for people, scripts, and agents. It supports `human`, `json`, and `jsonl` output modes where appropriate.

The TUI is the interactive operator surface. Its current slice renders daemon and Caddy status, entrypoints, domains, retained logs, and recovery actions from the shared operator view models. It remains useful when `cadderd` is offline by showing daemon-unavailable status and a visible start action. Later OpenSpec changes can add dedicated IIS handoff, autostart, settings, diagnostics, and history views without embedding local state in the TUI.

Web and Tauri GUI are future surfaces. They may return only as clients of the same daemon protocol and shared view-model contracts. Remote pairing, authentication, multi-host management, and update channels are out of scope for the reset.

## IPC Boundary

IPC is versioned newline-delimited JSON over a per-user local socket via `interprocess`. Each message has an envelope:

```json
{ "protocolVersion": 1, "type": "query-state-request", "payload": { "requestId": "..." } }
```

Supported v1.0 public messages include:

- register, unregister, and heartbeat entrypoint;
- query current state;
- subscribe to state changes;
- set entrypoint enabled;
- set domain enabled;
- query Windows IIS bindings;
- set Windows IIS handoff enabled or disabled;
- query Caddy logs;
- query durable history;
- query and set native autostart mode;
- request daemon shutdown.

## Caddy Integration

Real Caddy resolution is layered and recursion-safe. The daemon selects one executable for its lifetime in this order:

1. An absolute `--real-caddy` daemon-start override.
2. `[caddy]` in `cadder.toml` beside the Cadder executables.
3. `[caddy]` or `defaults` in the standard per-user `cadder.toml`.
4. The same configuration in the administrator-owned system `cadder.toml`.
5. A trusted native `caddy` executable on PATH.

The TOML schema is:

```toml
[caddy]
real_command = "caddy-real"
```

Project files, registration working directories, environment selectors, and shim flags never select real Caddy. `real_command` selects one program from PATH; `real_path` selects an absolute regular native executable. Cadder does not impose custom ownership or ACL rules on the Caddy installation or its parent directories. PATH fallback excludes the shim by operating-system file identity, including symlink and hardlink aliases.

At daemon startup, Cadder opens the selected executable and pins its canonical path, source, operating-system file identity, SHA-256 digest, semantic version, required module inventory, and compatibility-probe revision. Cadder accepts Caddy versions from 2.11.3 up to, but not including, 3.0.0. Metadata commands use bounded output, a 30-second deadline, and no stdin. Every subsequent `adapt`, `run`, `reload`, and `stop` process passes through the same verified spawn gate. On Windows, the daemon retains a read-only handle without write or delete sharing for its lifetime. Each child starts suspended, joins Cadder's private kill-on-close Job Object, and resumes only after successful assignment. An identity or digest mismatch fails closed and the newly created process tree is terminated and joined before the operation returns.

For each registration, Cadder runs:

```sh
caddy adapt --config <Caddyfile> --adapter <adapter>
```

The adapted JSON is inspected for HTTP host matchers. Active domains are composed into a generated effective Caddy JSON document. Domain conflicts are reported before runtime reload. When no active domains remain, the daemon enters idle config/runtime state instead of reloading an empty active config.

Runtime operations start the owned real Caddy process with the generated config and reload it on subsequent config changes. Captured stdout/stderr and control events are stored in a bounded log store with redaction for token-like values.

## Durable Runtime Storage

Cadder stores durable profile data in owner-protected files. Versioned JSON documents hold the manifest, snapshots, indexes, and stream metadata. Append-only JSON Lines segments hold state transactions with history and operational logs. Each transaction record has a sequence number, checksum, previous-record hash, and terminating LF. The daemon flushes a complete record before publishing the corresponding state change in memory.

One file-store worker owns `storage.lock` and serializes commits, queries, maintenance, and shutdown. Shutdown closes admission, drains accepted records, flushes active segments, and joins the worker before the daemon removes IPC discovery or releases process ownership. If a durability call exceeds its normal budget, Cadder keeps ownership until the worker finishes.

The store rejects unsupported schemas and invalid complete records. After a crash, it may remove only an incomplete final line. Corrupt authoritative files remain available as owner-protected recovery evidence. Live process handles, subscriptions, connection leases, and daemon-instance ownership stay in memory and are never restored as durable leases.

## Windows IIS Handoff

On Windows, the daemon exposes a small IIS provider behind `query-iis-bindings` and `set-iis-handoff`. The provider is platform-gated: non-Windows builds return an IIS-unavailable issue instead of loading Windows-only dependencies.

Discovery, route planning, restore metadata writes, Caddy config updates, and daemon/operator operation stay in the normal user context. Only IIS binding mutations are classified as administrator steps and are executed as a short privileged batch through the OS elevation prompt.

Supported handoff shapes are IIS `http` port 80 and `https` port 443 bindings when Cadder can identify one route host. Cadder persists restore metadata before mutating IIS, creates deterministic loopback backend bindings, injects a Caddy reverse-proxy route, and supports restore/rollback follow-up actions.

Windows Sandbox remains the preferred smoke boundary for installer, autostart, shim, daemon lifecycle, IIS handoff, and cleanup tests because those checks intentionally touch OS-level state.

## Packaging

`cargo xtask dist --out <dir>` builds the v1.0 portable runtime layout:

- `cadderd`
- `cadder`
- `caddy`
- `cadder.toml`

`cargo xtask package --out <dir> --platform <platform> --target <triple>` wraps that layout in `cadder-<version>-<platform>` and writes a neighboring `.sha256` checksum file.

Native runtime installers are built with `cargo xtask runtime-installer --out <dir> --platform <platform> --target <triple>`. They install only the v1.0 runtime binaries and sample configuration, produce `cadder-runtime-<version>-<platform>` artifacts, and write manifest/checksum files that record expected install paths.

`cargo xtask verify-release-assets` checks the cross-platform runtime installer and portable archive matrix before upload.

Cadder v1.0 has no in-app updater. Update surfaces route users to manual GitHub Releases downloads.

## Workspace Layout

The workspace topology is intentionally closed around documented product,
library, and tooling responsibilities. `cargo xtask verify-workspace-topology`
checks the Cargo workspace against this contract.

| Workspace member | Classification | Responsibility | Release-facing package |
| --- | --- | --- | --- |
| `crates/cadder-daemon` | Daemon | Runtime state, daemon lock, local IPC, Caddy integration, process runtime, durable storage, platform providers, logs, and the `cadderd` binary. | Yes |
| `crates/cadder-shim` | Shim | Package containing the PATH-facing `caddy` binary. | Yes |
| `crates/cadder-client` | Operator client | Package that builds the `cadder` operator executable for CLI and TUI workflows. | Yes |
| `crates/cadder-api` | Client API | Internal client API, daemon launch policy, state shaping, and reusable view-model boundary shared by CLI and TUI code. | No |
| `crates/cadder-ipc` | Shared IPC | Shared DTOs, activation/runtime/log states, IPC envelopes, and request/response contracts. | No |
| `xtask` | Docs/tooling | Repository validation task runner for checks that are Cadder-specific. | No |

`crates/cadder-api` remains a separate internal library because it
keeps daemon access and view-model construction mockable across CLI and TUI
tests. It may be merged into `crates/cadder-client` only through a future OpenSpec
change if that boundary stops carrying a testable responsibility.

Historical product crates such as `crates/cadderctl`, `crates/cadder-tui`, and
`crates/cadder-mcp` are obsolete in this topology. Future Web, Tauri, MCP, or
remote-management surfaces require their own OpenSpec change and must reuse the
daemon protocol and shared operator view-model contracts rather than creating a
second runtime control plane.

## Validation

Use Cargo from the repository root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask check
cargo xtask coverage
```

The Docker/Testcontainers end-to-end suite is intentionally separate from the default Cargo test path because it requires a running Docker daemon and the Docker CLI:

```sh
cargo build -p cadder-daemon -p cadder-shim
cargo test -p cadder-daemon --features docker-e2e --test testcontainers_e2e -- --ignored --test-threads=1
```

Focused tests are appropriate while iterating. Full validation should pass before release closeout.
