## MODIFIED Requirements

### Requirement: CADDY-001: Real Caddy comes only from trusted sources
The daemon SHALL select real Caddy from a trusted explicit override/configuration or safe PATH, pin the executable for its lifetime and reject untrusted project/shim selectors. Resolution SHALL recognize npm wrappers, SEA entries, aliases and file identity to avoid recursive Cadder execution.

#### Scenario: PATH starts with the npm or SEA shim
- **WHEN** an executable candidate resolves to a Cadder wrapper or entrypoint
- **THEN** resolution SHALL skip or reject it and continue safely or fail clearly
- **AND** it SHALL NOT recursively launch the shim

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

## ADDED Requirements

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
