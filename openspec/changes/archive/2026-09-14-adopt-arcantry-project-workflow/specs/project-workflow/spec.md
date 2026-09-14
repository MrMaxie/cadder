## ADDED Requirements

### Requirement: Project knowledge responsibilities remain explicit

The repository workflow MUST keep quick shared intake in root `todo.txt`, accepted product and engineering intent in OpenSpec, public release history in `CHANGELOG.md`, and private operational context under `.local/`. Shared and private sources MUST remain independent unless an explicit reviewed operation promotes content.

#### Scenario: A shared thought becomes accepted intent

- **WHEN** an agent captures a shared project thought and the user approves its promotion
- **THEN** the thought is recorded in `todo.txt` before promotion
- **AND** accepted intent is recorded through a normal OpenSpec change rather than inferred from the queue

#### Scenario: Private context is used

- **WHEN** an agent needs machine-local guidance or intake
- **THEN** it uses `.local` without creating a private Arcantry configuration that shadows the shared project contract

### Requirement: Release meaning comes from OpenSpec artifacts

The repository workflow MUST derive local release planning and generated public changelog content from archived OpenSpec `release.md` artifacts and release manifests. Git commits and diffs MUST NOT define release prose, category, impact or visibility. Local release operations MUST remain separate from Cadder's existing tag, CI and publication workflow.

#### Scenario: Archived work is planned for release

- **WHEN** Arcantry plans the next local release
- **THEN** it uses unassigned archived change artifacts and their declared SemVer impacts
- **AND** does not commit, tag, push, publish or modify CI

#### Scenario: Internal history is backfilled

- **WHEN** historical archived changes are marked internal
- **THEN** they remain available for audit and SemVer planning
- **AND** their titles and bodies are omitted from the public changelog

### Requirement: Agent guidance routes work by commitment level

Shared and private agent guidance MUST route quick capture, approved promotion, release maintenance and source reconciliation to the corresponding installed Arcantry skills. The private launcher MUST call the locally built Arcantry CLI without adding it to Cadder's runtime or build dependencies.

#### Scenario: An agent uses the project workflow

- **WHEN** an agent needs to capture, promote, reconcile or prepare release meaning
- **THEN** guidance identifies the matching skill and source responsibility
- **AND** preserves explicit user approval boundaries for promotion and external publication
