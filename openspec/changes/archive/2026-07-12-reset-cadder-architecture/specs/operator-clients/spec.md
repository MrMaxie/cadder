## ADDED Requirements

### Requirement: `cadder` exposes CLI and TUI contact surfaces
The `cadder` executable SHALL provide CLI and TUI workflows for inspecting,
starting, configuring, and managing Cadder through the daemon protocol.

#### Scenario: CLI command reads daemon state
- **WHEN** a user runs a CLI status command
- **THEN** `cadder` SHALL request state through the daemon protocol
- **AND** it SHALL render a deterministic, script-friendly result when requested

#### Scenario: TUI opens runtime overview
- **WHEN** a user opens the TUI
- **THEN** the TUI SHALL render runtime status, project/domain state, logs access, and available actions from a view model
- **AND** the TUI SHALL NOT depend on a real daemon for rendering tests

### Requirement: Clients remain useful while daemon is down
CLI and TUI SHALL handle daemon-down states as a normal product state.

#### Scenario: CLI state change while daemon is down
- **WHEN** a user runs a state-changing CLI command and no daemon is reachable
- **THEN** the command SHALL fail with an actionable daemon-unavailable message
- **AND** it SHALL include the supported start command

#### Scenario: TUI opens while daemon is down
- **WHEN** a user opens the TUI and no daemon is reachable
- **THEN** the TUI SHALL show offline status and setup diagnostics
- **AND** it SHALL offer a visible start action instead of exiting immediately

### Requirement: Client view models are mockable
Operator clients SHALL render from reusable view models supplied by a mockable
client service boundary.

#### Scenario: TUI rendering test uses fixture state
- **WHEN** a TUI test supplies fixture runtime, domain, platform-integration, and log state
- **THEN** the TUI SHALL render the expected view through a test backend
- **AND** no real daemon, real Caddy, or real IIS dependency SHALL be required

#### Scenario: CLI snapshot test uses fake daemon relation
- **WHEN** a CLI test supplies a fake daemon client result
- **THEN** the CLI SHALL render stable output suitable for snapshot or golden-file verification
- **AND** the test SHALL not open real IPC

### Requirement: Operator clients expose log workflows
CLI and TUI SHALL support log inspection for all logs, one project, one domain,
and selectable severity levels.

#### Scenario: CLI tails project logs
- **WHEN** a user tails logs for one project at info level
- **THEN** `cadder` SHALL request the corresponding daemon log stream
- **AND** it SHALL render only records matching the daemon's canonical filter

#### Scenario: TUI filters domain logs
- **WHEN** a user selects a domain and changes severity in the TUI
- **THEN** the TUI SHALL update the log query through the client service
- **AND** it SHALL preserve the surrounding runtime context

### Requirement: Client invocation model is documented
The project SHALL document whether TUI is launched through an explicit
subcommand, automatic TTY detection, or both before implementation depends on
that behavior.

#### Scenario: User asks for interactive mode
- **WHEN** a user invokes the documented TUI entrypoint
- **THEN** `cadder` SHALL start the TUI or explain why the terminal does not support it
- **AND** non-interactive CLI behavior SHALL remain predictable for automation
