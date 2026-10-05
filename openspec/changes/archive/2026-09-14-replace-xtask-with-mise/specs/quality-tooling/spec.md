## ADDED Requirements

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
Cargo SHALL own Rust formatting, linting, builds, and tests; `cargo-llvm-cov` SHALL own coverage calculation and the configured threshold; the official OpenSpec CLI SHALL own specification validation; Bun and Astro SHALL own documentation dependency and build checks; and `cargo-dist` SHALL own release planning, portable archives, included release files, SHA-256 checksums, and generated GitHub Release automation.

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

Release publication MUST remain separate from general validation, MUST use the source revision and artifact plan accepted by the release gate, and MUST grant write permission only to the stage that publishes immutable release assets.

#### Scenario: Pull request validation
- **WHEN** a pull request runs repository validation
- **THEN** CI installs the pinned project environment and invokes the documented `mise run` tasks
- **AND** the jobs do not receive release publication credentials

#### Scenario: Platform-specific evidence
- **WHEN** a requirement needs Windows, Linux, macOS, or Docker evidence
- **THEN** CI runs the focused task on the required runner or service
- **AND** the platform job does not introduce an alternative repository task implementation

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
