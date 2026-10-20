## MODIFIED Requirements

### Requirement: QT-001: Declarative contributor environment
The Node product SHALL declare exact dependencies, tool versions and locks, managed by Nub, without private paths/credentials. TypeScript SHALL be 6.0.3; the npm-tested Node minimum SHALL be 24.18.0 on Node 24 LTS. SEA SHALL use a separately verified, exact Node 26 builder pin. Required tools SHALL work on Windows/Linux/macOS without a custom compiled task runner after cutover.

#### Scenario: Fresh Node-only checkout
- **WHEN** a contributor follows the declared setup after Rust removal
- **THEN** Node tests/build/docs/packaging SHALL work without Cargo/Rust or private setup

### Requirement: QT-002: One discoverable task surface
Node validation/build/pack tasks SHALL be discoverable as Nub package scripts, with focused commands and a complete check. Existing tool provisioning MAY remain as a thin adapter during cutover, not a competing policy framework. Custom scripts SHALL be limited to Cadder-specific packaging/acceptance seams.

#### Scenario: Contributor runs complete checks
- **WHEN** the documented Node check/pack verification commands run
- **THEN** each SHALL delegate to its owning maintained tool
- **AND** there SHALL be no replacement general-purpose custom command dispatcher

### Requirement: QT-003: Maintained tools own validation and release formats
tsc noEmit, ESLint, Prettier, Vitest/V8 and ink-testing-library SHALL own their checks; own TS/TSX line coverage SHALL be at least 85%, including hard runtime modules without selective exclusion. npm emission SHALL use tsc with relative import rewriting; SEA bundling SHALL use esbuild and the pinned Node builder. All generated Node JS/bundles/archives/coverage SHALL go to system or runner temp outside checkout, never beside source or into tracked files. Astro/Starlight SHALL own documentation checks/builds with Nub-managed dependencies.

#### Scenario: Pack verification runs
- **WHEN** the same source is checked and packed for either channel
- **THEN** the supported tools SHALL produce only outside-checkout Node artifacts
- **AND** tracked files SHALL remain unchanged with no emitted source-side JS

### Requirement: QT-004: Local and CI task parity
CI SHALL invoke the same checks and non-publishing packing process as local verification, with native Windows/Linux/macOS system/security integration and every SEA target. PR jobs SHALL not publish or receive publication credentials. Native exact-artifact verification SHALL precede attest/publish stages with least privilege and source/tag validation. Documentation examples SHALL be tested.

#### Scenario: Candidate PR is validated
- **WHEN** CI verifies a migration or release candidate
- **THEN** it SHALL check contracts, coverage, native product behavior and both channels
- **AND** artifact preparation SHALL NOT create a public release or registry publication

### Requirement: QT-005: Evidence-gated task-runner removal
The Rust/Cargo product and obsolete native npm/release packaging SHALL be removed only after complete Node functionality and both distributions pass acceptance at the candidate revision. Each old responsibility SHALL have a verified replacement or explicitly approved removal. Unrelated pre-existing untracked npm content SHALL be inspected and preserved. After removal, the full Node-only gate SHALL pass again without Rust/Cargo installed.

#### Scenario: Rust removal is proposed
- **WHEN** one required capability, platform or distribution still lacks evidence
- **THEN** Rust removal SHALL wait and migration SHALL remain incomplete
- **AND** a foundation checkpoint SHALL NOT be represented as the finished migration

### Requirement: QT-006: npm packages pass clean-room verification
The gate SHALL verify the actual single npm tarball and every extracted SEA archive outside checkout using the publication packing process without publishing. It SHALL verify exact allowed files, version/license/repository identity, all three entries, local/global npm installs, argument/stream/exit fidelity and no wrapper recursion. npm SHALL have no platform optional packages or install scripts; SEA SHALL pass real TUI/SQLite/worker/child smoke without Node/npm PATH.

#### Scenario: Clean consumer verification fails
- **WHEN** either channel fails its native packed-consumer catalog
- **THEN** neither the whole migration nor matching final release SHALL be accepted

### Requirement: QT-007: npm publication uses staged trusted publishing
After separate publication approval, the single npm package SHALL retain short-lived OIDC trusted publishing from the exact protected GitHub workflow, staged publishing and maintainer 2FA review, without long-lived npm tokens. The consumed candidate SHALL match the verified source revision/version and complete npm/SEA evidence. The obsolete native-platform-package approval order SHALL not remain a dependency of the new single package.

#### Scenario: Authorized candidate reaches publishing
- **WHEN** the separately approved matched candidate enters the publishing workflow
- **THEN** it SHALL stage the exact verified single package with trusted provenance
- **AND** a maintainer SHALL approve publication without an embedded persistent token
