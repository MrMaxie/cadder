## MODIFIED Requirements

### Requirement: DST-001: One release application
Cadder 1.0 SHALL ship as one release application with exactly three executable entrypoints: `cadder`, `cadderd`, and the PATH-facing `caddy` shim.

All three entrypoints MUST report the same Cadder version and SHALL be distributed together in every supported portable archive.

#### Scenario: Coherent release archive
- **WHEN** a supported Cadder archive is inspected
- **THEN** it contains the platform-appropriate forms of `cadder`, `cadderd`, and `caddy`
- **AND** all three report the release version

### Requirement: DST-002: Supported artifact and installer forms
The Cadder 1.0 release SHALL provide one versioned portable archive with a SHA-256 checksum for every supported platform.

Native installers, package-manager formulae, shared multi-user installation prefixes, MSI, DEB, RPM, PKG, shell installers, PowerShell installers, Homebrew formulae, and an in-application updater MUST NOT be presented as supported Cadder 1.0 distribution forms.

#### Scenario: Platform-appropriate download
- **WHEN** a user views the 1.0 downloads for a supported platform
- **THEN** the release presents the portable archive and matching SHA-256 checksum for that platform

#### Scenario: Excluded installation forms
- **WHEN** release assets and documentation are checked for the 1.0 release
- **THEN** no native installer or package-manager artifact is advertised as a supported Cadder package

### Requirement: DST-004: Install, upgrade, and uninstall lifecycle
Each portable archive SHALL contain one coherent version of the three Cadder binaries, the license, and a sample configuration. The documentation SHALL describe manual extraction, daemon shutdown, autostart disablement, verified alias removal, coherent binary replacement, rollback using the previous archive, and removal from the user-selected extraction directory.

The documented lifecycle MUST preserve trusted user configuration, runtime data, unowned files, independently installed real Caddy binaries, and aliases that Cadder cannot prove it owns. It MUST NOT claim installer-managed cleanup or control another user's daemon.

#### Scenario: Fresh archive extraction
- **WHEN** a user extracts a supported Cadder archive
- **THEN** all three version-matched Cadder entrypoints and the sample configuration are available in the selected directory
- **AND** extraction does not create or replace a PATH-facing `caddy` alias

#### Scenario: Compatible manual upgrade
- **WHEN** a user follows the documented upgrade procedure
- **THEN** the procedure stops the owned runtime before all three binaries are replaced as one version
- **AND** it preserves trusted configuration, runtime data, and verified shim provenance

#### Scenario: Upgrade replacement fails
- **WHEN** the complete replacement binary set cannot be installed
- **THEN** the documentation directs the user to restore all three binaries from the previous archive
- **AND** it does not present a mixed-version installation as valid

#### Scenario: Portable archive is removed
- **WHEN** a user follows the portable removal instructions
- **THEN** the instructions stop the owned runtime, disable owned autostart, remove a verified Cadder alias, and identify the extracted Cadder files to remove
- **AND** they preserve configuration, runtime data, unowned files, and real Caddy

### Requirement: DST-005: Apache-2.0 release identity
Every Cadder Cargo package manifest, portable release archive, and public license statement SHALL identify Cadder as licensed under Apache-2.0. Each archive MUST include the canonical Apache-2.0 license text.

#### Scenario: License inspection
- **WHEN** a user or automated verifier inspects a Cadder 1.0 archive and its release metadata
- **THEN** all license identifiers resolve to Apache-2.0
- **AND** the archive contains the canonical license text

#### Scenario: Conflicting license metadata
- **WHEN** any package manifest, release input, archive, or public statement declares a different project license
- **THEN** the release gate fails before publication and identifies the conflicting artifact

### Requirement: DST-006: Integrity, SBOM, and provenance
Each public Cadder release SHALL provide a SHA-256 checksum for every portable archive.

The version reported by the three binaries MUST match the release tag and archive name.

#### Scenario: Release integrity verification
- **WHEN** a user verifies a downloaded archive
- **THEN** its SHA-256 digest matches the checksum published by the same release

#### Scenario: Version identity mismatch
- **WHEN** a binary or archive name reports a version different from the release tag
- **THEN** the release gate rejects the complete release set before publication

### Requirement: DST-008: Immutable 1.0 publication gate
Cadder MUST NOT publish version `1.0.0`, a final release tag, release archives, or public release documentation until the four supported archives build from the same source revision and their archive contents, version identity, SHA-256 checksums, license metadata, and documentation checks pass.

Published release assets SHALL be immutable. A later workflow run MUST fail rather than overwrite an asset with the same release identity.

#### Scenario: Incomplete release evidence
- **WHEN** a supported archive build, archive verification, checksum, version identity, license, or documentation check is incomplete or failing
- **THEN** the release remains unpublished
- **AND** the gate reports the missing evidence

#### Scenario: Verified release publication
- **WHEN** the complete portable archive matrix and release documentation pass for the same source revision
- **THEN** the pipeline can publish one immutable Cadder 1.0 release set

#### Scenario: Attempted asset replacement
- **WHEN** a publication run encounters an existing asset with the same release identity
- **THEN** the run fails without replacing the existing asset

## REMOVED Requirements

### Requirement: DST-003: Installers do not shadow real Caddy
**Reason**: Cadder 1.0 no longer provides installer-managed distribution paths; explicit shim alias behavior remains defined by `REG-007`.

**Migration**: Users extract the portable archive and run the explicit operator command when they want Cadder to create or remove a `caddy` alias.

### Requirement: DST-007: Platform signing gate
**Reason**: Mandatory platform signing and notarization require release infrastructure that is disproportionate to the portable-archive-only 1.0 scope.

**Migration**: Cadder 1.0 publishes SHA-256 checksums for every archive. Platform signing can return in a later distribution change when native installers or a broader release channel justify it.

## RENAMED Requirements

- FROM: `### Requirement: DST-001: Supported release application`
- TO: `### Requirement: DST-001: One release application`
