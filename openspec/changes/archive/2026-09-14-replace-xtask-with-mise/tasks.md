## 1. Baseline and ownership map

- [x] 1.1 Confirm that `simplify-v1-foundation` has resolved its IPC contract mismatch and that its accepted tooling changes are reconciled before modifying the shared `xtask`, Cargo, or workflow surfaces
- [x] 1.2 Inventory every current `xtask` command and record its maintained-tool owner, focused product-test owner, or explicit removal rationale; review the inventory against QT-002, QT-003, and QT-005
- [x] 1.3 Capture the accepted four-platform release plan, archive names, exact archive contents, version behavior, and checksum behavior without publishing or changing any release asset

## 2. Cargo-dist compatibility gate

- [x] 2.1 Select and pin a maintained `cargo-dist` version, record its license and supported configuration surface, and add the smallest config-only package definition for the existing three release binaries
- [x] 2.2 Configure one direct Cargo build command for the three existing binary packages without adding a wrapper crate or general-purpose build script
- [x] 2.3 Produce and inspect a non-publishing local release plan and archive, verifying the accepted binary names, root layout, license, sample configuration, version identity, and SHA-256 checksum
- [x] 2.4 Run non-publishing artifact builds on Windows x64, Linux x64, macOS x64, and macOS arm64 and compare the complete matrix with the baseline
- [x] 2.5 Record the compatibility decision; if cargo-dist requires substantial custom glue, stop this change before cleanup and request the follow-up OpenSpec package-model decision required by QT-005

## 3. Declarative project environment

- [x] 3.1 Select exact maintained versions for Rust and required components, Bun, Node/OpenSpec, `cargo-llvm-cov`, and `cargo-dist`, then add committed `mise.toml` and `mise.lock`
- [x] 3.2 Add small discoverable tasks for format, lint, test, strict OpenSpec validation, documentation check/build, coverage, complete check, and non-publishing release verification
- [x] 3.3 Order Cargo-heavy validation deterministically, allow only independent tool families to run concurrently, and verify failure propagation through the complete task
- [x] 3.4 Replace `xtask dev-env` and `dev-run` with a task-scoped non-secret mock-backend environment and verify that ordinary product commands do not inherit it
- [x] 3.5 Verify environment preparation, task discovery, and focused task execution from clean Windows, Linux, and macOS shells without `just`, Nushell, or private local configuration

## 4. Release pipeline migration

- [x] 4.1 Finalize cargo-dist target, archive-format, include-file, checksum, naming, and source-tarball settings while preserving the accepted distribution contract
- [x] 4.2 Generate the cargo-dist GitHub Release workflow and review runner selection, artifact flow, immutable publication behavior, and least-privilege permissions
- [x] 4.3 Add supported cargo-dist hooks or separate focused jobs only for existing platform evidence that cannot be represented by the generated workflow
- [x] 4.4 Verify workflow generation is reproducible and that a configuration drift check uses cargo-dist's supported command rather than a Cadder-owned parser

## 5. Validation ownership migration

- [x] 5.1 Move release-binary help, version, and shim-identity assertions that remain product requirements into focused tests owned by the corresponding executable packages
- [x] 5.2 Replace custom coverage invocation and policy with the direct `cargo-llvm-cov` threshold/report command and verify the configured 85 percent line gate
- [x] 5.3 Replace custom OpenSpec invocation and version checks with the pinned official CLI tasks and verify doctor, implementation-schema, and strict canonical-spec validation
- [x] 5.4 Keep documentation dependency, content, and build validation in Bun/Astro tasks, adding a focused maintained checker only if a concrete accepted documentation invariant remains uncovered
- [x] 5.5 Remove custom workspace-topology, release-profile, hard-coded asset-snippet, and release-matrix checks once their replacement owner or removal rationale is verified against the command inventory

## 6. CI and documentation cutover

- [x] 6.1 Update general GitHub Actions jobs to install the pinned mise environment and invoke the same supported tasks used locally without release credentials
- [x] 6.2 Retain explicit operating-system and Docker jobs only for required platform evidence, and verify they do not define an alternative task implementation
- [x] 6.3 Update contributor, architecture, release-maintainer, pull-request-template, and OpenSpec-template references from `cargo xtask` to `mise run` or the relevant direct owning-tool command
- [x] 6.4 Verify that end-user documentation contains no repository task-runner, mock-backend, `.local`, personal-path, or agent-only instructions
- [x] 6.5 Run the old and replacement complete gates on the same source revision and reconcile every behavioral difference before removing the old surface

## 7. Remove the custom task framework

- [x] 7.1 Remove the `xtask` workspace package, `.cargo/config.toml` alias, command implementation, task-framework tests, and xtask-only dependencies after QT-005 evidence is complete
- [x] 7.2 Remove superseded handwritten release packaging and verification workflow steps after cargo-dist artifact parity is established
- [x] 7.3 Remove stale `cargo xtask`, `just`, and Nushell references from checked-in project files while preserving historical archived OpenSpec evidence
- [x] 7.4 Review the resulting repository for a replacement general-purpose script, duplicated tool-version source, long inline shell program, or leaked local/private information and remove any such regression

## 8. Final verification

- [x] 8.1 Run the pinned mise format, lint, test, strict OpenSpec, documentation, coverage, and complete-check tasks from the repository root
- [x] 8.2 Run the complete non-publishing cargo-dist artifact matrix and verify archive contents, names, versions, SHA-256 checksums, and generated workflow state for the same source revision
- [x] 8.3 Verify the supported contributor setup and task-discovery journey on Windows, Linux, and macOS and record which paths were live-smoked versus automated-only
- [x] 8.4 Review the final diff and repository status for unrelated changes, generated debris, `.local` leakage, private paths, and incomplete accepted tasks before closeout
