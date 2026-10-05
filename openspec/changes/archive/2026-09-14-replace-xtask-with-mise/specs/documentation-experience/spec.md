## MODIFIED Requirements

### Requirement: DOC-003: Documentation environment boundary
End-user Cadder documentation MUST remain independent of any maintainer's workstation, private operational workspace, repository checkout, or development-only backend.

End-user pages SHALL use portable placeholders and product-level concepts. They MUST NOT expose `.local`, personal absolute paths, checkout-specific variables, mock-only commands, agent instructions, or repository task-runner details. Contributor and release-maintainer documentation MAY describe checked-in `mise run` tasks, direct native-tool commands, and supported development fixtures, but MUST remain free of personal workstation paths and private operational data.

#### Scenario: End-user content boundary scan
- **WHEN** end-user documentation is validated
- **THEN** it contains no `.local` references, personal filesystem paths, checkout-specific variables, mock-only workflows, or agent-only instructions

#### Scenario: Portable path example
- **WHEN** a command requires a path example
- **THEN** the documentation uses a platform-appropriate placeholder or standard installation location instead of a maintainer-specific path

#### Scenario: Contributor runs repository validation
- **WHEN** contributor documentation explains the supported repository gate
- **THEN** it names checked-in `mise run` tasks and direct owning-tool commands where diagnostic detail is useful
- **AND** it does not require `xtask`, `just`, Nushell, `.local`, a private backend, or a maintainer-specific path
