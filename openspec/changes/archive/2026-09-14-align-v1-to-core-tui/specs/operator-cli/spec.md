## MODIFIED Requirements

### Requirement: CLI-001: Stable public command hierarchy
The public operator executable SHALL accept exactly the following invocation surface:

```text
cadder --help
cadder --version
cadder tui
```

No other command, global output mode, profile selector, runtime-directory selector, or hidden production command SHALL be part of the Cadder 1.0 operator contract.

#### Scenario: TUI launch
- **WHEN** an operator runs `cadder tui`
- **THEN** Cadder starts the full-screen operator for the installation runtime

#### Scenario: Unsupported command
- **WHEN** an operator supplies any other subcommand or option
- **THEN** argument parsing fails before connecting to or changing the daemon
- **AND** help lists only the supported invocation surface

### Requirement: CLI-002: Explicit invocation and attach-first behavior
Running `cadder` without a subcommand or with `--help` SHALL print help without starting the TUI, daemon, or Caddy. Running `cadder --version` SHALL print the Cadder version without attaching to the daemon. Only `cadder tui` SHALL enter the operator workflow.

#### Scenario: Bare invocation
- **WHEN** an operator runs `cadder`
- **THEN** concise help is written and the process exits without starting runtime processes

#### Scenario: Version invocation
- **WHEN** an operator runs `cadder --version`
- **THEN** the executable reports the archive version without requiring a daemon connection

#### Scenario: TUI invocation
- **WHEN** an operator runs `cadder tui`
- **THEN** the TUI attaches to the installation runtime or presents an explicit action to start its daemon

## REMOVED Requirements

### Requirement: CLI-003: Global output and runtime selection options
**Reason**: The retained operator has one runtime and no machine-output command surface.

**Migration**: Use the installation runtime through `cadder tui`; test-only overrides remain private implementation seams.

### Requirement: CLI-004: Deterministic target selection
**Reason**: Command-line entrypoint, domain, history, and log targets are removed from the 1.0 operator surface.

**Migration**: Select visible entrypoints and domains inside the TUI.

### Requirement: CLI-005: Human-readable output discipline
**Reason**: Only clap-generated help, version output, and TUI startup errors remain at the shell boundary; they do not justify a separate operator-output framework.

**Migration**: Keep shell errors concise and put runtime details in the TUI diagnostic layer.

### Requirement: CLI-006: Versioned JSON and JSONL contracts
**Reason**: Cadder 1.0 exposes no machine-output CLI or export artifact.

**Migration**: A future automation journey must define its own audience and stable output contract.

### Requirement: CLI-007: Stable exit-code taxonomy
**Reason**: The removed command tree is the only consumer of the detailed exit taxonomy.

**Migration**: Help and version return ordinary process success; argument, startup, and terminal failures return nonzero without promising a public numeric taxonomy.

### Requirement: CLI-008: Safe daemon control and diagnostics
**Reason**: Daemon lifecycle and inspection move to explicit TUI actions and views.

**Migration**: Use Start, Stop, Restart, Status, and Logs in `cadder tui`; use foreground `cadderd` for operational diagnosis.

### Requirement: CLI-009: Bounded logs and history workflows
**Reason**: History, tail, export, paging, filters, and cursor workflows are not part of the core TUI journey.

**Migration**: Use the TUI Logs view for the bounded recent log set.

### Requirement: CLI-010: Shim setup behavior
**Reason**: The portable archive already supplies the PATH-facing shim as `caddy`; Cadder does not manage aliases or an independent Caddy installation.

**Migration**: Put the extracted Cadder directory on PATH according to the portable installation documentation.
