# Contributing to Cadder

Cadder is a Rust workspace for a per-user Caddy coordinator. The main parts are:

- `crates/cadder-ipc`: shared request and response contracts.
- `crates/cadder-daemon`: daemon state, IPC, Caddy process ownership, durable history, runtime storage, and the `cadderd` binary.
- `crates/cadder-api`: shared client API used by operator clients.
- `crates/cadder-client`: package that builds the `cadder` operator executable.
- `crates/cadder-shim`: PATH-facing `caddy` shim.
- `xtask`: current validation, coverage, distribution, and packaging tasks; OpenSpec-driven cleanup should shrink or replace custom orchestration with mature tools where practical.

## Development Setup

Use Cargo from the repository root. `.cargo/config.toml` defines the conventional `cargo xtask` alias.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask check
```

Documentation lives in `docs/site` and uses Bun:

```sh
cargo xtask docs-check
cargo xtask docs-build
```

## Local Workspace

`.local` is private operational workspace. Keep it in `.git/info/exclude`, not in `.gitignore`.

Repo-local agent skill verification is not part of the v1.0 product contract. CLI commands remain the stable automation surface for people and agents.

## Planning

OpenSpec is the source of truth for product requirements, design decisions, architecture changes, and implementation plans. New product or architecture work should start as an OpenSpec change, and project documentation should follow accepted specs or active changes.

## Releases

The release workflow publishes only tag events whose `v*` version exactly matches the root `[workspace.package]` version. Manual workflow runs build package artifacts without publishing a GitHub Release.

Release binaries use the workspace root release profile. Verify profile and release identity drift before packaging:

```sh
cargo xtask verify-release-profile
cargo xtask verify-release-identity
```

Build a local release layout:

```sh
cargo xtask dist --out target/cadder-dist
cargo xtask verify-dist --dir target/cadder-dist
```

The v1.0 portable runtime layout contains:

- `cadderd`
- `cadder`
- `caddy`
- `cadder.toml`

Build a versioned portable archive and checksum:

```sh
cargo xtask package --out target/cadder-packages --version 1.0.0 --platform windows-x64 --target x86_64-pc-windows-msvc
```

Build daemon-first native runtime installers:

```sh
cargo xtask runtime-installer --out target/cadder-runtime-installers --version 1.0.0 --platform windows-x64 --target x86_64-pc-windows-msvc
cargo xtask verify-runtime-installer-dist --dir target/cadder-runtime-installers --version 1.0.0 --platform windows-x64
```

Runtime installers are named `cadder-runtime-<version>-<platform>` and include only `cadderd`, `cadder`, `caddy`, and `cadder.toml`. Windows uses WiX, Linux uses `dpkg-deb` and `rpmbuild`, and macOS uses `pkgbuild`.

Before a public release upload, verify the combined artifact set:

```sh
cargo xtask verify-release-assets --dir target/release-assets --version 1.0.0 --mode dry-run
```

`verify-release-assets` checks portable archive contents, runtime installer manifests, checksums, and the complete platform matrix.

## Pull Requests

Keep pull requests focused. Include the reason for the change, the behavior that changed, and the commands you ran.

Automated tests should not depend on a globally installed real Caddy binary. Use repository fixtures or explicitly ignored integration tests when a real runtime is required.

See `docs/verification/release-profile.md` for the measurement protocol and release artifact inspection checklist.
