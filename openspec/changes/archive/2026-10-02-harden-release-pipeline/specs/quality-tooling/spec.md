## MODIFIED Requirements

### Requirement: QT-003: Maintained tools own validation and release formats
Cargo SHALL own Rust formatting, linting, builds, and tests; `cargo-llvm-cov` SHALL own coverage calculation and the configured threshold; the official OpenSpec CLI SHALL own specification validation; npm and Astro SHALL own documentation dependency and build checks; and `cargo-dist` SHALL own release planning, portable archives, included release files, SHA-256 checksums, and generated GitHub Release automation.

Cadder MAY apply one deterministic, focused postprocessor to cargo-dist's generated workflow only for security invariants that cargo-dist cannot express. The adapter MUST fail when its expected generated structure changes, MUST be verified by the supported repository task surface, and MUST NOT recreate release planning, artifact construction, checksums, or publication policy.

Cadder-specific executable behavior MUST be verified by tests in the package that owns the executable. Repository checks that only restate a native manifest, generated release plan, or accepted OpenSpec declaration MUST NOT be retained as independent policy engines.

#### Scenario: Complete repository gate
- **WHEN** a contributor or CI runs the complete supported check task
- **THEN** each required validation runs through its owning maintained tool or focused product test
- **AND** a failure identifies the owning validation boundary

#### Scenario: Portable release is planned
- **WHEN** release automation plans a Cadder version
- **THEN** `cargo-dist` describes one portable application for every supported target with the accepted binaries, included files, and SHA-256 checksums
- **AND** the committed workflow equals cargo-dist's generated output after the focused security hardening pass

### Requirement: QT-004: Local and CI task parity
Continuous integration SHALL invoke the same supported task definitions used locally for platform-independent repository validation. Platform-specific jobs MAY add only the runner, target, permissions, or external service evidence required by their operating-system contract.

Release pull requests MUST build and upload the complete cargo-dist artifact plan without publishing a release. Planning and artifact-build jobs MUST have read-only repository permissions and MUST NOT receive a publication token. Release tag values MUST be validated as data outside generated shell source. Release publication MUST remain separate from general validation, MUST use the source revision and artifact plan accepted by the release gate, MUST verify the exact tagged archives on their native operating-system runners before announce, and MUST grant write permission only to the stages that attest or publish immutable release assets.

#### Scenario: Pull request validation
- **WHEN** a pull request runs repository validation
- **THEN** CI installs the pinned project environment and invokes the documented `mise run` tasks
- **AND** the jobs do not receive release publication credentials

#### Scenario: Release candidate pull request
- **WHEN** cargo-dist evaluates a release pull request
- **THEN** it builds and uploads the complete portable artifact set for inspection with read-only repository access
- **AND** it does not create or modify a public GitHub Release

#### Scenario: Platform-specific evidence
- **WHEN** a requirement needs Windows, Linux, macOS, or Docker evidence
- **THEN** CI runs the focused task on the required runner or service
- **AND** the platform job does not introduce an alternative repository task implementation

#### Scenario: Tagged release publication
- **WHEN** a validated supported release tag is built
- **THEN** every target archive is verified on its native runner from a fresh directory outside the checkout
- **AND** GitHub Release publication waits for all verification jobs to succeed and receives write permission only in the publishing path

### Requirement: QT-007: npm publication uses staged trusted publishing
Cadder npm publication SHALL use npm trusted publishing from the exact GitHub-hosted workflow and protected release environment through short-lived OIDC credentials. The trusted publisher MUST allow staged publishing only, package publishing access MUST disallow traditional tokens, and no long-lived npm publish credential may be stored in the repository or GitHub Actions.

The workflow MUST stage packages only after it verifies the corresponding GitHub Release assets, SHA-256 checksums, and attestations. Every accepted asset attestation MUST identify the selected release tag, its exact source commit, and Cadder's release workflow as signer. npm provenance MUST identify the public repository and publishing workflow. A maintainer MUST review and approve each stage with 2FA, approving all platform packages before the root package.

#### Scenario: Automated staging
- **WHEN** a verified tagged release reaches the npm workflow
- **THEN** GitHub Actions checks out that exact tag and verifies every consumed asset against its ref, commit digest, repository, and release workflow
- **AND** it obtains a short-lived OIDC publishing identity and stages rather than directly publishes all version-matched packages
- **AND** no npm access token is available to the job

#### Scenario: Human publication approval
- **WHEN** the staged package set is ready for publication
- **THEN** a maintainer reviews the staged artifacts and approves them with 2FA
- **AND** the root package cannot become public before every referenced platform package
