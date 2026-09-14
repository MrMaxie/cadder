# Contributing

Cadder is a Rust workspace with a versioned IPC contract, daemon implementation, shared client API, operator TUI, and PATH-facing Caddy shim. OpenSpec is the source of truth for accepted behavior and changes.

## Setup

Install [mise](https://mise.jdx.dev/), then prepare the exact repository toolchain:

```sh
mise install --locked
mise tasks
```

The lockfile pins Rust and its formatting, lint, and coverage components, Bun, Node, OpenSpec, cargo-llvm-cov, and cargo-dist.

## Validation

Use focused Cargo tests while editing, then run the shared gate:

```sh
mise run fmt
mise run lint
mise run test
mise run openspec-check
mise run docs-check
mise run check
```

Coverage is owned directly by cargo-llvm-cov:

```sh
mise run coverage
```

The configured line threshold is 85 percent. Docker-backed Caddy tests remain an explicit job because they require a live container runtime.

## Documentation

Documentation uses Astro and Starlight:

```sh
mise run docs-check
mise run docs-build
```

Keep end-user pages free of private paths, test-only environment variables, and repository-internal workflows.

## Local TUI

Use the mock backend for safe local UI work:

```sh
mise run dev
mise run tui-web
```

The mock backend variable is scoped to those tasks and is not exported to ordinary commands.

## Releases

cargo-dist owns release planning, archives, checksums, source tarballs, and the generated GitHub Release workflow:

```sh
mise run dist-plan
mise run dist-build
```

`dist generate --check` verifies that `.github/workflows/release.yml` matches the pinned cargo-dist version. A `v1.0.0` style tag releases all three executable packages in lockstep. Do not publish, tag, or upload from contributor validation.
