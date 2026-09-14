# Operator TUI

## Purpose
Define the keyboard-operated Cadder 1.0 interface.

## Requirements

### Requirement: TUI-001: One routes-first workspace exposes current state
The TUI SHALL render every project and domain in one primary table, SHALL expose logs only as a contextual panel for the current selection, and SHALL support collections of arbitrary length without fixed row-count assumptions.

#### Scenario: Many registrations
- **WHEN** more projects or domains exist than fit in the viewport
- **THEN** selection and scrolling remain bounded by the actual collection length

#### Scenario: A developer inspects logs
- **WHEN** the developer opens logs for the current selection
- **THEN** a bounded log panel appears without replacing the routes workspace

### Requirement: TUI-002: Routine state is quiet and exceptional state is actionable
The TUI SHALL keep healthy global Caddy state in the header, SHALL use filled and empty shape markers for route activation, and SHALL omit diagnostic identifiers, redundant counts, and repeated state labels from the primary workspace.

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
The TUI MUST support keyboard-only navigation, visible focus, shape-based state meaning independent of color, bounded layouts, and terminal restoration after success, error, panic, or cancellation.

#### Scenario: Terminal is narrow
- **WHEN** available width is constrained
- **THEN** columns and contextual panels are clipped or reflowed without panicking or assuming a constant width
