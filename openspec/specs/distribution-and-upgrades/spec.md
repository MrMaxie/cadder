# Distribution and upgrades

## Purpose
Define maintained portable releases and coherent manual upgrades.
## Requirements
### Requirement: DIST-001: Release binaries are version matched
Cadder SHALL have equal, required npm and Node SEA GitHub Release channels with the same version and cadder/cadderd/caddy functionality. Standalone targets SHALL include Windows x64, Linux x64, macOS x64 and macOS arm64. Both channels SHALL pass acceptance before migration completion or final release readiness.

#### Scenario: One channel is ready
- **WHEN** npm passes but any supported standalone variant remains unverified
- **THEN** the migration SHALL remain incomplete rather than treating SEA as optional

### Requirement: DIST-002: Archives are portable and verifiable
Each standalone archive SHALL contain three version-matched Node SEA entries and required accepted assets, a SHA-256 checksum and release-source provenance. Every target SHALL be extracted outside checkout and pass native CLI/TUI, state/log SQLite worker and owned subprocess smoke without Node/npm on PATH. Standalone SHALL NOT require installed Node, download runtime components or bundle real Caddy. Existing release attestation verification SHALL bind assets to their exact tag/source commit/repository/signer before publication.

#### Scenario: Consumer has no Node installation
- **WHEN** a supported standalone archive is used with Node/npm absent from PATH
- **THEN** all three entries, TUI, SQLite worker and owned child workflows SHALL work
- **AND** checksum/provenance verification SHALL apply to the exact tested archive

### Requirement: DIST-003: Upgrades replace the coherent executable set
Transition guidance SHALL require stopping the old daemon before switching the coherent release set. Node SHALL re-register projects from existing files without importing or deleting old runtime data. Old releases SHALL remain available for rollback without automatic deprecation. Mixed Rust/Node IPC SHALL not be supported. Transition SHALL NOT require IIS handoff or autostart operations absent from the released product.

#### Scenario: User switches to Node and later rolls back
- **WHEN** the user follows the documented transition with each daemon stopped
- **THEN** existing configuration and old data/releases SHALL remain available
- **AND** rollback SHALL use the matched old set, not mixed IPC or imported old data

### Requirement: DIST-004: npm installs one Node application package
npm SHALL deliver one cadder tarball with three bin entries on Node 24 LTS, minimum 24.18.0. It SHALL contain compiled JS and required assets from the shared TS sources, not platform optionalDependencies or native Rust executables. Installation SHALL require no TS loader/compiler, install-time build, postinstall runtime download or bundled upstream Caddy. Local/global installs SHALL preserve arguments, streams, status and non-recursive PATH behavior.

#### Scenario: Clean consumer installs the packed tarball
- **WHEN** a supported consumer installs locally or globally without build tools
- **THEN** cadder, cadderd and the caddy shim SHALL run from the one package
- **AND** upstream Caddy SHALL remain a separately configured prerequisite

### Requirement: DIST-005: npm and portable archives share release identity
npm and SEA SHALL be built from the same source revision with one matching release version and pass the same functional catalog. Their artifact bytes need not match because packaging differs. A verified 2.0.0-rc.1 in both channels SHALL precede 2.0.0 acceptance. Tagging and publication SHALL require separate explicit authorization; artifact preparation alone SHALL NOT publish either channel.

#### Scenario: Candidate channels are compared
- **WHEN** packed npm and extracted SEA are accepted for a candidate
- **THEN** source revision, reported version and observable functional outcomes SHALL match
- **AND** no publication SHALL occur without its separate authority
