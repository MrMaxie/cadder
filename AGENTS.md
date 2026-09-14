# Cadder Agent Guide

## Project

Cadder is a cross-platform Rust Caddy coordinator. It provides a daemon (`cadderd`), a PATH-facing `caddy` shim, and the `cadder` operator executable for CLI and TUI workflows. Future Web and Tauri GUI surfaces must attach through the same daemon protocol and view-model contracts. Treat `openspec/` as the source of truth for target behavior, process boundaries, task scope, and planned work.

## Local Workspace Rules

- If `.local` exists, keep `.local` listed in `.git/info/exclude`; do not add it to `.gitignore` unless explicitly requested.
- Keep `.agents` content local-only unless a task explicitly makes it part of the repository contract.
- Read this file, applicable nested `AGENTS.md` files, and relevant `.local` workflow files before editing.
- Treat review, audit, analysis, `read-only`, and `do not run anything` requests as non-mutating. Only inspect and report: do not edit, format, build, test, launch processes, alter runtime state, stage, or commit until the user separately authorizes implementation.
- Use OpenSpec change artifacts for architecture, requirements, design, and implementation plans. Before accepting or applying a change, name the existing user journey it preserves and the smallest end-to-end capability it delivers next. Defer infrastructure, proof machinery, custom tooling, and hardening that do not unblock that capability. After repeated failures in a newly introduced layer, stop and challenge whether that layer should exist before hardening it further.
- Do not reintroduce Backlog.md, Backlog task folders, or MCP workflow surfaces unless the user explicitly requests a new OpenSpec change for that reversal.
- Keep project-facing text, source comments, docs, commits, and task notes in English.
- Keep chat with the user in Polish unless they ask otherwise.

## Repo Layout

- `crates/cadder-ipc`: shared DTOs, IPC envelopes, and request/response contracts.
- `crates/cadder-daemon`: daemon state, local IPC, lockfiles, Caddy integration, runtime process management, log storage, and the `cadderd` binary.
- `crates/cadder-shim`: package that builds the PATH-facing `caddy` shim binary.
- `crates/cadder-api`: internal client API, daemon launch policy, and reusable view models.
- `crates/cadder-client`: package that builds the `cadder` operator executable for CLI and TUI workflows.
- `openspec/`: canonical requirements, design, and task planning.
- `mise.toml`: pinned development tools and declarative validation tasks.
- `docs/ARCHITECTURE.md`: architecture notes that must follow OpenSpec, not override it.

## Build And Validation

Use Cargo from the repository root.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
mise run check
```

- Use `mise tasks` to discover repository workflows. Maintained tools own validation policy directly.
- Run focused tests after narrow edits, then run the full relevant validation before closeout or commit.
- Before reporting completion, reconcile every explicitly requested or accepted plan item as delivered, deliberately deferred with the user's approval, or blocked. A green validation gate does not make omitted requested work complete.
- Use `cargo add`, `cargo remove`, or another Cargo command for dependency changes; do not hand-edit dependency entries.
- Keep automated tests independent of a locally installed real Caddy unless explicitly marked ignored. Prefer fake Caddy fixtures for lifecycle/config/runtime tests.
- Application have to be well tested (at least 85% coverage) and well documented (docs/ with Astro + Starlight theme, that allows to create nice documentation pages).
- Application have to be building on CI/CD pipeline (GitHub Actions) and release artifacts have to be published to GitHub Releases.
- Documentation have to be written in English, and be regenerated and published via Github Actions as well.
- `mise run check` validates Rust, OpenSpec, documentation, and cargo-dist workflow state.

## Engineering Notes

- Cadder is cross-platform by default. OS-specific code must sit behind a small abstraction and keep Windows, Linux, and macOS behavior explicit.
- Runtime state is per user and rooted in `directories::ProjectDirs`, with `CADDER_RUNTIME_DIR` available for tests and custom deployments.
- IPC is versioned newline-delimited JSON over a per-user local socket via `interprocess`.
- The `caddy` shim must never recursively execute itself when Cadder needs real Caddy. Real Caddy comes only from an explicit daemon override, trusted per-user or system configuration, or a safe PATH that excludes the shim by file identity.
- The daemon owns only the real Caddy process it starts. It must not enumerate or kill unrelated Caddy processes.
- Normal operation must work at user privilege across supported operating systems.

## Skill Routing

- Treat the entries below as routing choices, not a cumulative checklist. Select the smallest non-overlapping set that covers the request. Add another skill or subagent only for a distinct decision or evidenced risk; review findings do not broaden the accepted scope without a new user decision.

- Use `$rust-pro` for Rust production code, async, process management, ownership-heavy design, contracts, and runtime boundaries.
- Use OpenSpec workflows for intake, execution planning, review, and closeout.
- Use `$arcantry:capture-project-work` to record one shared or private thought at its current commitment level.
- Use `$arcantry:promote-todo-to-openspec` only after explicit approval to promote selected todo.txt meaning into accepted OpenSpec intent.
- Use `$arcantry:maintain-release-story` to review change release metadata, plan local SemVer manifests, and render the managed changelog. Keep tagging and publication in Cadder's existing release workflow.
- Shared Arcantry configuration organizes Cadder project work; it does not authorize adding Arcantry to Cadder's runtime, Cargo build, CI, or publication pipeline. Change those surfaces only on a separate explicit user request.
- Use `$arcantry:reconcile-project-sources` to compare shared and private sources without merging or publishing them implicitly.
- Use `cadder tui` for operator workflows. The 1.0 executable has no parallel scripting command surface.
- Use `$commit-work` when staging or committing changes.
- Use `$agents-md-maintainer` when updating agent instructions.
- Use `$are-you-sure` after making any changes to code, for performing fresh-eyes self-review.
- Use `$caddy` as general-purpose reference for Caddy-specific knowledge needed in business layer of this code.
- Use `$code-simplifier` after making any changes to code, to simplify the code for readability or performance.
- Use `$handoff` when you are asked about handoff to other agents.
- Use `$humanizer` when you are creating or updating user-facing text.
- Use `$audience-scope-discipline` for user test, setup, and operational instructions. Verify that the documented public command and product surface actually exist, and keep test-only environment variables, private runtime paths, mock backends, and internal seams out unless the user explicitly asks for diagnostics.
- Use `$rust-async-patterns` for Rust production code in this application that uses async patterns.
- Use `$rust-best-practices` for Rust production code in this application that follows best practices.
- Use `$rust-profiling` for Rust production code in this application to make application more performant.

## Commit Rules

- Use Conventional Commits in English, without scoped prefixes unless project instructions change.
- Review staged content with `git diff --cached` before committing.
- Keep unrelated task records or user-created files out of commits unless the user explicitly says they are intentional and should be committed.

<!-- arcantry:start -->
## Arcantry

Use `arcantry.toml` for shared Arcantry configuration.
Treat configured OpenSpec sources as accepted product and engineering intent.
Use configured todo.txt sources for quick intake and changelog sources for consumer-facing release history.
<!-- arcantry:end -->
