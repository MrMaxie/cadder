## MODIFIED Requirements

### Requirement: IIS-005: The helper executes an allowlisted plan
The elevated helper MUST accept only typed IIS binding operations required by the approved plan and MUST NOT execute project paths, arbitrary commands, generated temporary scripts, runtime-provided script paths, or unvalidated PowerShell text. The installed `cadderd` helper SHALL resolve any required Windows system executable or module from an absolute operating-system location and SHALL NOT use user-controlled `PATH`, profiles, or module search paths while elevated.

#### Scenario: Plan contains unsupported operation
- **WHEN** a request contains an operation outside the IIS binding allowlist
- **THEN** validation rejects the entire plan before any mutation

#### Scenario: Project content contains executable text
- **WHEN** project-controlled configuration includes a command, script path, or executable reference
- **THEN** the helper ignores it as non-plan data and refuses any attempt to promote it into an elevated operation

#### Scenario: Generated temporary script is offered for elevation
- **WHEN** the daemon or helper would need to execute a mutable temporary script to perform the approved binding operations
- **THEN** the operation fails before elevation or IIS mutation
- **AND** the operator receives guidance to use an installed Cadder build with the typed IIS helper

#### Scenario: Elevated system tooling is resolved
- **WHEN** the helper invokes Windows tooling needed for an allowlisted IIS binding operation
- **THEN** it uses absolute operating-system paths without loading user profiles or user-controlled module search locations
