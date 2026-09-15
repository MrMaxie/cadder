## ADDED Requirements

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
