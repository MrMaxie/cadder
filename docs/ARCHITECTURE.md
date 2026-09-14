# Cadder Architecture

OpenSpec defines accepted behavior. This document describes the implemented 1.0 boundaries.

## Process topology

- `cadderd` is the single writer for one installation directory. It owns the local IPC listener, SQLite connection, active registrations, effective Caddy configuration, and the real Caddy child process.
- `caddy` is the PATH-facing shim. A managed `caddy run` attaches to the daemon or starts the matching `cadderd`, registers one entrypoint, renews its lease, and unregisters when the shim exits.
- `cadder` is the operator CLI and TUI. Both read bounded daemon snapshots and send explicit activation mutations through the shared client API.

Separate installation directories derive separate runtime identities. There is no public profile selector.

## IPC

Local IPC uses owner-authenticated `interprocess` sockets or Windows named pipes with bounded newline-delimited JSON frames. Every connection begins with an exact protocol-version handshake. Mixed versions are rejected before operation dispatch.

After the handshake, every request has one typed envelope containing `protocolVersion`, `operation`, `requestId`, and `payload`. Every response repeats the version, operation, and request ID and contains exactly one typed `result` or `error`. The closed operation set is:

1. register entrypoint
2. unregister entrypoint
3. heartbeat entrypoint
4. query state
5. set entrypoint activation
6. set domain activation
7. query bounded logs
8. shutdown daemon

There is no capability negotiation, paging, subscription, history, export, tail, watch, or autostart protocol.

## State and transactions

`data/cadder.sqlite3` is the only durable application store. One `tokio-rusqlite` connection serializes database work. SQLite uses the bundled engine, rollback journaling, full synchronous commits, foreign keys, a busy timeout, and startup integrity validation.

Desired entrypoint and domain activation is persisted only after Caddy accepts the candidate configuration. Publication to shared in-memory state follows the database commit. If persistence fails after Caddy apply, Cadder rolls the owned runtime back to the previous verified configuration; a failed rollback moves mutations into a safe read-only drain.

Leases, process ownership, and active routes are never restored as live after restart. Redacted log events are retained at 1,000 rows per stream and 5,000 rows globally. Queries return at most the newest 200 matching rows in ascending sequence order.

## Caddy ownership

The daemon resolves real Caddy only from an explicit foreground override, trusted configuration, or safe PATH discovery that excludes the shim by file identity. It pins the resolved executable for its lifetime. Project files and shim arguments cannot select a different real Caddy binary.

Only the Caddy child started by this daemon is controlled. `cadderd` never enumerates or terminates unrelated Caddy processes.

Explicit `cadder port` commands are a separate operator boundary. The client uses `netstat2` to inspect local listening sockets and `sysinfo` to read or signal the expected process. A kill requires a caller-supplied PID and revalidates that the PID still owns the port. This does not expand daemon ownership.

## CLI

The Clap command tree exposes daemon lifecycle, status, project, domain, port, Caddyfile, diagnostics, and bounded log workflows. `comfy-table` renders compact terminal tables. One client-side correlation model joins daemon registrations to syntactically local upstream ports, while the daemon remains the source of truth for registration, activation, Caddy runtime, applied configuration, and redacted log state.

## TUI

The Ratatui application separates pure UI state from async effects. Crossterm events and background results are coordinated with `tokio::select!`; owned effects are tracked and drained. Rendering uses Ratatui layout, table, paragraph, scrollbar, style, and test backend APIs rather than terminal-size assumptions or custom ANSI positioning.

The routes workspace accepts arbitrary collection lengths. The header reports `cadderd` and Caddy separately with consistent running and not-running states. Loopback upstreams hide redundant host text. Project paths dim the prefix and emphasize the repository-relative suffix, or only the final component when no repository boundary is found.

## Tooling and releases

`mise.toml` and `mise.lock` define the repository environment and tasks. Maintained tools own their own policies: Cargo and Clippy for Rust, cargo-llvm-cov for coverage, OpenSpec for specifications, Bun/Astro for documentation, and cargo-dist for release archives and checksums.

The cargo-dist workflow is generated at `.github/workflows/release.yml`. Windows x64, Linux x64, macOS x64, and macOS arm64 are release targets. A configuration-only root package lets cargo-dist produce one platform archive containing all three version-matched executables, the README, changelog, license, and sample configuration plus a SHA-256 checksum. Runtime code remains in its owning workspace crates.
