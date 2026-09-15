## Why

Cadder's contributor and release workflows are split between a custom Rust `xtask`, duplicated GitHub Actions setup, and direct tool commands. The custom layer owns orchestration, repository policy, archive creation, checksums, and release verification even where maintained tools already provide those capabilities, which makes routine tooling changes larger and harder to trust than the product code they support.

This change preserves the existing contributor journey from a fresh checkout to a complete local validation and the existing release journey from a version tag to four platform archives on GitHub Releases. The smallest end-to-end capability delivered next is one declarative, cross-platform project environment and task interface backed by purpose-built tools rather than a repository-specific task framework.

## What Changes

- Add `mise.toml` and `mise.lock` as the supported source of project tool versions, task definitions, task dependencies, and development-only environment values.
- **BREAKING**: Replace the contributor-facing `cargo xtask` command surface with documented `mise run` tasks and direct native tool commands.
- Remove the `xtask` workspace package, Cargo alias, command dispatcher, archive/checksum implementation, repository-policy verifiers, and tests that exist only to test that custom task framework after each responsibility has an accepted replacement.
- Use Cargo for Rust formatting, linting, tests, and builds; `cargo-llvm-cov` for coverage policy; the official OpenSpec CLI for specification validation; and Bun/Astro for documentation validation.
- Use `cargo-dist` to build and publish the existing portable release application, including its four supported targets, three version-matched binaries, sample configuration, Apache-2.0 license, SHA-256 checksums, and GitHub Release workflow.
- Move any Cadder-specific executable behavior checks to the owning crate's tests. Remove repository checks that merely restate Cargo manifests, generated release metadata, documentation configuration, or OpenSpec declarations.
- Make GitHub Actions install the pinned environment and invoke the same supported tasks used locally, while retaining platform-specific jobs only where operating-system evidence is required.
- Update contributor and release-maintainer documentation to use the new task surface without exposing development tooling in end-user documentation.
- Do not add `just`, Nushell, another general-purpose script language, a replacement task-runner crate, runtime product behavior, or a new public Cadder command.

Success means a contributor can install the declared environment, discover tasks, run the complete repository gate, build documentation, and produce a local release plan without compiling repository-specific orchestration code. A release tag must still produce the accepted portable artifact matrix from one source revision.

## Capabilities

### New Capabilities

- `quality-tooling`: Defines the declarative project environment, supported contributor task surface, ownership boundaries between mature tools, CI parity, and the prohibition on rebuilding a general-purpose repository task framework.

### Modified Capabilities

- `documentation-experience`: Replaces the contributor-facing `cargo xtask` documentation allowance with the supported `mise run` and direct native-tool workflow while preserving the boundary between public product guidance and repository operations.

## Impact

The change affects `xtask/`, the root Cargo workspace and Cargo alias, GitHub Actions workflows, contributor and architecture documentation, release configuration, and the development lockfile. It adds `mise` and `cargo-dist` as pinned development tools but does not add them to the shipped Cadder runtime or archives.

Implementation is sequenced after `simplify-v1-foundation` resolves its remaining IPC contract mismatch and its accepted tooling/runtime changes are reconciled. This change must not absorb or reopen the runtime, storage, CLI/TUI, or IPC scope of the other active changes.
