# operator-cli Specification

## Purpose
Define the complete public command tree, targeting rules, human and machine output, exit codes, daemon control, and bounded operational workflows.

## Requirements
### Requirement: CLI-001: Stable public command hierarchy
The `cadder` executable SHALL expose the following public 1.0 command hierarchy and SHALL reject removed or unrecognized product surfaces as invalid input:

```text
cadder
cadder tui
cadder doctor
cadder daemon status
cadder daemon start [--real-caddy <absolute-path>]
cadder daemon stop
cadder daemon restart [--real-caddy <absolute-path>]
cadder entrypoint list
cadder entrypoint enable <registration-id>
cadder entrypoint disable <registration-id>
cadder entrypoint forget <registration-id>
cadder domain list [--registration <registration-id>]
cadder domain enable <domain> [--registration <registration-id>]
cadder domain disable <domain> [--registration <registration-id>]
cadder logs show runtime [log-read-options]
cadder logs show entrypoint <registration-id> [log-read-options]
cadder logs show domain <domain> [--registration <registration-id>] [log-read-options]
cadder logs tail runtime [log-read-options]
cadder logs tail entrypoint <registration-id> [log-read-options]
cadder logs tail domain <domain> [--registration <registration-id>] [log-read-options]
cadder logs export runtime [log-read-options] [--output-path <path>]
cadder logs export entrypoint <registration-id> [log-read-options] [--output-path <path>]
cadder logs export domain <domain> [--registration <registration-id>] [log-read-options] [--output-path <path>]
cadder history show [--kind <registration|runtime|config|iis|autostart|log>] [--registration <registration-id>] [--limit <count>] [--after-cursor <cursor>] [--since <timestamp>] [--until <timestamp>]
cadder autostart status
cadder autostart enable
cadder autostart disable
cadder iis status
cadder iis preview handoff <binding-id> [--route-host <host>]
cadder iis preview restore <binding-id> [--route-host <host>]
cadder iis apply <plan-id>
cadder iis restore <plan-id>
cadder setup shim [--dir <path>]
cadder setup shim --remove [--dir <path>]
```

`log-read-options` SHALL consist of `--limit <count>`, `--minimum-severity <trace|debug|info|warn|error|fatal>`, `--after-cursor <cursor>`, `--since <timestamp>`, and `--until <timestamp>`. Web, Tauri, MCP, and externally exposed `watch` commands SHALL NOT be part of the 1.0 command hierarchy.

#### Scenario: Help presents the complete command hierarchy
- **WHEN** an operator runs `cadder --help` or requests help for a command group
- **THEN** the CLI lists the applicable commands, operands, global options, and concise operator-facing descriptions from the hierarchy above

#### Scenario: Excluded surface is rejected
- **WHEN** an operator invokes `cadder web`, `cadder mcp`, or another command outside the public hierarchy
- **THEN** the CLI rejects the invocation as invalid input without starting a service or changing runtime state

### Requirement: CLI-002: Explicit invocation and attach-first behavior
Running `cadder` without a subcommand SHALL print root help and exit successfully, and the full-screen interface SHALL start only through `cadder tui`. Commands that inspect or mutate daemon-owned state SHALL attach to the selected daemon and SHALL NOT start it implicitly. Daemon startup SHALL occur only through `daemon start`, `daemon restart`, or an explicit action inside the TUI.

#### Scenario: Bare invocation is non-interactive
- **WHEN** an operator runs `cadder` without arguments
- **THEN** the CLI prints root help, exits with code `0`, and does not enter terminal raw mode or start `cadderd`

#### Scenario: Attach-required command finds no daemon
- **WHEN** an operator runs an attach-required command while no daemon serves the selected runtime
- **THEN** the command exits with the daemon-unavailable classification and gives explicit `cadder daemon start` recovery guidance without starting the daemon

### Requirement: CLI-003: Global output and runtime selection options
The CLI SHALL accept the global options `--output <human|json|jsonl>`, `--no-color`, `--profile <name>`, and `--runtime-dir <path>` before or after the subcommand path. Human output SHALL be the default. Runtime selection SHALL use explicit `--runtime-dir` first, then `CADDER_RUNTIME_DIR`, then the selected profile's per-user runtime directory; `--runtime-dir` SHALL remain an advanced override and SHALL NOT change persistent configuration. A command MUST reject an output mode that cannot represent its result before connecting or changing state.

#### Scenario: Explicit runtime directory takes precedence
- **WHEN** an operator supplies `--runtime-dir` while an environment runtime directory and a profile are also present
- **THEN** the command attaches only to the explicit directory and reports that directory in status or diagnostics output

#### Scenario: Invalid global option is rejected
- **WHEN** an operator provides an unknown output mode, profile, or malformed runtime path
- **THEN** the CLI returns the invalid-input exit code without connecting to the daemon or changing files

#### Scenario: Tail requests buffered JSON
- **WHEN** an operator selects `--output json` for `logs tail`
- **THEN** the CLI exits with code `2` before connecting
- **AND** it explains that an unbounded tail supports only human or JSONL output

