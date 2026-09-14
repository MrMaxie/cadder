# Product topology

## Purpose
Define the minimal Cadder 1.0 product boundary, executable set, and separation between everyday and diagnostic surfaces.

## Requirements

### Requirement: TOP-001: Three executables form one installation
Cadder SHALL ship version-matched `cadder`, `cadderd`, and `caddy` executables that derive one runtime identity from their installation directory.

#### Scenario: Separate installations
- **WHEN** the same user runs executables from two different installation directories
- **THEN** each installation uses an independent endpoint and database

### Requirement: TOP-002: Product and diagnostic surfaces remain distinct
Managed `caddy run` and `cadder tui` SHALL be the user-facing 1.0 surfaces, while foreground `cadderd` and detailed logs remain diagnostic surfaces.

#### Scenario: User opens the operator
- **WHEN** the user runs `cadder tui`
- **THEN** Cadder opens the Status, Domains, and Logs interface
