# Caddy runtime

## Purpose
Define trusted real-Caddy resolution, route composition, and process ownership.

## Requirements

### Requirement: CADDY-001: Real Caddy comes only from trusted sources
Cadder MUST resolve real Caddy from an explicit foreground override, trusted configuration, or safe native PATH discovery that excludes the shim by file identity.

#### Scenario: Project attempts selection
- **WHEN** a project file, working directory, environment value, or shim flag attempts to select real Caddy
- **THEN** the daemon ignores or rejects that untrusted selector

### Requirement: CADDY-002: One owned process serves composed routes
The daemon SHALL adapt validated project Caddyfiles, compose active non-conflicting routes, and apply them through the one real Caddy child it owns.

#### Scenario: Another Caddy process exists
- **WHEN** an unrelated Caddy process is running
- **THEN** Cadder neither adopts nor terminates it

### Requirement: CADDY-003: Runtime transitions are verified
A desired-state mutation MUST publish only after Caddy accepts the candidate configuration and SQLite commits it; failure SHALL retain or restore the previous verified runtime.

#### Scenario: Persistence fails after apply
- **WHEN** Caddy accepted a candidate but SQLite cannot commit it
- **THEN** Cadder rolls Caddy back and keeps the previous authoritative state
