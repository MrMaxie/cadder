## MODIFIED Requirements

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
