## MODIFIED Requirements

### Requirement: DST-004: Install, upgrade, and uninstall lifecycle
Each portable archive SHALL contain one coherent version of `cadder`, `cadderd`, and the PATH-facing `caddy` shim, plus the license and sample configuration. Documentation SHALL describe manual extraction, daemon shutdown through the TUI, coherent binary replacement, rollback using the previous archive, and removal from the user-selected extraction directory.

The documented lifecycle MUST preserve trusted configuration, runtime data, unowned files, and independently installed real-Caddy binaries. It MUST NOT claim installer-managed cleanup, alias provenance, autostart management, or control of another user's runtime.

#### Scenario: Fresh archive extraction
- **WHEN** a user extracts a supported archive
- **THEN** all three version-matched executables and sample configuration are available in the selected directory

#### Scenario: Compatible manual upgrade
- **WHEN** a user follows the documented upgrade procedure
- **THEN** the procedure uses the TUI Stop action before replacing all three binaries as one version
- **AND** it preserves trusted configuration and runtime data

#### Scenario: Upgrade replacement fails
- **WHEN** the complete replacement set cannot be installed
- **THEN** documentation directs the user to restore all three binaries from the previous archive

#### Scenario: Portable archive is removed
- **WHEN** a user follows portable removal instructions
- **THEN** the instructions stop the owned runtime and identify only extracted Cadder files for removal
- **AND** they preserve configuration, runtime data, unowned files, and real Caddy
