# windows-iis-handoff Specification

## Purpose
Define explicit Windows IIS discovery, preview, scoped elevation, authenticated mutation, rollback, restore, cleanup, and auditable outcomes.

## Requirements
### Requirement: IIS-001: IIS integration is explicit and Windows-only
Cadder SHALL expose IIS status, preview, apply, and restore operations on Windows. On other platforms, status MUST report IIS as unsupported without treating the query as a failure, while preview, apply, and restore MUST return a typed unsupported-platform precondition error.

#### Scenario: Windows operator inspects IIS
- **WHEN** a Windows user runs `cadder iis status`
- **THEN** Cadder reports discovered IIS bindings, Cadder ownership, handoff state, and whether elevation is required for the next operation

#### Scenario: Non-Windows operator checks IIS status
- **WHEN** a Linux or macOS user invokes `cadder iis status`
- **THEN** the operator reports that IIS is unsupported and exits successfully
- **AND** it does not contact an elevation mechanism

#### Scenario: Non-Windows operator requests IIS mutation
- **WHEN** a Linux or macOS user invokes IIS preview, apply, or restore
- **THEN** the operator reports that IIS is unsupported with the stable precondition exit code
- **AND** it does not contact an elevation mechanism

### Requirement: IIS-002: Preview is complete and does not mutate IIS
`cadder iis preview` MUST produce a typed plan that lists every binding read, addition, removal, replacement, ownership check, precondition, and rollback action without changing IIS or active Cadder routing. A serialized plan MUST NOT exceed 524,288 bytes. The daemon SHALL persist the immutable plan in owner-protected runtime state, record a redacted preview history event, and return a single-use plan ID that expires five minutes after creation. A profile SHALL retain at most 32 unused plans; creating another preview MUST invalidate and remove the oldest unused payload while recording a superseded outcome.

An expired plan payload MUST be removed during a bounded maintenance cycle within one minute. A consumed plan payload MAY remain only until its helper returns or reaches its deadline and MUST then be removed within one minute. Redacted plan ID, canonical hash, timestamps, and outcome SHALL remain only in normal history under `OBS-006`.

#### Scenario: Valid handoff preview
- **WHEN** a user previews a handoff for an eligible binding
- **THEN** the plan identifies the exact existing binding, proposed Cadder route, required privilege, expected post-state, and inverse restore action

#### Scenario: Conflicting ownership
- **WHEN** an existing binding is not owned by the current Cadder profile and cannot be safely handed off
- **THEN** preview reports a blocking conflict
- **AND** apply cannot accept that plan

#### Scenario: Preview repeated
- **WHEN** the same unchanged system state is previewed twice
- **THEN** the semantic plan and its canonical hash are identical

#### Scenario: Preview expires
- **WHEN** five minutes pass without an accepted apply or restore request
- **THEN** the daemon marks the plan expired
- **AND** bounded maintenance removes its payload while later use of its plan ID fails as a precondition without launching the helper

#### Scenario: Preview plan is too large
- **WHEN** a complete canonical IIS plan would exceed 524,288 serialized bytes
- **THEN** preview fails as configuration-too-large without storing a partial plan
- **AND** IIS and active Cadder routing remain unchanged

#### Scenario: Unused-plan capacity is reached
- **WHEN** a profile already retains 32 unused plans and creates another preview
- **THEN** the daemon invalidates and removes the oldest unused plan payload before storing the new plan
- **AND** history records the superseded outcome without retaining the removed payload

### Requirement: IIS-003: Normal Cadder operation remains unelevated
The daemon, shim, CLI, and TUI MUST run at user privilege, and IIS discovery or mutation SHALL NOT turn the general daemon into an elevated control service.

#### Scenario: IIS is unused
- **WHEN** a user runs Cadder without invoking IIS apply or restore
- **THEN** no elevation prompt or elevated process occurs

#### Scenario: Elevated operator launches Cadder normally
- **WHEN** Cadder detects that the general daemon was started elevated
- **THEN** it refuses normal service startup with guidance to run the per-user daemon without elevation

### Requirement: IIS-004: Mutation uses one authenticated helper invocation
Each IIS apply or restore MUST consume one current, unused plan ID before launching the installed `cadderd` binary in a one-shot elevated helper mode. The invocation SHALL bind the helper to one canonical plan and requesting daemon instance and SHALL terminate it after one result or deadline. A consumed, expired, changed, or unknown plan MUST NOT launch another helper and MUST require a new preview.

