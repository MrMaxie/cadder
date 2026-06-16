# Cadder Architecture

Cadder is a cross-platform Rust daemon, shim, and terminal UI for routing project-local `caddy run` invocations into one persistent per-user Caddy runtime.

## Process Roles

- `cadderd`: per-user daemon. It owns registrations, local IPC, Caddyfile adaptation, effective Caddy config composition, the Cadder-owned real Caddy process, runtime diagnostics, and bounded log storage.
- `caddy`: PATH-facing shim. It intentionally shadows Caddy for managed `caddy run` commands, attaches to an already running `cadderd`, registers the caller's config, heartbeats while alive, and unregisters on exit.
- `cadder-tui`: Ratatui/Crossterm UI. It attaches to the daemon and shows overview, entrypoints, grouped domains, per-domain logs, diagnostics, search/filtering, activation toggles, and explicit backend start or shutdown actions.
- Real Caddy runtime: resolved external binary. Cadder never embeds Caddy and must not recursively execute its own shim.

All three Cadder binaries expose `--help` and `--version` through their Clap command definitions. The release-facing command names are `cadderd`, `caddy`, and `cadder-tui`.

## Cross-Platform Runtime Model

Cadder uses per-user runtime paths from `directories::ProjectDirs`, with `CADDER_RUNTIME_DIR` as an override for tests and custom deployments. The daemon owns:

- a lockfile guarded by `fs4`, preventing multiple daemons for the same runtime directory;
- a local IPC socket name derived from the runtime directory;
- an effective generated Caddy JSON config file;
- daemon metadata and bounded in-memory state.

`cadderd` is a manually started per-user backend. The `caddy` shim and `cadder-tui` are attach-only clients: they connect to an existing daemon when it is available, surface explicit unavailable guidance when it is not, and never spawn it as an implicit side effect of opening a dashboard or running `caddy run`. Explicit backend start is limited to direct `cadderd` execution or deliberate client actions such as pressing `s` in `cadder-tui`. Explicit launch attempts still use a per-runtime launch lock before spawning `cadderd`, so concurrent start requests wait for the daemon socket instead of spawning another child, while the daemon's own runtime lock remains the final ownership guard. OS services are intentionally deferred; v1 remains a user daemon model.

## IPC Boundary

IPC is versioned newline-delimited JSON over a per-user local socket via `interprocess`. Each message has an envelope:

```json
{ "protocolVersion": 1, "type": "query-state-request", "payload": { "requestId": "..." } }
```

Supported public messages include:

- register, unregister, and heartbeat entrypoint;
- query current state;
- subscribe to state changes;
- set entrypoint enabled;
- set domain enabled;
- query Windows IIS bindings;
- set Windows IIS handoff enabled or disabled;
- query Caddy logs;
- request daemon shutdown.

Shim registrations are tied to the IPC session that created them. If the shim process exits without an unregister request, pipe disconnect cleanup removes only registrations owned by that session.

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

Cadder no longer treats `caddy-real` as a built-in default. Users may still configure `caddy-real` explicitly through CLI, TOML, or environment variables. PATH fallback excludes the current executable and the known shim path from `CADDER_CADDY_SHIM_PATH`, so Cadder does not resolve its own shim as real Caddy.

For each registration, Cadder runs:

```sh
caddy adapt --config <Caddyfile> --adapter <adapter>
```

The adapted JSON is inspected for HTTP host matchers. The adapter resolves real Caddy against the registration's source working directory, so project-local `cadder.toml` files are honored for shim-driven `caddy run` registrations from arbitrary directories. Active domains are composed into a generated effective Caddy JSON document. Domain conflicts are reported before runtime reload. When no active domains remain, the daemon enters idle config/runtime state instead of reloading an empty active config.

Runtime operations start the owned real Caddy process with the generated config and reload it on subsequent config changes. Captured stdout/stderr and control events are stored in a bounded log store with redaction for token-like values.

## Windows IIS Handoff

On Windows, the daemon exposes a small IIS provider behind the `query-iis-bindings` and `set-iis-handoff` IPC messages. The provider is platform-gated: non-Windows builds do not expose the TUI view, and the daemon provider returns an IIS-unavailable issue instead of loading Windows-only dependencies.

IIS discovery is explicit and separate from `query-state`, so the TUI's periodic state refresh does not enumerate IIS bindings while holding daemon state. The Windows provider uses PowerShell `WebAdministration` commands to list, add, remove, and restore bindings. Discovery, route planning, restore metadata writes, Caddy config updates, and daemon/TUI operation stay in the normal user context. Only IIS binding mutations are classified as administrator steps. On Windows those mutations are executed as a short privileged batch through the OS elevation prompt; on non-Windows the same path returns typed unsupported/elevation-unavailable issues. Default automated tests use a fake provider and do not require a local IIS installation or elevated privileges.

