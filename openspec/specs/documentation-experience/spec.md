# Documentation experience

## Purpose
Define accurate, audience-scoped documentation for users and contributors.

## Requirements

### Requirement: DOC-001: User documentation follows the retained journey
Astro/Starlight documentation SHALL describe verified Node routes-first TUI, existing CLI lifecycle/project/domain/inspection commands, bounded diagnostic logs and coherent transition workflows in English, without exposing private/test-only context. Published 2.0 instructions SHALL only advertise flows that pass actual product acceptance.

#### Scenario: User follows a Node guide
- **WHEN** a user follows a documented 2.0 workflow
- **THEN** commands SHALL exist in the tested product rather than fixture handlers
- **AND** old Rust data SHALL remain protected during the documented transition

### Requirement: DOC-002: Contributor tooling is direct and reproducible
Contributor documentation SHALL use the exact Node/Nub TS, OpenSpec, Astro, coverage and npm/SEA workflow, replacing Bun and obsolete Rust tasks after the cutover gate. Sources SHALL remain TS/TSX and generated artifacts SHALL not be committed. SEA's separate Node builder SHALL not change npm's Node minimum.

#### Scenario: Contributor prepares artifacts
- **WHEN** a contributor follows the documented pack verification workflow
- **THEN** both channels SHALL use the shared sources and outside-checkout staging
- **AND** no loader/build SHALL be required during consumer npm installation

### Requirement: DOC-003: Private context does not leak
Published documentation MUST NOT contain personal absolute paths, `.local`, credentials, mock-only commands, or agent-only operational details.

#### Scenario: Documentation is built
- **WHEN** Astro validates the site
- **THEN** only portable reader-facing content is included

### Requirement: DOC-004: Installation guidance distinguishes npm, Cadder, and Caddy
Verified npm and standalone GitHub downloads SHALL be presented as equal channels with the same functionality/version. npm SHALL document Node 24 LTS minimum 24.18.0; standalone SHALL require no installed Node/npm. Both SHALL identify upstream Caddy as separate. Unpublished/unverified versions SHALL not be presented as available, and SEA's mechanism status SHALL not demote its channel.

#### Scenario: Consumer chooses standalone
- **WHEN** the consumer selects a supported GitHub archive instead of npm
- **THEN** instructions SHALL offer the same product workflow without Node installation
- **AND** neither channel SHALL be described as secondary or optional
