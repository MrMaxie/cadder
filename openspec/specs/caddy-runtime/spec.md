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

### Requirement: CADDY-004: Owned listeners and routes remain local
Cadder SHALL bind its owned HTTP and HTTPS servers only to IPv4 and IPv6 loopback addresses on the existing ports. Each registration's complete adapted route tree MUST remain behind an outer matcher containing only that registration's active canonical hosts, and project input MUST NOT widen either boundary.

#### Scenario: Remote peer addresses the developer host
- **WHEN** a peer connects through a non-loopback interface and supplies an active `.localhost` host header
- **THEN** no Cadder-owned HTTP or HTTPS listener accepts the connection

#### Scenario: Adapted route contains a hostless sibling
- **WHEN** one retained branch matches an active host and a sibling branch has no host matcher
- **THEN** the complete route tree remains reachable only under the registration's active canonical hosts

### Requirement: CADDY-005: Project configuration adaptation is bounded
Cadder MUST capture at most 32 MiB from each `caddy adapt` output stream, reject partial adaptation output when either limit is exceeded, and retain bounded cleanup for any adapter process tree that remains active.

#### Scenario: Adapter exceeds an output limit
- **WHEN** `caddy adapt` writes more than 32 MiB to stdout or stderr
- **THEN** Cadder reports a bounded failure without applying configuration and cleans up the owned adapter process tree if it remains active