Supported handoff shapes are IIS `http` port 80 and `https` port 443 bindings when Cadder can identify one route host. A concrete IIS host header is used directly. A wildcard or empty IIS host header requires an explicit route host from the caller; `cadder-tui` uses the `/` input for this, and full URLs are accepted by extracting only the DNS host. Unsupported protocols, unsupported ports, duplicate host bindings, active Cadder-domain conflicts, missing HTTPS certificate metadata, and provider privilege errors are returned as typed issues and shown inline by `cadder-tui`.

Cadder remains the front door during IIS handoff. Before removing an IIS-owned public binding, the daemon persists restore metadata in `daemon.json` under the runtime directory. The record includes the original site name, protocol, IP address, port, host header, binding information, route host, and loopback backend binding. Enabling HTTP handoff creates a deterministic `127.0.0.1:<port>` HTTP binding on the same IIS site, using a port below the default Windows dynamic TCP range. Enabling HTTPS handoff creates a deterministic `127.0.0.1:<port>` HTTPS binding on the same IIS site and copies the discovered IIS TLS certificate metadata to that backend binding. If the certificate metadata is unavailable, Cadder rejects the handoff before writing metadata or mutating IIS.

Caddy routes HTTPS IIS backends with an HTTP transport that enables TLS. The route host is used as the upstream TLS server name. Caddy skips certificate verification for the loopback backend because IIS certificates commonly do not validate for `127.0.0.1`. This keeps IIS applications seeing an HTTPS backend request while limiting the trust tradeoff to the local loopback hop.

After planning the backend binding, Cadder removes the original public binding, injects a Caddy reverse-proxy route for the route host, and applies Caddy. The loopback binding creation and public binding removal are batched together because they are adjacent IIS mutations for the same requested handoff; Caddy route application, registration state, discovery, and metadata writes are not included in the privileged batch. Persisted IIS proxy routes are hydrated when the daemon starts so restart does not drop an active handoff route. If Cadder cannot apply the proxy route after changing IIS, the daemon attempts a privileged rollback batch that restores the original IIS binding and removes the loopback binding, then reports whether rollback succeeded or failed. If rollback fails, restore metadata and the loopback backend binding are kept so the operator can retry recovery. Restoring handoff removes the Caddy IIS proxy route in the user context, then runs one privileged batch to recreate the original IIS binding and remove the backend binding. Metadata is cleared only after restore succeeds. Restore is rejected while other active Cadder routes still need the front-door port, because IIS would reclaim raw `:80` or `:443`.

`set-iis-handoff-response` includes typed operation steps, privilege level, approval outcome, step status, issue data, and available follow-up actions such as retry elevation, rollback handoff, retry restore, loopback cleanup, or metadata cleanup. User denial of the administrator prompt is a normal typed outcome. It leaves daemon, TUI, registration, log, and non-IIS operations usable and returns retry guidance instead of silently escalating or failing the daemon.

## Domain Logs TUI

`cadder-tui` exposes per-domain logs from the Domains view. Pressing `Enter` or `l` on a domain row opens a log-focused view bound to that domain's `LogStreamIdentity`; the view keeps the selected stream even if registrations change later, so it never silently falls back to an entrypoint or unrelated domain.

The TUI attempts to attach without spawning the daemon, upgrades successful attaches into a durable `subscribe-state-request` stream, and keeps periodic `query-state-request` probes only for late daemon availability and recovery after backend loss. It also loads a bounded log page through `query-logs-request`, stores the returned cursor, and tails by issuing follow-up requests with that cursor. Auto-tail is enabled by default and can be paused with `p`; while paused, the TUI keeps keyboard handling responsive and avoids automatic log refreshes until the user resumes or manually refreshes with `Enter`. Log severity is primarily controlled from the Settings view; applying a new severity resets the cursor before reloading so entries from different filters are not mixed.

On Windows, `cadder-tui` also exposes an IIS Handoff view. The view lists site, protocol, IP address, port, host header, handoff state, and safety details. Pressing `Enter` refreshes IIS discovery; pressing `Space` hands off an available binding or restores a handed-off binding. Before dispatching a mixed-elevation action, the TUI states why administrator approval may be requested. After the daemon responds, the status line summarizes succeeded, approved, denied, failed, and follow-up operation steps. For wildcard or empty-host IIS rows, press `/`, type the route host or URL, press `Enter` to keep the value, then press `Space`. The view is absent on non-Windows platforms rather than shown as a disabled placeholder.

