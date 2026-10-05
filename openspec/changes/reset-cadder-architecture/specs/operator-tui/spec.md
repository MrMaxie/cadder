## MODIFIED Requirements

### Requirement: TUI-001: One routes-first workspace exposes current state
Ink/React TUI SHALL render real daemon-backed projects and domains in one routes-first primary table through the same client service used by CLI, without production mocks or random statuses. Domain rows SHALL use tree connectors beneath their project, with no empty separator rows between projects. Collections SHALL support scrolling without fixed lengths. The TUI SHALL NOT add separate activity, logs, history, IIS or autostart views; explicit diagnostics and bounded logs SHALL remain CLI surfaces.

#### Scenario: Operator changes a domain
- **WHEN** the user changes activation in the routes workspace
- **THEN** the TUI SHALL render committed daemon state without unrelated diagnostic details
- **AND** fixture data SHALL be limited to tests/development, not production

### Requirement: TUI-003: Lifecycle actions are explicit
TUI SHALL offer idempotent explicit start while offline, confirmed bounded stop and ordered restart with pending/success/failure states. Startup SHALL follow the shared client launch boundary. Opening cadder tui without --start-daemon SHALL NOT imply daemon start; the explicit --start-daemon option SHALL start or attach before rendering.

#### Scenario: Offline TUI opens
- **WHEN** the user opens cadder tui without --start-daemon and no daemon is running
- **THEN** TUI SHALL display offline state and offer an explicit start action
- **AND** restart SHALL observe prior shutdown before starting the replacement
