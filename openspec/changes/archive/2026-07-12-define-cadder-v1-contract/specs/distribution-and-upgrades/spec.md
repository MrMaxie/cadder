## ADDED Requirements

### Requirement: DST-001: Supported release application
Cadder 1.0 SHALL ship as one release application with exactly three Cadder executable entrypoints: `cadder`, `cadderd`, and `cadder-caddy`.

The release application SHALL support Windows x64, Linux x64, macOS x64, and macOS arm64. It MUST NOT bundle the real Caddy executable.

#### Scenario: Complete bundle on every supported platform
- **WHEN** a release bundle is inspected for any supported target
- **THEN** it contains the platform-appropriate forms of `cadder`, `cadderd`, and `cadder-caddy`
- **AND** it does not contain an executable presented as the real Caddy server

#### Scenario: Unsupported target request
- **WHEN** a user requests an official 1.0 artifact for a target outside the supported matrix
- **THEN** the release surface identifies that target as unsupported instead of presenting an unverified artifact

### Requirement: DST-002: Supported artifact and installer forms
The Cadder 1.0 release SHALL provide portable archives for every supported platform, a per-user shell installer for supported Unix platforms, a per-user PowerShell installer and per-user MSI for Windows x64, and a Homebrew installation path for supported macOS targets when the selected prefix is owned by the installing user.

Shared multi-user installation prefixes, per-machine MSI, DEB, RPM, PKG, and an in-application updater MUST NOT be presented as supported Cadder 1.0 distribution forms.

#### Scenario: Platform-appropriate installation choices
- **WHEN** a user views the 1.0 downloads for a supported platform
- **THEN** the release presents only the portable and installer forms supported for that platform

#### Scenario: Excluded package formats
- **WHEN** release assets and documentation are checked for the 1.0 release
- **THEN** no DEB, RPM, or PKG artifact is advertised as a supported Cadder package

### Requirement: DST-003: Installers do not shadow real Caddy
Every Cadder distribution SHALL install the compatibility shim under the non-conflicting name `cadder-caddy` and MUST NOT create a PATH-facing `caddy` alias during installation or upgrade. Alias creation and removal SHALL occur only through the explicit operator command and policy defined by `REG-007`.

#### Scenario: Fresh installer preserves the Caddy command
- **WHEN** a supported installer completes on a machine with or without real Caddy
- **THEN** `cadder-caddy` is available in the Cadder installation
- **AND** the installer leaves every existing or absent `caddy` command unchanged

#### Scenario: Upgrade preserves explicit alias ownership
- **WHEN** an installation with a verified Cadder-owned alias is upgraded
- **THEN** the installer preserves its provenance for `REG-007` validation
- **AND** it does not create a new alias in another destination

### Requirement: DST-004: Install, upgrade, and uninstall lifecycle
Each installer-managed path SHALL install and manage one binary set owned by the current operating-system user and SHALL provide deterministic installation, compatible in-place upgrade, and uninstall behavior that changes only that user's Cadder-owned resources. It MUST NOT replace a shared machine-wide binary set or control another user's daemon. A portable archive SHALL include a versioned manifest of its three binaries and SHALL document manual daemon shutdown, autostart disablement, verified alias removal, binary replacement, and removal from the user-selected extraction directory; it MUST NOT claim installer-managed cleanup.

Before replacing or removing binaries, an installer-managed lifecycle operation MUST request a bounded stop of each affected profile owned by the installing user and each profile's owned Caddy child. If ownership or shutdown cannot be proven, the operation SHALL fail with recovery guidance. An upgrade MUST verify the complete new application, replace the three binaries as one coherent version, and restore the previous set if replacement fails. Compatible upgrades MUST preserve trusted user configuration, runtime data, and verified shim provenance. Uninstall SHALL remove installed Cadder binaries, Cadder-owned aliases, and Cadder-owned startup entries while preserving user data unless the user explicitly requests a purge. No lifecycle operation may remove or modify the independently installed real Caddy executable.

#### Scenario: Fresh installation
- **WHEN** a user installs Cadder through a supported installer
- **THEN** all three Cadder entrypoints are available in the documented installation location
- **AND** the installer does not silently claim the `caddy` command

#### Scenario: Shared prefix is requested
- **WHEN** an installer or Homebrew operation selects a machine-wide or multi-user prefix not owned by the current user
- **THEN** Cadder reports that the installation scope is unsupported for 1.0
- **AND** it does not replace binaries or contact another user's runtime

