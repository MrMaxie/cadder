## MODIFIED Requirements

### Requirement: RUN-001: Runtime identity belongs to one installation owner
One extracted Cadder installation SHALL own one runtime identity and SHALL admit only its operating-system owner through the protected local endpoint and files. Cadder 1.0 SHALL NOT expose named runtime profiles.

#### Scenario: Installation starts its runtime
- **WHEN** the installation owner starts `cadderd` directly, through the TUI, or through managed `caddy run`
- **THEN** every entrypoint derives the same runtime identity from that installation directory

#### Scenario: Different user attempts attachment
- **WHEN** another operating-system user attempts to attach to the runtime
- **THEN** endpoint authentication rejects the connection before protocol dispatch

### Requirement: RUN-002: Daemon lifecycle is explicit
The TUI SHALL expose explicit Start, Stop, and Restart actions. The managed `caddy run` shim SHALL start a missing daemon only because registration requires it. No read-only shim command, help invocation, version invocation, or ordinary TUI exit SHALL start or stop runtime processes.

#### Scenario: Explicit TUI start
- **WHEN** an operator selects Start while no daemon is ready
- **THEN** Cadder launches `cadderd` from the same archive and reports running only after readiness

#### Scenario: Managed run requires a daemon
- **WHEN** supported `caddy run` cannot attach to a daemon
- **THEN** the shim performs the same archive-owned launch and readiness path before registration

#### Scenario: Explicit TUI restart
- **WHEN** an operator confirms Restart
- **THEN** Cadder observes bounded shutdown before launching and confirming the replacement daemon

### Requirement: RUN-003: An installation runtime admits one live daemon
Each installation runtime MUST admit only one live daemon by exclusively owning its local IPC endpoint for the daemon lifetime.

#### Scenario: Concurrent start
- **WHEN** two start attempts race for one installation runtime
- **THEN** one daemon acquires the endpoint and the other observes the ready owner without replacing its state

#### Scenario: Stale Unix endpoint
- **WHEN** a socket remains after its daemon exits and no owner answers readiness
- **THEN** Cadder removes only that stale socket and retries the exclusive bind

### Requirement: RUN-005: Shutdown drains in-flight work
The daemon SHALL stop accepting mutations, finish or cancel owned handlers and background work, request bounded shutdown of its owned Caddy child, flush durable state, and release its endpoint. The complete normal shutdown budget SHALL NOT exceed 30 seconds. Exceeding a phase SHALL produce a diagnostic failure while teardown continues.

#### Scenario: TUI requests Stop
- **WHEN** the authenticated runtime owner confirms Stop
- **THEN** the daemon acknowledges shutdown, drains within the bounded timeline, and releases the endpoint

#### Scenario: Work exceeds its shutdown phase
- **WHEN** an owned request, Caddy stop, storage operation, or response write exceeds its phase
- **THEN** Cadder cancels or escalates only that owned work and continues teardown

### Requirement: RUN-007: Degraded state remains inspectable
The daemon MUST expose a typed degraded state when storage, Caddy, trusted configuration, or recovery fails. It SHALL preserve bounded Status and Logs inspection while rejecting only mutations that cannot be performed safely.

#### Scenario: Runtime recovery fails
- **WHEN** Cadder cannot restore a verified Caddy configuration or open authoritative storage
- **THEN** the TUI identifies the degraded subsystem and presents its user-relevant recovery action

#### Scenario: Degraded cause clears
- **WHEN** the operator completes recovery and the daemon verifies healthy state
- **THEN** Status returns to healthy and safe mutations become available

### Requirement: RUN-008: Trusted configuration has one explicit precedence
The runtime SHALL load one immutable trusted configuration at startup without profile overlays. Real-Caddy selection SHALL use an explicit foreground-daemon override first, then an owner-protected `cadder.toml` beside the Cadder executables, standard owner configuration, standard system configuration, and finally safe native PATH discovery. Project files and registration working directories MUST NOT participate. Unknown keys, unsafe files, conflicting selectors, and non-native executables MUST fail before readiness.

#### Scenario: Portable configuration selects real Caddy
- **WHEN** an owner-protected `cadder.toml` beside the three Cadder executables selects one valid real-Caddy source
- **THEN** the daemon pins that source for its lifetime

#### Scenario: Project attempts runtime configuration
- **WHEN** a project-local file or working directory contains a real-Caddy selector
- **THEN** startup and registration ignore it as an untrusted runtime source

#### Scenario: Trusted file changes while running
- **WHEN** trusted configuration changes after startup
- **THEN** the daemon keeps its pinned snapshot until explicit Restart

## REMOVED Requirements

### Requirement: RUN-006: Autostart is explicit and reversible
**Reason**: Cadder 1.0 starts the daemon through managed run or an explicit TUI action; platform startup integration has no retained user journey.

**Migration**: Start the daemon from `cadder tui` or allow the first managed `caddy run` to start it.

## RENAMED Requirements

- FROM: `### Requirement: RUN-001: Runtime identity is per user and profile`
- TO: `### Requirement: RUN-001: Runtime identity belongs to one installation owner`
- FROM: `### Requirement: RUN-003: A profile admits one live daemon`
- TO: `### Requirement: RUN-003: An installation runtime admits one live daemon`