### Requirement: CLI-004: Deterministic target selection
Entrypoint mutations SHALL select an exact live or durable registration ID. Domain operations SHALL canonicalize the requested domain and SHALL require `--registration` when more than one entrypoint contains that domain. Log and history reads MAY select a live, durable, or tombstoned registration ID and MUST retain that historical query identity until its last retained record expires. Other commands MUST reject a tombstoned ID as a failed precondition.

#### Scenario: Unique domain is selected
- **WHEN** one registered entrypoint contains the requested canonical domain and no registration filter is supplied
- **THEN** the CLI selects that entrypoint and performs the requested operation against the matching domain

#### Scenario: Repeated domain requires disambiguation
- **WHEN** multiple entrypoints contain the requested canonical domain and no registration filter is supplied
- **THEN** the CLI exits with the conflict-or-precondition code, lists the matching registration IDs, and instructs the operator to retry with `--registration`

#### Scenario: Target is absent
- **WHEN** an entrypoint ID, domain, log stream, IIS binding, or plan ID exists in neither the authoritative snapshot nor its permitted historical index
- **THEN** the CLI exits with the conflict-or-precondition code and does not mutate another target

#### Scenario: Forgotten entrypoint has retained logs
- **WHEN** an operator requests logs for a tombstoned registration ID with retained records
- **THEN** the CLI returns those records and identifies the entrypoint as forgotten
- **AND** it does not present the tombstone as an active entrypoint

#### Scenario: Live entrypoint cannot be forgotten
- **WHEN** an operator invokes `entrypoint forget` for an entrypoint with a current lease
- **THEN** the CLI exits with code `5`
- **AND** it instructs the operator to stop the owning shim before retrying

### Requirement: CLI-005: Human-readable output discipline
In human mode, successful command data SHALL be written to standard output, while errors, warnings, and recovery guidance SHALL be written to standard error. Human output SHALL use concise labels and actionable language, SHALL NOT expose mock data or internal serialization details, and SHALL contain no ANSI styling when `--no-color` is supplied.

#### Scenario: Human command succeeds
- **WHEN** a one-shot command succeeds in human mode
- **THEN** standard output contains the requested result, standard error contains no routine status noise, and the process exits with code `0`

#### Scenario: Human command fails
- **WHEN** a command fails in human mode
- **THEN** standard error contains the error and recovery guidance, standard output contains no partial machine payload, and the process returns the classified nonzero exit code

### Requirement: CLI-006: Versioned JSON and JSONL contracts
Except for the stdout log-export artifact defined by `CLI-009`, every JSON one-shot result SHALL be one object with integer `schemaVersion` value `1`, `command`, `ok`, and exactly one of `data` or `error`. An error object SHALL contain stable `kind`, `code`, `message`, `guidance`, `retryable`, `requestId`, and `exitCode` fields, with `guidance` and `requestId` represented as a string or `null` when absent. Every operator JSONL line SHALL be an independently valid object with `schemaVersion`, `command`, `event`, `ok`, and exactly one of `data` or `error`. Machine modes SHALL write their complete contract only to standard output and SHALL emit no human prose or ANSI control sequences.

#### Scenario: JSON command succeeds
- **WHEN** an operator requests `--output json` for a successful one-shot command
- **THEN** standard output contains one success envelope with `schemaVersion: 1`, command-specific `data`, and no `error`

#### Scenario: JSON command fails
- **WHEN** an operator requests `--output json` for a command that fails after the output mode can be determined
- **THEN** standard output contains one error envelope whose `exitCode` matches the process exit code, standard error is empty, and no success envelope is emitted

#### Scenario: JSONL stream emits an error
- **WHEN** a JSONL log tail encounters a terminal error
- **THEN** it emits one final `error` event using the same error object contract and exits with the classified code

### Requirement: CLI-007: Stable exit-code taxonomy
The CLI SHALL use only the following public exit codes: `0` for success, `2` for invalid input, `3` for daemon unavailable, `4` for permission or elevation failure, `5` for conflict or failed precondition including missing targets, ambiguous targets, and an incomplete retained range, `6` for Cadder or source configuration failure, `7` for real-Caddy resolution, validation, lifecycle, or administration failure, `8` for protocol incompatibility, and `9` for storage or uncategorized internal failure. Exit code `1` SHALL NOT be emitted by a classified operator result.

#### Scenario: Failure classification is reflected everywhere
- **WHEN** a command returns a classified operator error
- **THEN** the process status, human recovery message, and machine `error.exitCode` identify the same category

#### Scenario: Status observes a stopped daemon
- **WHEN** `cadder daemon status` can determine that the selected daemon is stopped
- **THEN** it reports the stopped state and exits with code `0` because the status query itself completed successfully

#### Scenario: Streaming command is cancelled by the operator
- **WHEN** an attached `logs tail` receives Ctrl+C and shuts down cleanly
- **THEN** it flushes complete output records, performs no mutation, and exits with code `0`

