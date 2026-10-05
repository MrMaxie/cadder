## MODIFIED Requirements

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
