# Operator TUI

## Purpose
Define the keyboard-operated Cadder 1.0 interface.

## Requirements

### Requirement: TUI-001: One routes-first workspace exposes current state
The TUI SHALL render every project and domain in one primary table and SHALL support collections of arbitrary length without fixed row-count assumptions. Domain rows SHALL use tree connectors beneath their project, and projects SHALL follow one another without empty separator rows. The TUI SHALL NOT expose a separate activity or logs view.

#### Scenario: Many registrations
- **WHEN** more projects or domains exist than fit in the viewport
- **THEN** selection and scrolling remain bounded by the actual collection length

### Requirement: TUI-002: Routine state is quiet and exceptional state is actionable
The TUI SHALL keep `cadderd` connection state and Caddy runtime state separately visible in the header, SHALL use filled and empty shape markers for route activation, and SHALL omit diagnostic identifiers, redundant counts, and repeated state labels from the primary workspace. Neutral colors SHALL carry routine structure while the green accent is reserved for focus, enabled state, and available keys.

#### Scenario: Cadder is healthy
- **WHEN** the daemon and Caddy runtime are operating normally
- **THEN** the interface emphasizes projects, domains, targets, and available actions instead of internal process or storage details

#### Scenario: An action fails
- **WHEN** a route or lifecycle action fails
- **THEN** the interface presents the failure and available recovery guidance inline

### Requirement: TUI-003: Lifecycle actions are explicit
The primary workspace MUST provide idempotent Start, confirmed bounded Stop, and ordered Restart actions with pending, offline, failure, and success states.

#### Scenario: Restart
- **WHEN** the user confirms Restart
- **THEN** the TUI observes shutdown before launching and confirming the new daemon

### Requirement: TUI-004: Rendering remains accessible and recoverable
The TUI MUST support keyboard-only navigation, visible focus, shape-based state meaning independent of color, bounded layouts, and terminal restoration after success, error, panic, or cancellation. Space and Enter SHALL both toggle the selected project or domain while Enter SHALL start `cadderd` when the daemon is offline.

#### Scenario: Terminal is narrow
- **WHEN** available width is constrained
- **THEN** columns are clipped or reflowed without panicking or assuming a constant width
