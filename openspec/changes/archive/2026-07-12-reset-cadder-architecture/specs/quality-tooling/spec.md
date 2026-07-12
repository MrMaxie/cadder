## ADDED Requirements

### Requirement: OpenSpec is the planning source of truth
The repository SHALL use OpenSpec for product requirements, design decisions,
and implementation planning.

#### Scenario: New architecture work is proposed
- **WHEN** new architecture or product behavior work is proposed
- **THEN** the work SHALL be captured as an OpenSpec change
- **AND** the change SHALL define affected capabilities before implementation

#### Scenario: Project documentation conflicts with OpenSpec
- **WHEN** project documentation conflicts with accepted OpenSpec requirements
- **THEN** OpenSpec SHALL be treated as authoritative
- **AND** the documentation SHALL be updated or explicitly marked stale

### Requirement: Release surface follows accepted specs
Release artifacts, docs, and validation SHALL include only product surfaces
defined by accepted OpenSpec requirements or an active approved change.

#### Scenario: Release artifact list generated
- **WHEN** release metadata is generated
- **THEN** every shipped artifact SHALL map to a documented product surface
- **AND** undocumented artifacts SHALL fail validation or require an explicit OpenSpec change

#### Scenario: Documentation advertises a workflow
- **WHEN** user documentation advertises a workflow
- **THEN** that workflow SHALL map to an accepted requirement or active change
- **AND** stale workflows SHALL be removed or moved to historical notes

### Requirement: Files stay small and cohesive
Production source files SHALL stay focused on one concern and SHOULD remain
under the project file-size threshold unless an explicit exception is documented.

#### Scenario: File exceeds threshold
- **WHEN** a source file exceeds the configured line threshold
- **THEN** the change SHALL either split the file by concern or document a narrow exception
- **AND** validation SHALL report the exception visibly

#### Scenario: Generated file exceeds threshold
- **WHEN** a generated file exceeds the threshold
- **THEN** the file SHALL be marked as generated or excluded by policy
- **AND** reviewers SHALL not treat it as a precedent for hand-written modules

### Requirement: Mature tools are preferred over custom orchestration
The project SHALL prefer proven external tools for testing, coverage, release,
documentation, and command orchestration when they satisfy Cadder's needs.

#### Scenario: Tooling task duplicates external tool
- **WHEN** a custom tooling feature duplicates a mature external tool
- **THEN** the change SHALL either replace it or justify why Cadder-specific logic is required
- **AND** the resulting implementation SHALL remain small and reviewable

#### Scenario: Dependency proposed
- **WHEN** a new dependency is proposed for tooling or runtime code
- **THEN** the task SHALL state the job it performs, why it is preferable to custom code, and how it will be tested

### Requirement: Test strategy covers independent seams
Cadder SHALL test domain logic, external adapters, protocol contracts, client
rendering, platform providers, and log filtering independently before relying on
system smoke tests.

#### Scenario: Runtime logic tested without external process
- **WHEN** runtime registration, config composition, or lifecycle logic is tested
- **THEN** the test SHALL use fake adapters unless the test is explicitly marked as integration or smoke coverage
- **AND** failure paths SHALL be covered with typed errors

#### Scenario: Client rendering tested without daemon
- **WHEN** client screens or output modes are tested
- **THEN** tests SHALL use mock view models or fake client responses
- **AND** no real IPC SHALL be required

#### Scenario: System behavior needs OS integration
- **WHEN** a behavior depends on OS services, process installation, path resolution, or platform-specific permissions
- **THEN** a system smoke test SHALL cover that behavior
- **AND** unit tests SHALL still cover the decision logic separately

### Requirement: Coverage and CI are explicit
The project SHALL keep automated coverage and CI expectations visible in
OpenSpec and project documentation.

#### Scenario: Full validation runs in CI
- **WHEN** CI validates Cadder
- **THEN** it SHALL run the formatting, linting, test, documentation, release metadata, and coverage checks required by the current specs
- **AND** failures SHALL identify which project contract was violated where practical

#### Scenario: Local verification is skipped
- **WHEN** a change cannot run part of the expected verification locally
- **THEN** the final change notes SHALL state what was skipped and why
