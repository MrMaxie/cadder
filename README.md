![Cadder banner](assets/banner.webp)

# Cadder

Cadder coordinates local Caddy reverse proxies that would otherwise fight for the same HTTP and HTTPS ports. Projects can keep using a `caddy run` workflow, while Cadder registers them with one per-user daemon and applies the active configs through one real Caddy process.

Published documentation: <https://maxie.dev/Cadder/>

## What's included

Cadder ships four executables:

- `caddy` is the Caddy-compatible shim. For `caddy run`, it attaches to a running `cadderd`, registers the current project's Caddyfile, keeps that registration alive while the shim process runs, and unregisters on exit. If the backend is unavailable, Cadder fails with guidance instead of starting it implicitly. Other Caddy commands are delegated to the safely resolved real Caddy binary.
- `cadderd` is the per-user daemon. It owns local IPC, entrypoint registrations, adapted Caddy config composition, the generated effective runtime config, the real Caddy process it starts, diagnostics, and bounded log storage.
- `cadderctl` is the non-TUI operator CLI. It attaches to the daemon for one-shot state queries, logs, and toggles, and provides an explicit `daemon start` path for scripting and automation.
- `cadder-tui` is the terminal UI. It attaches to the daemon, shows overview state, entrypoints, domains, per-domain logs, diagnostics, filters, toggles, log export, and daemon shutdown, and exposes an explicit `s` action when the backend needs to be started.

Each executable supports `--help` and `--version`.

## Quick use

1. Download the latest Cadder release for your operating system from [GitHub Releases](https://github.com/MrMaxie/Cadder/releases).
2. Create `cadder.toml` next to Cadder with the path to the real Caddy binary.
3. Start `cadderd`, then run a project through Cadder's `caddy` shim.
4. Use `cadderctl` for automation-friendly state and log queries, or open `cadder-tui` for the interactive dashboard.

Full setup docs:

- [Getting started](docs/site/src/content/docs/quick-start/getting-started.mdx)
- [cadder.toml](docs/site/src/content/docs/user-guide/cadder-toml.mdx)
- [How to use](docs/site/src/content/docs/user-guide/how-to-use.mdx)
- [Real Caddy resolution](docs/site/src/content/docs/reference/real-caddy-resolution.mdx)

## Commands

```sh
cadderd
cadderctl daemon status
caddy run
cadder-tui
```

`caddy run` now requires a running Cadder backend. Start `cadderd` directly or open `cadder-tui` and press `s` before retrying the shim command.

`cadderctl` gives you non-interactive inspection and control commands such as:

```sh
cadderctl daemon status
cadderctl entrypoints list --output json
cadderctl domains disable app.localhost --registration shim-1
cadderctl logs show --limit 20 domain app.localhost --registration shim-1 --output json
```

For a local checkout, run the project checks with:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p xtask -- check
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for repository workflow, security reporting, and architecture notes.

## License

Cadder is licensed under the terms in [LICENSE](LICENSE).
