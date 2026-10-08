# Operator CLI

## Purpose
Define the intentionally small command surface of the operator executable.
## Requirements
### Requirement: CLI-001: The operator exposes focused developer workflows
Commander-based cadder SHALL preserve the released 1.0.5 help/version, status, daemon lifecycle, project/domain controls, port/Caddyfile inspection, diagnostics and bounded redacted log commands, with stable arguments, human-readable output and exit status through the shared client service. Bare cadder SHALL print help successfully without starting TUI or the daemon, regardless of TTY. cadder tui SHALL open the routes workspace without implicit daemon startup; its explicit --start-daemon option SHALL use the shared attach-first bounded launch contract. Explicit daemon start/restart and managed caddy run SHALL retain their launch policy. Other state and inspection commands SHALL remain attach-only. Profile, machine-output, history, export, continuous tail, watch, IIS and autostart commands SHALL be rejected as unsupported.

#### Scenario: Bare invocation
- **WHEN** cadder has no subcommand, with or without a TTY
- **THEN** it SHALL print help successfully without starting a daemon or interactive session

#### Scenario: Explicit TUI startup option
- **WHEN** the user runs cadder tui --start-daemon
- **THEN** Cadder SHALL start or attach through the bounded shared launch path before opening the TUI

#### Scenario: CLI state command while daemon is offline
- **WHEN** a state command cannot attach
- **THEN** it SHALL report actionable offline state with the stable exit contract
- **AND** it SHALL NOT start a daemon implicitly

#### Scenario: Unsupported expansion is requested
- **WHEN** the user requests machine output, history, export, continuous tail, watch, profiles, IIS or autostart
- **THEN** Cadder SHALL reject the unsupported input rather than restoring an older command surface
