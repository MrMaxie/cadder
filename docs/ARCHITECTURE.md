# Cadder Architecture

Cadder v1.0 is scoped around a small, testable runtime topology:

- `cadderd`: per-user daemon and runtime owner.
- `caddy`: PATH-facing Caddy-compatible shim.
- `cadder`: operator executable with CLI and TUI workflows.

The target product contract is limited to the daemon, shim, operator CLI, and TUI. Future Web and Tauri GUI surfaces require a new OpenSpec change and must reuse the daemon protocol and shared operator view-model contracts. The active OpenSpec change `reset-cadder-architecture` is authoritative when this document and OpenSpec disagree.

## Process Roles

- `cadderd` owns registrations, local IPC, Caddyfile adaptation, effective Caddy config composition, the Cadder-owned real Caddy process, runtime diagnostics, durable history, native autostart state, IIS handoff metadata, and bounded log storage.
- `caddy` intentionally shadows Caddy for managed `caddy run` commands. It attaches to an already running daemon, registers the caller's config, heartbeats while alive, and unregisters on exit. Non-`run` commands are delegated to the safely resolved real Caddy binary.
- `cadder` is the only operator-facing v1 binary. Its CLI and TUI support daemon lifecycle, runtime status, entrypoints, domains, logs, diagnostics, history, IIS handoff, autostart, settings, and watch flows. Both surfaces attach through the daemon protocol and render from mockable operator view models.
- Real Caddy is an external binary. Cadder never embeds Caddy and must not recursively execute its own shim.

All release-facing Cadder binaries expose `--help` and `--version`. Runtime installers and portable archives contain only `cadderd`, `cadder`, `caddy`, and `cadder.toml`.

## Runtime Model

Cadder uses per-user runtime paths from `directories::ProjectDirs`, with `CADDER_RUNTIME_DIR` as the highest-priority override for tests and custom deployments. `CADDER_RUNTIME_PROFILE=dev` selects a repeatable development profile under the same per-user runtime base, using `CADDER_DEV_WORKSPACE` or `CADDER_DEV_ID` to derive an isolated runtime identity.

The daemon owns:

- a lockfile guarded by `fs4`, preventing multiple daemons for the same runtime directory;
- a local IPC socket name derived from the runtime directory;
- an effective generated Caddy JSON config file;
- daemon metadata, durable SQLite runtime storage, and bounded in-memory state.

Direct `cadderd` execution is the foreground diagnostic path. Explicit client-triggered starts use the detached background launch contract, redirect stdio away from the caller, and wait for the runtime socket before reporting success. Restart flows first request shutdown, then wait for the previous owner to release both the socket and runtime lock before launching the next daemon.

Development workflows can select `CADDER_CADDY_BACKEND=mock` or `--caddy-backend mock`. The mock backend prepares registrations and effective config state without invoking `caddy adapt`, `caddy run`, `reload`, or `stop`, and it never binds HTTP or HTTPS ports.

## Operator Surfaces

The CLI is the stable automation surface for people, scripts, and agents. It supports `human`, `json`, and `jsonl` output modes where appropriate.

The TUI is the interactive operator surface. It must render from mockable view models and cover the same daemon status, Caddy status, project/domain state, log access, IIS handoff, autostart, settings, and recovery actions as the CLI. The TUI must remain useful when `cadderd` is offline by showing daemon-unavailable status and a visible start action.

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

Real Caddy resolution is layered and recursion-safe. The effective command is selected in this order:

1. CLI override.
2. `cadder.toml` in the current working directory.
3. `cadder.toml` next to the executable.
4. Environment variables, including `CADDER_CADDY_REAL_COMMAND`.
5. `caddy` on PATH as the final fallback.

The TOML schema is:

```toml
[caddy]
real_command = "/absolute/path/to/caddy"
```

PATH fallback excludes the current executable and the known shim path from `CADDER_CADDY_SHIM_PATH`, so Cadder does not resolve its own shim as real Caddy.

For each registration, Cadder runs:

```sh
caddy adapt --config <Caddyfile> --adapter <adapter>
```

The adapted JSON is inspected for HTTP host matchers. Active domains are composed into a generated effective Caddy JSON document. Domain conflicts are reported before runtime reload. When no active domains remain, the daemon enters idle config/runtime state instead of reloading an empty active config.

Runtime operations start the owned real Caddy process with the generated config and reload it on subsequent config changes. Captured stdout/stderr and control events are stored in a bounded log store with redaction for token-like values.

## Durable Runtime Storage

Cadder persists runtime history in `runtime.sqlite3` under the runtime directory. The daemon owns schema creation and migration for this database. The initial schema stores history records for registration lifecycle events, activation toggles, daemon shutdown, autostart changes, IIS handoff activity, and other runtime events. History retention keeps the most recent 10,000 records and prunes older rows after successful writes.

The durable store complements, but does not replace, in-memory daemon state. Current registrations, active log buffers, process handles, subscriptions, and live Caddy runtime ownership remain in memory. If the database contains a schema version newer than the running daemon supports, Cadder reports storage as unavailable with a diagnostic instead of rewriting the file.

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
| `crates/cadder-daemon` | Daemon | Runtime state, daemon lock, local IPC, Caddy integration, process runtime, durable storage, platform providers, and logs. | No |
| `crates/cadderd` | Daemon | Binary entrypoint for the Cadder daemon. | Yes |
| `crates/cadder-shim` | Shim | Package containing the PATH-facing `caddy` binary. | Yes |
| `crates/cadder` | Operator client | Package that builds the `cadder` operator executable for CLI and TUI workflows. | Yes |
| `crates/cadder-operator` | Operator client | Internal operator service, daemon launch policy, state shaping, and reusable view-model boundary shared by CLI and TUI code. | No |
| `crates/cadder-protocol` | Shared protocol/API | Shared DTOs, activation/runtime/log states, IPC envelopes, and request/response contracts. | No |
| `xtask` | Docs/tooling | Repository validation task runner for checks that are Cadder-specific. | No |

`crates/cadder-operator` remains a separate internal library for now because it
keeps daemon access and view-model construction mockable across CLI and TUI
tests. It may be merged into `crates/cadder` only through a future OpenSpec
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
cargo build -p cadderd -p cadder-shim
cargo test -p cadder-daemon --features docker-e2e --test testcontainers_e2e -- --ignored --test-threads=1
```

Focused tests are appropriate while iterating. Full validation should pass before release closeout.