### Requirement: CLI-008: Safe daemon control and diagnostics
`daemon start`, `daemon stop`, and `daemon restart` SHALL be idempotent, SHALL operate only on the selected per-user daemon, and SHALL report the final observed state. Restart SHALL wait for the owned daemon to stop before starting and confirming readiness. `doctor` SHALL perform non-mutating checks of runtime discovery, IPC compatibility and permissions, storage, configuration, real-Caddy resolution, and platform integrations, and SHALL return the most specific blocking exit category when the installation is not operational.

#### Scenario: Start finds an already running daemon
- **WHEN** the selected daemon is already ready and the operator runs `cadder daemon start`
- **THEN** the command reports that no start was needed, leaves the process unchanged, and exits successfully

#### Scenario: Restart completes
- **WHEN** the selected daemon is running and the operator runs `cadder daemon restart`
- **THEN** Cadder stops only that owned daemon, starts its replacement, waits for readiness, and reports the replacement instance as running

#### Scenario: Doctor finds a protocol mismatch
- **WHEN** `cadder doctor` attaches to an incompatible daemon protocol
- **THEN** it reports the client and daemon compatibility information, provides upgrade guidance, performs no mutation, and exits with code `8`

### Requirement: CLI-009: Bounded logs and history workflows
One-shot log reads and exports SHALL default to at most `50` entries, history reads SHALL default to at most `100` records, and explicit limits SHALL be within `1..=500`. Log and history reads SHALL accept an opaque cursor or an inclusive RFC 3339 time range, reject an inverted range locally, and apply filters through the daemon's canonical query semantics. `--after-cursor` and `--since` MUST be mutually exclusive. A tail with `--until` SHALL close successfully after emitting every matching event through that timestamp.

`logs tail` SHALL support only human and operator JSONL output, resume from an opaque cursor without duplicating acknowledged entries, and report stream gaps or retention truncation. `logs export --output-path <path>` SHALL atomically write the versioned export JSONL artifact and return an ordinary human, JSON, or operator-JSONL command result. Without `--output-path`, export MUST require `--output jsonl` and write only versioned export record, gap, or error objects to standard output without an operator envelope. A retained-range gap SHALL remain in the artifact and end with exit code `5`; any records after the gap remain usable but the command MUST NOT report a complete export.

#### Scenario: Invalid read limit is rejected locally
- **WHEN** an operator supplies a log or history limit outside `1..=500`
- **THEN** the CLI exits with code `2` before sending a request to the daemon

#### Scenario: Tail crosses a retention gap
- **WHEN** the next available cursor follows entries removed by retention
- **THEN** human output displays an explicit gap notice and JSONL emits a data event that marks the gap before continuing with available entries

#### Scenario: Export without a path uses an incompatible mode
- **WHEN** an operator runs `logs export` without `--output-path` in human or JSON mode
- **THEN** the CLI exits with code `2` before querying the daemon
- **AND** it instructs the operator to select JSONL or provide an output path

#### Scenario: Export range has expired
- **WHEN** retention removed part of the requested export range
- **THEN** the export contains a versioned gap record followed by available records
- **AND** the process exits with code `5` without claiming a complete export

#### Scenario: Export destination cannot be replaced
- **WHEN** the complete log export cannot be atomically written to the requested path
- **THEN** the original destination remains unchanged and the command returns the invalid-input or permission classification appropriate to the failure

### Requirement: CLI-010: Platform-specific IIS and shim setup behavior
On Windows, IIS mutation SHALL require a current immutable preview plan: `iis preview handoff` or `iis preview restore` returns a single-use plan ID valid for five minutes, and `iis apply` or `iis restore` accepts only the matching unexpired plan ID. On non-Windows platforms, `iis status` SHALL succeed with an explicit unsupported status, while preview and mutation commands SHALL fail as unsupported preconditions. `setup shim` SHALL invoke the ownership and collision policy defined by `REG-007`; a missing or non-PATH destination returns code `2`, unsafe ownership or filesystem permission returns code `4`, a collision or provenance mismatch returns code `5`, and success returns code `0`.

#### Scenario: IIS plan is applied on Windows
- **WHEN** an operator previews a handoff, reviews the returned plan, and invokes `cadder iis apply <plan-id>` before the plan expires
- **THEN** the CLI submits exactly that plan for scoped elevation and reports the authenticated apply result

#### Scenario: Stale IIS plan is rejected
- **WHEN** an operator supplies a stale, changed, or unknown plan ID to `iis apply` or `iis restore`
- **THEN** the command exits with code `5` and no privileged mutation runs

#### Scenario: IIS mutation is requested on another platform
- **WHEN** an operator invokes IIS preview, apply, or restore on Linux or macOS
- **THEN** the command exits with code `5`, identifies IIS as Windows-only, and leaves the runtime unchanged

#### Scenario: Shim alias collides with an existing executable
- **WHEN** `cadder setup shim` finds a `caddy` command that is not a verified Cadder-owned alias
- **THEN** it returns the `REG-007` conflict as exit code `5`
- **AND** standard error or the machine error identifies the conflicting path