Log refresh, late-availability probes, IIS discovery, activation toggles, IIS handoff actions, and shutdown requests are dispatched as short background IPC tasks. The TUI accepts at most one active state probe, one active state subscription stream, and one active log refresh for the current stream, and surfaces IPC failures as a read-error state rather than blocking terminal input.

When the daemon is unavailable, the Overview view shows the daemon state as not running, connection failed, starting, or start failed. Press `s` to explicitly start `cadderd` with the TUI's runtime directory, daemon path, and real Caddy command options. Press `r` to retry attach or refresh without spawning a daemon. Daemon-dependent actions such as activation toggles, log refreshes, IIS discovery/handoff, and shutdown are gated until a valid state snapshot marks the daemon connected; navigation, retry, explicit start, settings, existing log export, and quit remain responsive. `ShutdownDaemonRequest` stops the owned real Caddy runtime and then terminates the actual `cadderd` process after it replies to the caller.

The log store reports stream status and retention metadata in `query-logs-response`, including active, empty, stale, removed, read-error, gap, more-before, and truncated-by-retention states. Diagnostic exports are timestamped text files in the caller's current working directory and contain only the daemon-redacted `LogEntry.raw_message` content plus stream metadata.

## Portable Packaging

`cargo run -p xtask -- dist --out <dir>` builds release binaries and copies the current platform's executable names into a portable layout:

- `cadderd`
- `cadder-tui`
- `caddy`
- `cadder.toml`

On Windows the binaries include the `.exe` suffix. `cargo run -p xtask -- verify-dist --dir <dir>` checks the expected files and runs `caddy --cadder-shim-info` from the layout. `cargo run -p xtask -- package --out <dir> --platform <platform> --target <triple>` builds the target-specific layout, wraps it in a versioned portable archive, and writes a neighboring `.sha256` checksum file. The package version defaults to the root `Cargo.toml` `[workspace.package]` version, while `--version <version>` remains available for explicit dry-run overrides. Windows artifacts are `.zip` archives; Linux and macOS artifacts are `.tar.gz` archives.

The packaging workflow does not modify PATH, shell profiles, package-manager shims, OS services, or other system state.

Image assets under `assets/` are documentation and release artwork only. They are not copied into the runtime layout and are not required by the daemon, shim, TUI, or verification commands.

The current runtime model is a single per-user daemon that owns the backend and serves zero-to-many independent dashboards or shim clients. Future UI surfaces, including a desktop `cadder` application, should attach to that same backend instead of changing ownership semantics.

## Workspace Layout

- `crates/cadder-protocol`: shared DTOs, activation/runtime/log states, IPC envelopes, and request/response contracts.
- `crates/cadder-daemon`: runtime paths, daemon lock, local IPC server/client, registration state, Caddy integration, process runtime, and log store.
- `crates/cadderd`: daemon binary.
- `crates/cadder-shim`: package containing the PATH-facing `caddy` binary.
- `crates/cadder-tui`: terminal UI.
- `xtask`: repository validation task runner.

## Validation

Use Cargo from the repository root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p xtask -- check
```

Focused tests are appropriate while iterating. Full validation should pass before closeout.

The Docker/Testcontainers end-to-end suite is intentionally separate from the default Cargo test
path because it requires a running Docker daemon and the Docker CLI. It runs compiled host Cadder
binaries while real Caddy runs in an official disposable Caddy container:

```sh
cargo build -p cadderd -p cadder-shim
cargo test -p cadder-daemon --features docker-e2e --test testcontainers_e2e -- --ignored --test-threads=1
```

The suite uses a unique `CADDER_RUNTIME_DIR`, Testcontainers-managed container lifecycle, dynamic
host port mappings, and a wrapper command that delegates Caddy operations into the container. It
must not require a machine-global Caddy installation.

Cadder uses `cargo-llvm-cov 0.8.7` as the canonical Rust workspace coverage tool. On Windows, the canonical gate uses `stable-x86_64-pc-windows-msvc` because GNU coverage can fail without the profiler runtime. The executable gate is:

```sh
cargo run -p xtask -- coverage
```

`xtask coverage` runs `cargo +stable-x86_64-pc-windows-msvc llvm-cov --workspace --json --summary-only --fail-under-lines 85 --output-path target/llvm-cov/coverage-summary.json` on Windows and `cargo llvm-cov --workspace --json --summary-only --fail-under-lines 85 --output-path target/llvm-cov/coverage-summary.json` elsewhere. The command fails when total line coverage is below 85% or when coverage cannot be measured. It does not require a machine-global real Caddy installation; tests use repository fixtures.

No project-specific coverage exclusions are currently configured. Future exclusions must be limited to generated, platform-gated, or intentionally untestable code and documented next to the `xtask coverage` command definition.
