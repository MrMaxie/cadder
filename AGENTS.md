# Cadder Agent Guide

## Project

Cadder is a cross-platform Rust Caddy coordinator. It provides a daemon (`cadderd`), a PATH-facing `caddy` shim, and the `cadder` operator executable for CLI and TUI workflows. Future Web and Tauri GUI surfaces must attach through the same daemon protocol and view-model contracts. Treat `openspec/` as the source of truth for target behavior, process boundaries, task scope, and planned work.

## Local Workspace Rules

- If `.local` exists, keep `.local` listed in `.git/info/exclude`; do not add it to `.gitignore` unless explicitly requested.
- Keep `.agents` content local-only unless a task explicitly makes it part of the repository contract.
- Read this file, applicable nested `AGENTS.md` files, and relevant `.local` workflow files before editing.
- Use OpenSpec change artifacts for architecture, requirements, design, and implementation plans.
- Do not reintroduce Backlog.md, Backlog task folders, or MCP workflow surfaces unless the user explicitly requests a new OpenSpec change for that reversal.
- Keep project-facing text, source comments, docs, commits, and task notes in English.
- Keep chat with the user in Polish unless they ask otherwise.

## Repo Layout

- `crates/cadder-protocol`: shared DTOs, IPC envelopes, and request/response contracts.
- `crates/cadder-daemon`: daemon state, local IPC, lockfiles, Caddy integration, runtime process management, and log storage.
- `crates/cadderd`: daemon binary.
- `crates/cadder-shim`: package that builds the PATH-facing `caddy` shim binary.
- `crates/cadder`: package that builds the `cadder` operator executable for CLI and TUI workflows.
- `openspec/`: canonical requirements, design, and task planning.
- `xtask`: current validation task runner; prefer shrinking it or replacing custom logic with mature tools as OpenSpec changes require.
- `docs/ARCHITECTURE.md`: architecture notes that must follow OpenSpec, not override it.

## Build And Validation

Use Cargo from the repository root.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask check
```

- `cargo xtask` is the current task-runner entrypoint from `.cargo/config.toml`; use `cargo xtask --help` to discover existing validation, docs, release, verification, and dev commands until OpenSpec-driven tooling changes replace or shrink it.
- Run focused tests after narrow edits, then run the full relevant validation before closeout or commit.
- Use `cargo add`, `cargo remove`, or another Cargo command for dependency changes; do not hand-edit dependency entries.
- Keep automated tests independent of a locally installed real Caddy unless explicitly marked ignored. Prefer fake Caddy fixtures for lifecycle/config/runtime tests.
- Application have to be well tested (at least 85% coverage) and well documented (docs/ with Astro + Starlight theme, that allows to create nice documentation pages).
- Application have to be building on CI/CD pipeline (GitHub Actions) and release artifacts have to be published to GitHub Releases.
- Documentation have to be written in English, and be regenerated and published via Github Actions as well.
- `cargo xtask check` validates the daemon, shim, operator CLI/TUI, docs, release metadata, and asset contract.

## Engineering Notes

- Cadder is cross-platform by default. OS-specific code must sit behind a small abstraction and keep Windows, Linux, and macOS behavior explicit.
- Runtime state is per user and rooted in `directories::ProjectDirs`, with `CADDER_RUNTIME_DIR` available for tests and custom deployments.
- IPC is versioned newline-delimited JSON over a per-user local socket via `interprocess`.
- The `caddy` shim must never recursively execute itself when Cadder needs real Caddy. Real Caddy comes only from an explicit daemon override, trusted per-user or system configuration, or a safe PATH that excludes the shim by file identity.
- The daemon owns only the real Caddy process it starts. It must not enumerate or kill unrelated Caddy processes.
- Normal operation must work at user privilege. Elevated behavior is limited to explicit IIS handoff or an explicitly elevated daemon, and user-level clients must retain controlled access to that daemon through documented local IPC policy.

## Skill Routing

- Use `$rust-pro` for Rust production code, async, process management, ownership-heavy design, contracts, and runtime boundaries.
- Use OpenSpec workflows for intake, execution planning, review, and closeout.
- Use the `cadder` CLI for attach-first Cadder shell automation, explicit daemon lifecycle control, logs tail, watches, diagnostics, TUI launch, and exact CLI recovery flows.
- Use `$commit-work` when staging or committing changes.
- Use `$agents-md-maintainer` when updating agent instructions.
- Use `$are-you-sure` after making any changes to code, for performing fresh-eyes self-review.
- Use `$caddy` as general-purpose reference for Caddy-specific knowledge needed in business layer of this code.
- Use `$code-simplifier` after making any changes to code, to simplify the code for readability or performance.
- Use `$handoff` when you are asked about handoff to other agents.
- Use `$humanizer` when you are creating or updating user-facing text.
- Use `$rust-async-patterns` for Rust production code in this application that uses async patterns.
- Use `$rust-best-practices` for Rust production code in this application that follows best practices.
- Use `$rust-profiling` for Rust production code in this application to make application more performant.

## Commit Rules

- Use Conventional Commits in English, without scoped prefixes unless project instructions change.
- Review staged content with `git diff --cached` before committing.
- Keep unrelated task records or user-created files out of commits unless the user explicitly says they are intentional and should be committed.