#### Scenario: Helper receives an accepted plan
- **WHEN** the user approves elevation for a current preview
- **THEN** the normal daemon sends the canonical typed plan through a private one-shot channel
- **AND** the helper verifies the daemon instance, plan hash, nonce, expiry, and allowed operation before mutation

#### Scenario: Plan changes after preview
- **WHEN** system state or plan content differs from the approved plan hash
- **THEN** the helper rejects the request before changing IIS

#### Scenario: Helper receives a second request
- **WHEN** any client attempts another request on a completed or expired helper channel
- **THEN** the channel rejects it and the helper exits

#### Scenario: Plan ID is replayed
- **WHEN** a client submits a plan ID already consumed by apply or restore
- **THEN** the daemon rejects the replay before elevation
- **AND** it directs the user to create a new preview

### Requirement: IIS-005: The helper executes an allowlisted plan
The elevated helper MUST accept only typed IIS binding operations required by the approved plan and MUST NOT execute project paths, arbitrary commands, scripts, or unvalidated PowerShell text.

#### Scenario: Plan contains unsupported operation
- **WHEN** a request contains an operation outside the IIS binding allowlist
- **THEN** validation rejects the entire plan before any mutation

#### Scenario: Project content contains executable text
- **WHEN** project-controlled configuration includes a command, script path, or executable reference
- **THEN** the helper ignores it as non-plan data and refuses any attempt to promote it into an elevated operation

### Requirement: IIS-006: Apply is idempotent and transactional
Applying a freshly previewed plan for an unchanged desired handoff MUST converge on the same post-state, and any partial failure MUST trigger the plan's validated rollback before reporting the result. Plan IDs remain single-use under `IIS-004`; semantic idempotency MUST NOT permit replay.

#### Scenario: First apply succeeds
- **WHEN** the helper applies a valid plan against its expected pre-state
- **THEN** every planned binding reaches the expected post-state
- **AND** Cadder records the applied plan and restore information only after verification

#### Scenario: Apply is repeated
- **WHEN** the user creates and applies a new preview for a handoff already in its expected post-state
- **THEN** the helper reports an idempotent success without duplicating bindings or losing restore data

#### Scenario: Mutation fails partway
- **WHEN** one planned IIS mutation fails after an earlier mutation succeeded
- **THEN** the helper executes validated inverse operations in reverse order
- **AND** reports both the primary failure and rollback outcome

### Requirement: IIS-007: Restore returns owned bindings to the recorded pre-state
`cadder iis restore` SHALL restore only bindings covered by a verified Cadder handoff record and SHALL preserve unrelated IIS configuration.

#### Scenario: Restore succeeds
- **WHEN** current IIS state still matches the applied handoff's owned post-state
- **THEN** the helper restores the recorded pre-state and Cadder marks the handoff inactive

#### Scenario: Binding changed externally
- **WHEN** an owned binding no longer matches the recorded post-state
- **THEN** restore stops with a conflict preview
- **AND** it does not overwrite the external change

#### Scenario: Unrelated site exists
- **WHEN** IIS contains sites or bindings outside the handoff record
- **THEN** apply and restore leave them unchanged

### Requirement: IIS-008: Elevated communication is private and bounded
The one-shot IIS channel and its temporary authentication material MUST be accessible only to the requesting user, the launched elevated helper, and required operating-system principals, and MUST expire within a documented timeout.

#### Scenario: Unrelated local process connects
- **WHEN** another user or unrelated process attempts to use the helper channel
- **THEN** operating-system access control or authenticated handshake denies it before plan parsing

#### Scenario: Helper deadline expires
- **WHEN** elevation, connection, or execution exceeds the deadline
- **THEN** both sides close the channel, remove temporary authentication material, and report a timeout without leaving a reusable privileged endpoint

### Requirement: IIS-009: IIS outcomes are auditable without secrets
Cadder SHALL record preview, approval, helper validation, mutation, rollback, and restore outcomes as redacted history and log events linked by request and plan identifiers.

#### Scenario: Apply is denied at elevation prompt
- **WHEN** the user declines or cancels elevation
- **THEN** history records a cancelled outcome without claiming a helper or IIS mutation ran

#### Scenario: Helper rejects authentication
- **WHEN** helper authentication fails
- **THEN** the daemon records a security rejection with identifiers and reason category but omits nonces, credentials, and sensitive IIS data
