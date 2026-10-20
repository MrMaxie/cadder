# Project registration

## Purpose
Define live shim-owned entrypoints and exclusive domains.

## Requirements

### Requirement: REG-001: Registration belongs to one live shim session
Managed run SHALL register a validated entrypoint owned by a session nonce, renew it through heartbeat, reconnect safely and unregister on clean exit or lost ownership. Ctrl+C, expired heartbeat and daemon restart SHALL not leave a stale active route. Another session SHALL NOT alter the current owner's lease.

#### Scenario: Old shim detaches a replacement session
- **WHEN** a stale shim sends heartbeat or unregister for a replacement owner
- **THEN** the daemon SHALL reject it and preserve current registration

### Requirement: REG-002: Domains have one active owner
Canonical domain ownership MUST be exclusive across active entrypoints, regardless of input case or international-domain spelling.

#### Scenario: Two projects request one domain
- **WHEN** a second active entrypoint requests a domain already owned by another
- **THEN** Cadder preserves the current owner and rejects the conflicting change

### Requirement: REG-003: Durable intent is not a live lease
Cadder MAY restore stable entrypoint identity and desired activation after restart but MUST NOT restore a lease, session nonce, process owner, or active route as live.

#### Scenario: Daemon restarts
- **WHEN** SQLite contains a previously committed entrypoint
- **THEN** it remains inactive until a current shim registers it

### Requirement: REG-004: Existing project formats and controls remain compatible
The Node product SHALL preserve existing cadder.toml and Caddyfiles, documented configuration precedence, multi-project registration, domain filtering and activation workflows. It SHALL validate inputs through the shared contract and reject active domain conflicts without replacing the last committed owner.

#### Scenario: Existing project is registered after transition
- **WHEN** the user registers an existing configuration through Node managed run
- **THEN** the daemon SHALL accept its retained format without a project rewrite
- **AND** active domain conflicts SHALL preserve the previous committed state