#### Scenario: Compatible upgrade
- **WHEN** a user upgrades an existing compatible Cadder installation
- **THEN** the lifecycle operation drains the affected daemon and owned Caddy child before replacement
- **AND** the installed binaries move to the requested version as one coherent application
- **AND** trusted configuration, runtime data, and owned shim provenance remain usable

#### Scenario: Upgrade replacement fails
- **WHEN** any of the three verified replacement binaries cannot be installed
- **THEN** the lifecycle operation restores the complete previous binary set
- **AND** it reports failure without leaving a mixed-version installation

#### Scenario: Standard uninstall
- **WHEN** a user uninstalls Cadder without requesting data purge
- **THEN** the lifecycle operation stops the affected owned processes and removes Cadder-owned binaries, aliases, and startup entries
- **AND** user configuration, runtime data, and the real Caddy installation remain unchanged

#### Scenario: Portable archive is removed
- **WHEN** a user follows the portable removal instructions
- **THEN** the instructions stop the owned runtime, disable owned autostart, remove a verified Cadder alias, and identify the three files to remove from that extraction directory
- **AND** they preserve configuration, runtime data, unowned files, and real Caddy

### Requirement: DST-005: Apache-2.0 release identity
Every Cadder package manifest, installer metadata record, release archive, and public license statement SHALL identify Cadder as licensed under Apache-2.0.

Each release bundle MUST include the canonical Apache-2.0 license text.

#### Scenario: License inspection
- **WHEN** a user or automated verifier inspects any Cadder 1.0 bundle and its release metadata
- **THEN** all license identifiers resolve to Apache-2.0
- **AND** the bundle contains the canonical license text

#### Scenario: Conflicting license metadata
- **WHEN** any release input or generated artifact declares a different project license
- **THEN** the release gate fails before publication and identifies the conflicting artifact

### Requirement: DST-006: Integrity, SBOM, and provenance
Each public Cadder release SHALL provide SHA-256 checksums for binary archives and native installers, a CycloneDX software bill of materials for the release application, and verifiable build provenance covering every published release asset.

The version reported by the three binaries MUST match the release tag, archive identity, installer metadata, SBOM, and provenance subject.

#### Scenario: Release integrity verification
- **WHEN** a user verifies a downloaded archive or native installer
- **THEN** its SHA-256 digest matches the checksum published by the same release
- **AND** its provenance resolves to the tagged Cadder source revision

#### Scenario: Version identity mismatch
- **WHEN** a binary, artifact name, installer record, SBOM, or provenance subject reports a version different from the release tag
- **THEN** the release gate rejects the complete release set before publication

### Requirement: DST-007: Platform signing gate
Cadder SHALL publish Windows executables and MSI packages only with valid Authenticode signatures, and SHALL publish macOS executables and installation artifacts only after valid code signing and notarization.

Signature readiness flags or the presence of credentials MUST NOT substitute for cryptographic verification of the produced artifacts.

#### Scenario: Signed Windows release candidate
- **WHEN** a Windows release candidate reaches the publication gate
- **THEN** every shipped executable and MSI has a valid, trusted Authenticode signature

#### Scenario: Notarized macOS release candidate
- **WHEN** a macOS release candidate reaches the publication gate
- **THEN** every shipped executable has a valid code signature
- **AND** the installation artifact passes notarization verification

#### Scenario: Missing or invalid signature
- **WHEN** a required signature or notarization result is absent, expired, untrusted, or invalid
- **THEN** publication fails and no asset from that release candidate is made public

### Requirement: DST-008: Immutable 1.0 publication gate
Cadder MUST NOT publish version `1.0.0`, a final release tag, release assets, installer channels, or public documentation until every accepted 1.0 requirement has passing evidence and the complete release matrix passes for the same source revision.

Published release assets SHALL be immutable. A later workflow run MUST fail rather than overwrite an asset with the same release identity.

#### Scenario: Incomplete implementation evidence
- **WHEN** any requirement verification, platform build, installation lifecycle test, security check, documentation check, or signing check is incomplete or failing
- **THEN** the release remains unpublished
- **AND** the gate reports the missing evidence

#### Scenario: Verified release publication
- **WHEN** all requirement, release, documentation, security, signing, and platform evidence passes for the same source revision
- **THEN** the pipeline can publish one immutable Cadder 1.0 release set

#### Scenario: Attempted asset replacement
- **WHEN** a publication run encounters an existing asset with the same release identity
- **THEN** the run fails without replacing the existing asset
