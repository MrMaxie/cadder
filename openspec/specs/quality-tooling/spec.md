# quality-tooling Specification

## Purpose
Define the maintained contributor toolchain, validation gate, and release automation boundary.
## Requirements
### Requirement: QT-001: Declarative contributor environment
Cadder SHALL define its supported development-tool versions, task entrypoints, task dependencies, working directories, and non-secret development environment values in committed `mise` configuration with a committed lockfile.

The configuration MUST work from a fresh checkout on Windows, Linux, and macOS without requiring a repository-specific compiled task runner. It MUST NOT contain credentials, personal paths, `.local` paths, or values that alter ordinary product behavior outside an explicitly scoped development task.

#### Scenario: Fresh contributor setup
- **WHEN** a contributor installs `mise` and prepares the declared project environment from a fresh checkout
- **THEN** the pinned Rust, documentation, specification, coverage, and release tools become available
- **AND** no `xtask`, `just`, Nushell, or private workstation configuration is required

#### Scenario: Development-only environment
- **WHEN** a contributor runs a task that requires the mock Caddy backend
- **THEN** the mock setting applies only to that task and its children
- **AND** an ordinary Cadder command does not inherit the mock setting from committed global configuration

### Requirement: QT-002: One discoverable task surface
The supported repository task surface SHALL be discoverable through `mise tasks` and SHALL use `mise run <task>` as its stable documented invocation.

Tasks MUST remain small adapters to an owning tool. The repository MUST NOT add a second general command runner, embed substantial shell programs in task configuration, or recreate a command-dispatch framework in Rust or another language.

#### Scenario: Contributor discovers validation commands
- **WHEN** a contributor lists project tasks
- **THEN** the output identifies focused formatting, linting, testing, specification, documentation, coverage, complete-check, and release-verification tasks
- **AND** each task delegates its work to the tool that owns that responsibility

#### Scenario: New automation is proposed
- **WHEN** a proposed task would parse a maintained tool's format or reproduce its policy
- **THEN** the proposal is rejected in favor of that tool's supported command or configuration
- **AND** any claimed exception requires an explicit Cadder product invariant and a focused verification owner

### Requirement: QT-003: Maintained tools own validation and release formats
Cargo SHALL own Rust formatting, linting, builds, and tests; `cargo-llvm-cov` SHALL own coverage calculation and the configured threshold; the official OpenSpec CLI SHALL own specification validation; npm and Astro SHALL own documentation dependency and build checks; and `cargo-dist` SHALL own release planning, portable archives, included release files, SHA-256 checksums, and generated GitHub Release automation.

Cadder-specific executable behavior MUST be verified by tests in the package that owns the executable. Repository checks that only restate a native manifest, generated release plan, or accepted OpenSpec declaration MUST NOT be retained as independent policy engines.

#### Scenario: Complete repository gate
- **WHEN** a contributor or CI runs the complete supported check task
- **THEN** each required validation runs through its owning maintained tool or focused product test
- **AND** a failure identifies the owning validation boundary

#### Scenario: Portable release is planned
- **WHEN** release automation plans a Cadder version
- **THEN** `cargo-dist` describes one portable application for every supported target with the accepted binaries, included files, and SHA-256 checksums
- **AND** no Cadder-owned archive or checksum implementation is invoked

### Requirement: QT-004: Local and CI task parity
Continuous integration SHALL invoke the same supported task definitions used locally for platform-independent repository validation. Platform-specific jobs MAY add only the runner, target, permissions, or external service evidence required by their operating-system contract.

Release pull requests MUST build and upload the complete cargo-dist artifact plan without publishing a release. Release publication MUST remain separate from general validation, MUST use the source revision and artifact plan accepted by the release gate, MUST verify the exact tagged archives on their native operating-system runners before announce, and MUST grant write permission only to the stages that attest or publish immutable release assets.

#### Scenario: Pull request validation
- **WHEN** a pull request runs repository validation
- **THEN** CI installs the pinned project environment and invokes the documented `mise run` tasks
- **AND** the jobs do not receive release publication credentials

#### Scenario: Release candidate pull request
- **WHEN** cargo-dist evaluates a release pull request
- **THEN** it builds and uploads the complete portable artifact set for inspection
- **AND** it does not create or modify a public GitHub Release

