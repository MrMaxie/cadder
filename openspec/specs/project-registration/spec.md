# Project registration

## Purpose
Define live shim-owned entrypoints and exclusive domains.

## Requirements

### Requirement: REG-001: Registration belongs to one live shim session
Each managed run MUST register a validated entrypoint with a session nonce, renew it by heartbeat, reconnect safely, and unregister it on clean exit or connection loss.

#### Scenario: Heartbeat owner differs
- **WHEN** a heartbeat or detach uses the wrong session nonce
- **THEN** Cadder rejects it without changing the registration

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
