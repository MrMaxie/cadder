## ADDED Requirements

### Requirement: Logs are structured and labeled
`cadderd` SHALL capture daemon, shim, and real Caddy logs into a structured log
model with timestamp, source, severity, runtime, project, domain, and redaction
metadata where applicable.

#### Scenario: Caddy emits an access log
- **WHEN** real Caddy emits an access log for a Cadder-managed route
- **THEN** `cadderd` SHALL store or stream the log with project and domain labels
- **AND** clients SHALL be able to identify it as Caddy-sourced

#### Scenario: Shim emits a diagnostic
- **WHEN** the shim rejects a command or cannot contact the daemon
- **THEN** the diagnostic SHALL be available in the structured log stream when the daemon can receive it
- **AND** secrets SHALL be redacted before persistence

### Requirement: Logs can be queried by operator dimensions
Operator clients SHALL be able to query and tail logs by all logs, runtime,
project, domain, source, and severity.

#### Scenario: User tails all logs at debug level
- **WHEN** a user requests all logs with debug-level visibility
- **THEN** the client SHALL receive daemon, shim, and Caddy logs allowed by the retention policy
- **AND** the daemon SHALL apply one canonical severity filter

#### Scenario: User tails logs for one domain
- **WHEN** a user requests logs for a specific domain
- **THEN** the daemon SHALL return only log records mapped to that domain
- **AND** the result SHALL be correct even when one Caddy site contains multiple domains

### Requirement: Log filtering semantics are canonical
The daemon SHALL own severity and dimension filtering semantics so every client
surface displays equivalent results for the same query.

#### Scenario: CLI and TUI run equivalent queries
- **WHEN** CLI and TUI request the same project logs at the same severity
- **THEN** both clients SHALL receive equivalent records in the same canonical order
- **AND** differences SHALL be limited to presentation

#### Scenario: Invalid log level requested
- **WHEN** a client requests an unsupported log level
- **THEN** the daemon SHALL reject the query with a typed validation error
- **AND** the client SHALL display supported levels

### Requirement: Log retention is bounded and configurable
Cadder SHALL bound local log retention and SHALL expose retention status to
operator clients.

#### Scenario: Retention limit reached
- **WHEN** retained logs exceed the configured storage policy
- **THEN** `cadderd` SHALL prune or rotate logs according to the policy
- **AND** it SHALL preserve recent records needed for diagnostics
