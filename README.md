![Cadder banner](assets/banner.webp)

# Cadder

Cadder coordinates local Caddy reverse proxies that would otherwise fight for the same HTTP and HTTPS ports. Projects can keep using a `caddy run` workflow, while Cadder registers them with one per-user daemon and applies active configs through one real Caddy process.

Published documentation: <https://maxie.dev/cadder/>

## What's Included

Cadder v1.0 ships three runtime-facing binaries:

- `cadderd`: the per-user daemon. It owns local IPC, entrypoint registrations, adapted Caddy config composition, the generated effective runtime config, the real Caddy process it starts, diagnostics, durable history, and bounded log storage.
- `caddy`: the Caddy-compatible shim. For `caddy run`, it attaches to `cadderd`, registers the current project's Caddyfile, keeps that registration alive while the shim process runs, and unregisters on exit. Other Caddy commands are delegated to the safely resolved real Caddy binary.
- `cadder`: the operator executable. It provides CLI and TUI workflows for scripts, agents, and operators.

Native runtime installers are named `cadder-runtime-<version>-<platform>`. Portable runtime archives are named `cadder-<version>-<platform>`. Both contain only `cadderd`, `cadder`, `caddy`, and `cadder.toml`.

The target v1 surface is intentionally limited to the runtime daemon, PATH shim, operator CLI, and TUI. Web and Tauri GUI surfaces are deferred until they can reuse the same daemon protocol and view-model contracts.

OpenSpec is the planning source of truth for architecture, requirements, design decisions, and implementation tasks. Project documentation follows accepted OpenSpec specs or active changes.

## Quick Use

1. Download the runtime installer or portable archive for your OS from [GitHub Releases](https://github.com/MrMaxie/cadder/releases).
2. Create `cadder.toml` next to Cadder with the path to the real Caddy binary.
3. Start `cadderd`, then run a project through Cadder's `caddy` shim.
4. Use `cadder` for automation-friendly state, log, diagnostics, IIS, autostart, daemon lifecycle, and TUI workflows.

## Commands

```sh
cadderd
cadder daemon status
cadder entrypoints list --output json
cadder domains disable app.localhost --registration shim-1
cadder logs show --limit 20 domain app.localhost --registration shim-1 --output json
cadder history show --limit 20 --output json
cadder autostart status
cadder tui
caddy run
```

`caddy run` requires a running Cadder backend. Start `cadderd` directly or run `cadder daemon start` before retrying the shim command.

For a local checkout, run the project checks with:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask check
```

The repository defines `cargo xtask` in `.cargo/config.toml`; run `cargo xtask --help` to list validation, docs, release, verification, and dev commands.

Use the isolated dev runtime profile for CLI checks with:

```sh
cargo xtask dev-env --format powershell
cargo xtask dev-run -- cadder daemon status
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for repository workflow, security reporting, and architecture notes.

## License

Cadder is licensed under the terms in [LICENSE](LICENSE).
