## Context

Cadder currently exposes sixteen repository commands through a Rust `xtask`. They combine four different responsibilities: invoking external tools, defining development environment values, validating repository conventions, and building or inspecting release archives. GitHub Actions duplicates part of the tool installation and command sequence. The nearly completed `simplify-v1-foundation` change removed native installers and custom OpenSpec/LCOV parsing, but intentionally retained `xtask` and custom portable packaging.

The accepted product journey remains unchanged: a user downloads one archive for a supported platform and receives the version-matched `cadder`, `cadderd`, and `caddy` executables, the Apache-2.0 license, and a sample configuration. The contributor journey remains one discoverable command for the complete repository gate. This design changes only the development and release implementation behind those journeys.

Information boundaries are:

- Product information: supported binaries, archive contents, versions, platforms, and user documentation.
- Operational information: CI jobs, release permissions, artifact publication, and tool versions.
- Diagnostic information: coverage reports, validation output, and release plans or manifests.
- Local information: private paths, `.local`, credentials, mock-only overrides, and workstation configuration. Local information is not committed or published.

Implementation starts from the reconciled result of `simplify-v1-foundation`. It does not modify runtime ownership, IPC, storage, CLI/TUI behavior, or Caddy integration.

## Goals / Non-Goals

**Goals:**

- Provide one cross-platform, declarative contributor interface for environment setup and repository tasks.
- Let each maintained tool own its command model, file formats, and validation policy.
- Replace custom archive, checksum, and release-workflow code with maintained release tooling.
- Keep local and CI commands aligned while retaining explicit platform-specific evidence.
- Delete `xtask` and its task-framework tests after every retained responsibility has an accepted owner.
- Preserve the current release artifact contract and public documentation boundary.

**Non-Goals:**

- Add `just`, Nushell, Make, another script language, or a replacement Rust task runner.
- Change shipped commands, runtime behavior, archive contents, supported targets, or release permissions.
- Add native installers, signing, an updater, or package-manager distribution.
- Hide complex behavior inside long inline TOML, YAML, shell, or PowerShell scripts.
- Turn `mise` into an application runtime dependency or expose it in end-user documentation.

## Decisions

### `mise` is the only project environment and task interface

The repository will commit `mise.toml` and `mise.lock`. Exact versions are selected during implementation and remain pinned until an intentional tooling update. `mise run <task>` is the stable form used in scripts and contributor documentation. Direct shorthand such as `mise <task>` is not documented because future built-in command names can shadow tasks.

The alternatives were evaluated against the current cross-platform Rust and documentation workspace:

| Option | Local fit | Low cost | Low risk | Maintainability | Correctness | Maturity | Net value |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Cargo aliases and handwritten CI only | 3 | 4 | 3 | 2 | 3 | 5 | 3 |
| `just` plus separate tool installation | 4 | 4 | 4 | 4 | 4 | 5 | 4 |
| `mise` for tools, environment, and tasks | 5 | 4 | 4 | 5 | 4 | 4 | 5 |
| Retain the Rust `xtask` framework | 5 | 5 | 3 | 2 | 5 | 5 | 3 |

`just` is a focused command runner, but pairing it with `mise` would create two task namespaces and two configuration sources without adding a missing capability. A Rust runner has strong types but requires custom code and tests for ordinary process orchestration. `mise` provides the needed integrated environment and task graph while keeping task definitions declarative.

### Tool ownership is explicit

`mise` owns tool acquisition, exact tool versions, task discovery, task dependencies, working directories, and task-scoped non-secret environment values. It does not parse Cargo metadata, OpenSpec artifacts, coverage reports, documentation assets, or release archives.

The owning tools are:

| Responsibility | Owner |
| --- | --- |
| Rust toolchain and components | `mise` tool declaration, invoking `rustup` semantics |
| Formatting, linting, compilation, and tests | Cargo and rustup components |
| Coverage calculation and threshold | `cargo-llvm-cov` |
| Specification health and strict validation | official OpenSpec CLI |
| Documentation dependencies, checking, and build | Bun and Astro |
| Release planning, builds, archives, included files, checksums, and GitHub Release workflow | `cargo-dist` |
| Platform runner selection, permissions, secrets, and immutable publication gate | GitHub Actions |
| Executable behavior such as help, version, and shim identity | tests owned by the executable packages |

Tasks use direct argument arrays or short cross-platform commands where supported. They must not embed substantial Bash, PowerShell, or command-interpreter programs. Independent documentation and specification checks may run concurrently; Cargo commands that contend for the same target directory run in a deterministic sequence.

### Development environment values are scoped and non-secret

The current mock-backend development value moves from `xtask dev-env` and `dev-run` to a task-scoped `mise` environment. It is not activated globally for ordinary product commands. Secrets, signing credentials, tokens, personal paths, and `.local` locations remain outside committed configuration.

The first task set is intentionally small: task discovery, format, lint, unit/integration tests that do not require a real local service, strict OpenSpec validation, documentation check/build, coverage, the complete `check` gate, and release plan/build verification. New aliases require a repeated contributor need and must invoke an owning tool rather than implement policy.

