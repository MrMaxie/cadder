# Caddy runtime

## Purpose
Define trusted real-Caddy resolution, route composition, and process ownership.

## Requirements

### Requirement: CADDY-001: Real Caddy comes only from trusted sources
The daemon SHALL select real Caddy from a trusted explicit override/configuration or safe PATH, pin the executable for its lifetime and reject untrusted project/shim selectors. Resolution SHALL recognize npm wrappers, SEA entries, aliases and file identity to avoid recursive Cadder execution.

#### Scenario: PATH starts with the npm or SEA shim
- **WHEN** an executable candidate resolves to a Cadder wrapper or entrypoint
- **THEN** resolution SHALL skip or reject it and continue safely or fail clearly
- **AND** it SHALL NOT recursively launch the shim

### Requirement: CADDY-002: One owned process serves composed routes
The daemon SHALL adapt validated project Caddyfiles, compose active non-conflicting routes, and apply them through the one real Caddy child it owns.

#### Scenario: Another Caddy process exists
- **WHEN** an unrelated Caddy process is running
- **THEN** Cadder neither adopts nor terminates it

### Requirement: CADDY-003: Runtime transitions are verified
One queue SHALL serialize preparation, adaptation/composition, validation, apply, active-state verification, SQLite commit and publication. Rejection SHALL retain last-known-good. Persistence failure after apply SHALL restore and verify the previous configuration; failed rollback SHALL fence further mutations. An ambiguous apply outcome SHALL require active-state reconciliation before the next mutation. Pending state SHALL NOT become authoritative prematurely.

#### Scenario: Apply response is lost
- **WHEN** the daemon cannot determine whether Caddy applied a candidate
- **THEN** it SHALL fence later changes and reconcile active state
- **AND** only verified, persisted state SHALL be published as committed

#### Scenario: Database commit fails after successful apply
- **WHEN** Caddy accepted a candidate but persistence fails
- **THEN** the daemon SHALL restore last-known-good and retain prior state
- **AND** failed restore SHALL remain fenced with a recoverable diagnostic

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

### Requirement: CADDY-006: Administration uses protected loopback mutual TLS
The daemon-Caddy admin channel SHALL use loopback mTLS with an internal CA, validated server identity and an owner-protected authorized client certificate. The CA SHALL NOT be installed in system trust. Plaintext local admin SHALL be disabled, and project input SHALL NOT override the administration policy.

#### Scenario: Caller has no authorized certificate
- **WHEN** a caller contacts Caddy admin without the correct client certificate
- **THEN** Caddy SHALL refuse administrative access
- **AND** there SHALL be no unprotected listener fallback

#### Scenario: Project supplies admin settings
- **WHEN** project configuration attempts to replace admin transport or trust
- **THEN** the daemon SHALL reject or override it with the protected policy
- **AND** no system trust-store mutation SHALL be needed