#### Scenario: Platform-specific evidence
- **WHEN** a requirement needs Windows, Linux, macOS, or Docker evidence
- **THEN** CI runs the focused task on the required runner or service
- **AND** the platform job does not introduce an alternative repository task implementation

#### Scenario: Tagged release publication
- **WHEN** a supported release tag is built
- **THEN** every target archive is verified on its native runner from a fresh directory outside the checkout
- **AND** GitHub Release publication waits for all verification jobs to succeed

### Requirement: QT-005: Evidence-gated task-runner removal
The `xtask` package and command surface SHALL be removed only after every retained responsibility is mapped to a maintained tool, a focused owning-package test, or an explicit accepted removal rationale.

The replacement release path MUST demonstrate the complete supported artifact matrix without publishing, and the replacement complete-check path MUST pass on the same source revision before the old task framework is deleted.

#### Scenario: Migration inventory is reviewed
- **WHEN** removal of `xtask` is proposed
- **THEN** every existing command has a recorded replacement owner or removal rationale
- **AND** no command is translated into a new general-purpose script solely to preserve the old structure

#### Scenario: Replacement release path is incompatible
- **WHEN** cargo-dist cannot produce the accepted one-application artifact contract without substantial custom glue
- **THEN** implementation stops before deleting the existing release path
- **AND** a follow-up OpenSpec decision chooses a new package model or explicitly scoped adapter

### Requirement: QT-006: npm packages pass clean-room verification

The npm package gate SHALL verify the exact packed file set, platform metadata, exact optional dependency versions, absence of install lifecycle scripts, and matching license and repository identity before any package is staged.

On every supported native runner, the gate MUST install locally packed packages in a fresh consumer directory outside the checkout and MUST exercise `--version` and `--help` for `cadder`, `cadderd`, and the Cadder `caddy` shim. The gate MUST prove that the npm `caddy` launcher cannot resolve itself as upstream Caddy.

#### Scenario: Package candidate is verified

- **WHEN** npm package tarballs are prepared for a release
- **THEN** every supported platform passes native clean-room installation and command checks
- **AND** unexpected files, lifecycle scripts, version drift, or launcher recursion fail the gate before registry staging

### Requirement: QT-007: npm publication uses staged trusted publishing

Cadder npm publication SHALL use npm trusted publishing from the exact GitHub-hosted workflow and protected release environment through short-lived OIDC credentials. The trusted publisher MUST allow staged publishing only, package publishing access MUST disallow traditional tokens, and no long-lived npm publish credential may be stored in the repository or GitHub Actions.

The workflow MUST stage packages only after it verifies the corresponding GitHub Release assets, SHA-256 checksums, and attestations. npm provenance MUST identify the public repository and publishing workflow. A maintainer MUST review and approve each stage with 2FA, approving all platform packages before the root package.

#### Scenario: Automated staging

- **WHEN** a verified tagged release reaches the npm workflow
- **THEN** GitHub Actions obtains a short-lived OIDC publishing identity
- **AND** it stages rather than directly publishes all version-matched packages
- **AND** no npm access token is available to the job

#### Scenario: Human publication approval

- **WHEN** the staged package set is ready for publication
- **THEN** a maintainer reviews the staged artifacts and approves them with 2FA
- **AND** the root package cannot become public before every referenced platform package

### Requirement: QT-008: New npm names use an explicit bootstrap

Before the first trusted release, each new npm package name SHALL be created interactively with account 2FA using a non-executable bootstrap version and an explicit non-default distribution tag. If the registry also assigns its required default tag to the package's only published version, that tag MAY temporarily point to the same non-executable bootstrap. Trusted publisher configuration and token restrictions MUST be applied only after the package names exist and before a real Cadder version is staged. The first verified release MUST replace the temporary default tag before npm installation is documented publicly.

#### Scenario: Package namespace is initialized

- **WHEN** maintainers bootstrap the npm package set
- **THEN** the default tag exposes no executable Cadder application and may point only to the non-executable bootstrap until the first verified release
- **AND** each package can be bound to the exact stage-only GitHub trusted publisher