### `cargo-dist` owns the portable release pipeline

`cargo-dist` will own archive construction, SHA-256 generation, included license/configuration files, target naming, release manifests, and generated GitHub Release workflow. Its configuration is committed and its generated workflow is checked for drift using the tool's supported generation command rather than a repository parser.

The current workspace builds the three release binaries from separate Cargo packages, while `cargo-dist` normally treats separate binary packages as separate applications. The first implementation slice is therefore a bounded compatibility gate using a config-only `cargo-dist` package with three declared binaries and one direct Cargo build command. This route is preferred because it preserves existing package ownership and adds no product wrapper crate.

The compatibility gate must demonstrate on the supported runner matrix that the configuration produces one application archive per target with the accepted name, exact file set, root layout, executable names, version identity, and checksum. If the generic package route cannot meet that contract without a substantial custom build script, implementation stops for an explicit OpenSpec decision. It must not silently introduce a distribution facade crate or retain custom packaging under a new name.

### Repository-specific checks move to the owning boundary or disappear

Checks that duplicate declared Cargo workspace members, release profile values, hard-coded documentation snippets, or the expected list of generated release assets are removed when the source tool already rejects invalid input or produces a machine-readable plan.

A Cadder-specific invariant remains automated only when all of the following are true:

1. It protects accepted product behavior rather than repository style.
2. No selected tool already owns the invariant.
3. It can live as a focused test beside the product code or artifact it validates.
4. Its failure message identifies the violated contract.

Executable help/version and shim-role behavior therefore belongs in executable integration tests. Documentation references belong to the documentation build or a focused maintained checker if a demonstrated gap remains. Release contents and checksums belong to `cargo-dist` plan/build verification. A new general repository-verifier module is prohibited by this design.

### CI installs the same pinned environment and retains narrow permissions

General validation jobs install the pinned `mise` environment and invoke the same named tasks used locally. Platform matrices remain explicit for Rust and runtime integration evidence. Release jobs use the generated `cargo-dist` workflow and only the publication stage receives content-write permission. General tasks receive no release secrets.

External setup actions and tool versions are pinned according to repository policy. Generated workflow changes are reviewed as generated artifacts and regenerated through `cargo-dist`; hand-maintained additions use supported cargo-dist hooks or separate narrow workflows instead of editing generated internals.

## Risks / Trade-offs

- [Risk] Contributors must install `mise` before using the canonical task interface. -> Document one short bootstrap path and keep direct Cargo/Bun/OpenSpec commands understandable for diagnosis.
- [Risk] `mise` becomes a second source for versions already present in manifests or workflows. -> Make `mise.toml` and its lock the development-tool source, remove duplicated setup versions where possible, and leave application dependency versions in their native manifests.
- [Risk] A broad `check` DAG introduces nondeterminism or Cargo target-directory contention. -> Run build-heavy Rust gates sequentially and parallelize only independent tool families.
- [Risk] The config-only multi-binary cargo-dist model is insufficient or changes while experimental. -> Prove the complete artifact matrix before deleting any release path; stop for a new decision instead of adding custom glue.
- [Risk] Removing custom validators drops a real invariant. -> Map every current command to an owning tool, focused test, explicit removal rationale, or accepted deferral before deleting `xtask`.
- [Risk] Generated release workflow changes permissions or publication timing. -> Review the generated permission boundary, run non-publishing plans and pull-request artifact builds, and keep immutable release publication gated by the existing contract.
- [Risk] Running old and new task surfaces during migration creates drift. -> Keep the overlap temporary, compare their outcomes on the same revision, then remove `xtask` and update all documentation in one cleanup slice.

## Migration Plan

1. Complete and reconcile `simplify-v1-foundation`; record a clean baseline for the current repository and portable artifact contract.
2. Add pinned `mise` configuration and the minimal atomic task set while retaining `xtask` as a temporary comparison oracle.
3. Run the cargo-dist compatibility gate for one local platform and then the four supported CI runners without publishing a release.
4. Configure cargo-dist release metadata, included files, SHA-256 checksums, and generated GitHub workflow; verify artifact parity against the accepted specification.
5. Move executable behavior checks to their owning packages and remove duplicated repository-policy checks one responsibility at a time.
6. Switch CI and contributor documentation to `mise run` tasks and cargo-dist commands, comparing the old and new complete gates on the same source revision.
7. Remove the `xtask` package, `.cargo` alias, xtask-only dependencies, tests, documentation, and workflow calls.
8. Run the full cross-platform repository, documentation, and non-publishing release verification, then review the final tree for a replacement task framework or leaked local information.

Rollback before publication is a source revert to the last accepted `xtask` workflow. Published release assets remain immutable and are not replaced during migration. The old path is removed only after the new path has equivalent accepted evidence.

## Open Questions

- Can the pinned cargo-dist version model the existing three-package binary set as one config-only application on all four targets? Task 2 resolves this before the release migration proceeds.
- If it cannot, should Cadder restructure executable packages into one distribution package or retain a narrowly scoped release adapter? That decision requires a follow-up OpenSpec amendment and explicit approval; it is not delegated to implementation.
