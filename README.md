![Cadder banner](assets/banner.webp)

# Cadder

Cadder coordinates local Caddy projects that would otherwise compete for the same HTTP and HTTPS listeners. Projects keep the familiar `caddy run` workflow while one per-installation `cadderd` daemon owns the real Caddy process and combines active routes.

Published documentation: <https://maxie.dev/cadder/>

## Runtime

Cadder 1.0 consists of three version-matched executables:

- `cadderd` owns local IPC, registrations, SQLite state, redacted logs, effective Caddy configuration, and the real Caddy child process.
- `caddy` is the PATH-facing shim. `caddy run` starts or attaches to `cadderd`, registers the current project, sends heartbeats, and unregisters on exit.
- `cadder` opens the keyboard-operated TUI for Status, Domains, and Logs, including explicit Start, Stop, and Restart actions.

Keep all three executables together. Configure the trusted real Caddy source in `cadder.toml` beside them:

```toml
[caddy]
real_path = "C:/Tools/caddy/caddy.exe"
```

Use `real_command` instead when a separately named real Caddy executable is available on PATH. Configure exactly one source.

## Quick use

1. Download the matching `cadder` archive and SHA-256 file for your platform from [GitHub Releases](https://github.com/MrMaxie/cadder/releases).
2. Extract its `cadder`, `cadderd`, and `caddy` executables into one user-owned directory.
3. Copy `cadder.toml.example` to `cadder.toml` and configure real Caddy.
4. Put the directory on PATH, run `caddy run` in each project, then open `cadder tui`.

Managed `caddy run` starts the matching daemon automatically when it is not already running. Mixed Cadder versions fail the exact protocol handshake before changing runtime state.

## Development

Install [mise](https://mise.jdx.dev/), then use the pinned project environment:

```sh
mise install --locked
mise tasks
mise run check
mise run tui-web
```

`mise run tui-web` launches the TUI through the exact `ttyglass` npm package declared by the project. `just tui-web` is the short convenience entrypoint for the same task.

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License

Cadder is licensed under the Apache License 2.0. See [LICENSE](LICENSE).
