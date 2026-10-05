## MODIFIED Requirements

### Requirement: REG-001: Registration belongs to one live shim session
Managed run SHALL register a validated entrypoint owned by a session nonce, renew it through heartbeat, reconnect safely and unregister on clean exit or lost ownership. Ctrl+C, expired heartbeat and daemon restart SHALL not leave a stale active route. Another session SHALL NOT alter the current owner's lease.

#### Scenario: Old shim detaches a replacement session
- **WHEN** a stale shim sends heartbeat or unregister for a replacement owner
- **THEN** the daemon SHALL reject it and preserve current registration

## ADDED Requirements

### Requirement: REG-004: Existing project formats and controls remain compatible
The Node product SHALL preserve existing cadder.toml and Caddyfiles, documented configuration precedence, multi-project registration, domain filtering and activation workflows. It SHALL validate inputs through the shared contract and reject active domain conflicts without replacing the last committed owner.

#### Scenario: Existing project is registered after transition
- **WHEN** the user registers an existing configuration through Node managed run
- **THEN** the daemon SHALL accept its retained format without a project rewrite
- **AND** active domain conflicts SHALL preserve the previous committed state
