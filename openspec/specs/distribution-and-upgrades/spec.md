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
Each cargo-dist archive MUST contain the three expected executables, README, changelog, Apache-2.0 license, and sample configuration and MUST have a SHA-256 checksum.

#### Scenario: User downloads a release
- **WHEN** an archive and checksum are obtained from one release
- **THEN** the checksum can be verified before extraction without an installer

### Requirement: DIST-003: Upgrades replace the coherent executable set
Public guidance MUST instruct the user to stop through the TUI, replace all three executables from one release, and restart through managed run or the TUI.

#### Scenario: Mixed versions connect
- **WHEN** only part of the executable set was replaced
- **THEN** the exact handshake rejects the mismatch before mutation
