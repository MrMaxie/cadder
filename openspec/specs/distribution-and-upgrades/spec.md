# Distribution and upgrades

## Purpose
Define maintained portable releases and coherent manual upgrades.
## Requirements
### Requirement: DIST-001: Release binaries are version matched
Cadder SHALL version and release the `cadder`, `cadderd`, and `caddy` executables together in one application archive for Windows x64, Linux x64, Intel macOS, and Apple Silicon macOS.

#### Scenario: Unified tag is planned
- **WHEN** cargo-dist plans tag `v1.0.0`
- **THEN** it includes one 1.0.0 application with all three executables on all four targets

### Requirement: DIST-002: Archives are portable and verifiable
Each cargo-dist archive MUST contain exactly the three expected executables, README, changelog, Apache-2.0 license, and sample configuration and MUST have a SHA-256 checksum.

Before publication, each archive MUST be extracted into a fresh directory outside the source checkout on its native operating system. The verifier MUST check the checksum, exact file set, executable permissions where applicable, and successful `--version` and `--help` behavior for all three executables. Published archives, checksums, source archive, and cargo-dist manifest MUST carry GitHub artifact attestations from the release workflow.

#### Scenario: User downloads a release
- **WHEN** an archive and checksum are obtained from one release
- **THEN** the checksum can be verified before extraction without an installer
- **AND** a GitHub attestation can associate the asset with the release workflow

#### Scenario: Release archive is verified
- **WHEN** a release archive is evaluated before publication
- **THEN** only the accepted binaries and static release files are present
- **AND** each executable reports the planned version and exposes help outside the source checkout
- **AND** Unix executables retain their executable permissions

### Requirement: DIST-003: Upgrades replace the coherent executable set
Public guidance MUST instruct the user to stop through the TUI, replace all three executables from one release, and restart through managed run or the TUI.

#### Scenario: Mixed versions connect
- **WHEN** only part of the executable set was replaced
- **THEN** the exact handshake rejects the mismatch before mutation

### Requirement: DIST-004: npm installs the coherent native application

Cadder SHALL publish one public `cadder` launcher package and one platform package for each supported release target. Every package in one release MUST use the same version, and the root package MUST depend on the matching platform packages by exact optional dependency versions.

The root package MUST expose `cadder`, `cadderd`, and the Cadder `caddy` shim. It MUST select only the package matching the current operating system, architecture, and C runtime, launch the native executable without a shell, preserve standard streams and exit status, and report an actionable error for an unsupported platform or omitted optional dependency.

The npm installation MUST NOT bundle, download, replace, or claim to provide the independently installed upstream Caddy server.

#### Scenario: One-off operator invocation

- **WHEN** a user runs `npx cadder` on a supported platform with optional dependencies enabled
- **THEN** the matching native `cadder` executable runs with the requested arguments
- **AND** its version matches the root and platform package versions

#### Scenario: Global installation

- **WHEN** a user installs `cadder` globally on a supported platform
- **THEN** `cadder`, `cadderd`, and the Cadder `caddy` shim are available as commands
- **AND** all three native executables report the same release version

#### Scenario: Upstream Caddy remains separate

- **WHEN** the npm-installed Cadder shim needs a real Caddy executable
- **THEN** the npm launcher cannot be selected as upstream Caddy
- **AND** real Caddy resolution continues through Cadder's existing trusted sources

#### Scenario: Optional package is unavailable

- **WHEN** the platform package was omitted or the current platform is unsupported
- **THEN** the launcher exits unsuccessfully without downloading or compiling an executable
- **AND** the diagnostic identifies the missing package or unsupported target

### Requirement: DIST-005: npm and portable archives share release identity

Every npm platform package SHALL contain the exact three executable files taken from the corresponding verified cargo-dist release archive. npm packaging MUST preserve executable permissions where applicable and MUST NOT alter the native executable bytes.

#### Scenario: Release channel comparison

- **WHEN** a native executable from an npm platform package is compared with the corresponding verified GitHub Release archive
- **THEN** their cryptographic digests match
- **AND** both report the same Cadder release version
