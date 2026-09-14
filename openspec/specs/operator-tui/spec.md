# Operator TUI

## Purpose
Define the keyboard-operated Cadder 1.0 interface.

## Requirements

### Requirement: TUI-001: Three bounded views expose current state
The TUI SHALL render Status, Domains, and Logs from bounded daemon responses and SHALL support collections of arbitrary length without fixed row-count assumptions.

#### Scenario: Many registrations
- **WHEN** more domains exist than fit in the viewport
- **THEN** selection and scrolling remain bounded by the actual collection length

### Requirement: TUI-002: Lifecycle actions are explicit
Status MUST provide idempotent Start, confirmed bounded Stop, and ordered Restart actions with pending, offline, stale, failure, and success states.

#### Scenario: Restart
- **WHEN** the user confirms Restart
- **THEN** the TUI observes shutdown before launching and confirming the new daemon

### Requirement: TUI-003: Rendering remains accessible and recoverable
The TUI MUST support keyboard-only navigation, visible focus, text-independent color meaning, bounded layouts, and terminal restoration after success, error, panic, or cancellation.

#### Scenario: Terminal is narrow
- **WHEN** available width is constrained
- **THEN** columns are clipped or reflowed without panicking or assuming a constant width
