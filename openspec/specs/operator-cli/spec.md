# Operator CLI

## Purpose
Define the intentionally small command surface of the operator executable.

## Requirements

### Requirement: CLI-001: The operator exposes only the TUI
`cadder` MUST support help, version, and the `tui` subcommand and MUST reject removed profile, machine-output, lifecycle, history, export, tail, watch, and autostart commands.

#### Scenario: Bare invocation
- **WHEN** a user runs `cadder` without a subcommand
- **THEN** Cadder prints help successfully without starting the TUI

#### Scenario: TUI invocation
- **WHEN** a user runs `cadder tui`
- **THEN** Cadder opens the interactive operator
