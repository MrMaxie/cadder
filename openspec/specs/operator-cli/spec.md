# Operator CLI

## Purpose
Define the intentionally small command surface of the operator executable.
## Requirements
### Requirement: CLI-001: The operator exposes focused developer workflows
`cadder` MUST support help, version, `tui`, daemon lifecycle and status, project listing and activation, domain listing and activation, port inspection and guarded process termination, Caddyfile inspection, explicit diagnostics, and bounded redacted log reads. It MUST reject profile, machine-output, history, export, continuous tail, watch, and autostart commands.

#### Scenario: Bare invocation
- **WHEN** a user runs `cadder` without a subcommand
- **THEN** Cadder prints help successfully without starting the TUI

#### Scenario: TUI invocation
- **WHEN** a user runs `cadder tui`
- **THEN** Cadder opens the interactive operator

#### Scenario: General state inspection
- **WHEN** a user runs `cadder status`, `cadder projects list`, or `cadder domains list`
- **THEN** Cadder reports current daemon-backed state using developer-facing project and domain terminology

#### Scenario: Activation management
- **WHEN** a user explicitly enables or disables a selected project or domain
- **THEN** Cadder applies the mutation through the daemon and reports the result

#### Scenario: Daemon lifecycle management
- **WHEN** a user explicitly starts, stops, or restarts `cadderd`
- **THEN** Cadder uses the existing attach-first, bounded stop, or ordered restart operation and reports the outcome

#### Scenario: On-demand diagnostics
- **WHEN** a user requests diagnostics or bounded logs for the runtime, a project, or a domain
- **THEN** Cadder reports the selected diagnostic data without adding it back to the primary TUI

#### Scenario: Removed command
- **WHEN** a user invokes a profile, machine-output, history, export, continuous tail, watch, or autostart command
- **THEN** Cadder rejects it as unsupported CLI input
