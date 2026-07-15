![Cadder banner](assets/banner.webp)

# Cadder

Cadder coordinates local Caddy reverse proxies that would otherwise fight for the same HTTP and HTTPS ports. Projects can keep using a `caddy run` workflow, while Cadder registers them with one per-user daemon and applies active configs through one real Caddy process.

Published documentation: <https://maxie.dev/cadder/>

## What's Included

Cadder v1.0 ships three runtime-facing binaries:

- `cadderd`: the per-user daemon. It owns local IPC, entrypoint registrations, adapted Caddy config composition, the generated effective runtime config, the real Caddy process it starts, diagnostics, durable history, and bounded log storage.
- `cadder-caddy`: the Caddy-compatible shim. `cadder setup shim` creates the optional `caddy` alias after checking for collisions. For `caddy run`, the shim attaches to `cadderd`, registers the current project's Caddyfile, keeps that registration alive while the shim process runs, and unregisters on exit. Read-only Caddy commands use the trusted real-Caddy resolver; unsupported mutations fail closed.
- `cadder`: the operator executable. It provides CLI and TUI workflows for scripts, agents, and operators.

Native runtime installers are named `cadder-runtime-<version>-<platform>`. Portable runtime archives are named `cadder-<version>-<platform>`. Both contain `cadderd`, `cadder`, `cadder-caddy`, checksums, and a `cadder.toml` configuration template.

The 1.0 surface contains the runtime daemon, optional PATH alias, operator CLI, and TUI. Web and Tauri GUI surfaces remain outside 1.0 and attach through the same daemon protocol and view-model contracts in later releases.

OpenSpec is the planning source of truth for architecture, requirements, design decisions, and implementation tasks. Project documentation follows accepted OpenSpec specs or active changes.

## Quick Use

1. Download the runtime installer or portable archive for your OS from [GitHub Releases](https://github.com/MrMaxie/cadder/releases).
2. Copy the `cadder.toml` template to the standard per-user Cadder configuration directory and set an absolute `defaults.real_caddy` path, or place a trusted real `caddy` on `PATH`.
3. Run `cadder setup shim`, start `cadderd`, then run a project through the optional `caddy` alias.
4. Use `cadder` for automation-friendly state, log, diagnostics, autostart, daemon lifecycle, and TUI workflows.

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
