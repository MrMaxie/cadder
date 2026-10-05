## ADDED Requirements

### Requirement: IIS handoff is optional and explicit
Windows IIS handoff SHALL be disabled unless the user explicitly enables,
previews, or applies it.

#### Scenario: Normal user-level setup
- **WHEN** a user installs or starts Cadder without enabling IIS handoff
- **THEN** Cadder SHALL operate without admin elevation
- **AND** it SHALL NOT mutate IIS bindings

#### Scenario: User requests IIS handoff
- **WHEN** a user asks Cadder to enable IIS handoff
- **THEN** Cadder SHALL show the required changes and privilege requirement before applying them
- **AND** the user SHALL be able to cancel without mutating IIS

### Requirement: IIS operations use a platform provider seam
IIS discovery, handoff, restore, and status checks SHALL be implemented behind a
mockable platform provider boundary.

#### Scenario: Daemon unit test uses fake IIS provider
- **WHEN** daemon logic is tested with a fake IIS provider
- **THEN** tests SHALL cover discovery, apply, restore, and failure paths without real IIS
- **AND** the same daemon logic SHALL be used with the real provider in production

#### Scenario: IIS provider command fails
- **WHEN** the platform provider fails to apply or restore IIS state
- **THEN** Cadder SHALL return a typed error with recoverable context
- **AND** it SHALL preserve enough state to retry or restore safely

### Requirement: Elevation is scoped
Cadder SHALL require elevation only for operations that need it, such as IIS
binding mutation or an explicitly elevated daemon mode.

#### Scenario: Non-admin user views IIS status
- **WHEN** a non-admin user views IIS handoff status
- **THEN** Cadder SHALL show what can be discovered at user privilege
- **AND** it SHALL identify actions that require elevation

#### Scenario: Elevated operation requested
- **WHEN** an operation requires admin rights
- **THEN** Cadder SHALL use the documented elevated helper or elevated daemon flow
- **AND** it SHALL return control to the user-level client with a clear result

### Requirement: Real IIS behavior has system smoke coverage
IIS handoff SHALL have focused system smoke coverage for behavior that cannot be
validated through fake providers.

#### Scenario: Sandbox smoke checks IIS handoff
- **WHEN** the Windows Sandbox smoke harness runs an IIS handoff and restore cycle
- **THEN** Cadder SHALL apply the handoff, preserve restore metadata, restore the binding, and leave no stale handoff state
- **AND** unsupported IIS environments SHALL produce a clear unavailable result
