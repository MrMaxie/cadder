# Operator TUI

## Purpose
Define the keyboard-operated Cadder 1.0 interface.

## Requirements

### Requirement: TUI-001: One routes-first workspace exposes current state
Ink/React TUI SHALL render real daemon-backed projects and domains in one routes-first primary table through the same client service used by CLI, without production mocks or random statuses. Domain rows SHALL use tree connectors beneath their project, with no empty separator rows between projects. Collections SHALL support scrolling without fixed lengths. The TUI SHALL NOT add separate activity, logs, history, IIS or autostart views; explicit diagnostics and bounded logs SHALL remain CLI surfaces.

#### Scenario: Operator changes a domain
- **WHEN** the user changes activation in the routes workspace
- **THEN** the TUI SHALL render committed daemon state without unrelated diagnostic details
- **AND** fixture data SHALL be limited to tests/development, not production

### Requirement: TUI-002: Routine state is quiet and exceptional state is actionable
The TUI SHALL keep `cadderd` connection state and Caddy runtime state separately visible in the header, SHALL use filled and empty shape markers for route activation, and SHALL omit diagnostic identifiers, redundant counts, and repeated state labels from the primary workspace. Neutral colors SHALL carry routine structure while the green accent is reserved for focus, enabled state, and available keys.

#### Scenario: Cadder is healthy
- **WHEN** the daemon and Caddy runtime are operating normally
- **THEN** the interface emphasizes projects, domains, targets, and available actions instead of internal process or storage details

#### Scenario: An action fails
- **WHEN** a route or lifecycle action fails
- **THEN** the interface presents the failure and available recovery guidance inline

### Requirement: TUI-003: Lifecycle actions are explicit
TUI SHALL offer idempotent explicit start while offline, confirmed bounded stop and ordered restart with pending/success/failure states. Startup SHALL follow the shared client launch boundary. Opening cadder tui without --start-daemon SHALL NOT imply daemon start; the explicit --start-daemon option SHALL start or attach before rendering.

#### Scenario: Offline TUI opens
- **WHEN** the user opens cadder tui without --start-daemon and no daemon is running
- **THEN** TUI SHALL display offline state and offer an explicit start action
- **AND** restart SHALL observe prior shutdown before starting the replacement

### Requirement: TUI-004: Rendering remains accessible and recoverable
The TUI MUST support keyboard-only navigation, visible focus, shape-based state meaning independent of color, bounded layouts, and terminal restoration after success, error, panic, or cancellation. Space and Enter SHALL both toggle the selected project or domain while Enter SHALL start `cadderd` when the daemon is offline.

#### Scenario: Terminal is narrow
- **WHEN** available width is constrained
- **THEN** columns are clipped or reflowed without panicking or assuming a constant width
