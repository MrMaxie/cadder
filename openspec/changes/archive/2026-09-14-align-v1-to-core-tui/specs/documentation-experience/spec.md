## MODIFIED Requirements

### Requirement: DOC-002: Audience-directed information architecture
The documentation site SHALL organize content by user intent using distinct Getting Started, How-to Guides, Troubleshooting, Reference, and Explanation sections.

End-user managed-run and TUI guidance, operator foreground-daemon and recovery guidance, and contributor or release-maintainer procedures MUST appear at the level appropriate to that audience. Contributor and release procedures MUST NOT interrupt task-oriented user guidance.

#### Scenario: New user onboarding
- **WHEN** a new user opens the documentation
- **THEN** Getting Started provides the shortest path from portable archive extraction through configured real Caddy, managed `caddy run`, and successful TUI inspection

#### Scenario: Operator recovery
- **WHEN** an operator encounters a daemon, shim, Caddy, storage, or permission failure
- **THEN** Troubleshooting provides symptom-based recovery without requiring repository knowledge

#### Scenario: Contributor guidance
- **WHEN** a contributor needs build, test, architecture, or release-maintenance instructions
- **THEN** those instructions remain outside the end-user journey

### Requirement: DOC-004: Complete user journeys
Public documentation SHALL cover portable archive installation, trusted real-Caddy configuration, PATH selection of the bundled `caddy` shim, first managed project registration, Status, Domains, Logs, activation controls, daemon Start/Stop/Restart, manual upgrade, portable removal, foreground diagnosis, and recovery.

Each journey MUST state prerequisites, the expected result, relevant safety constraints, and task-relevant troubleshooting when it can fail. It SHALL NOT present unimplemented CLI commands, profiles, autostart, history, export, tail, alias setup, installer management, mock backends, or hidden flags as product behavior.

#### Scenario: First managed project
- **WHEN** a user follows the first-project journey
- **THEN** the user extracts one archive, configures real Caddy, puts that directory on PATH, runs supported `caddy run`, opens `cadder tui`, and sees the registered project

#### Scenario: Safe manual upgrade
- **WHEN** a user follows the upgrade journey
- **THEN** the user stops the owned runtime from the TUI before replacing all three binaries coherently
- **AND** the instructions preserve runtime data, trusted configuration, and independently installed real Caddy

### Requirement: DOC-005: Executable examples and generated reference
Every published Cadder command example SHALL be executable against the release candidate it documents. Public command reference SHALL be generated or verified from the same source revision as the binaries and MUST contain only help, version, `cadder tui`, the supported `caddy run` form, and explicitly documented foreground `cadderd` operation.

An invalid command, stale help surface, archive-name mismatch, or undocumented public option MUST fail documentation validation before publication. Cadder 1.0 SHALL NOT publish a machine-output or public wire-schema reference.

#### Scenario: Executable command example
- **WHEN** documentation validation runs against a release candidate
- **THEN** each marked command example completes with its documented outcome

#### Scenario: Stale command reference
- **WHEN** checked-in public command reference differs from the release candidate's supported surface
- **THEN** validation fails and identifies the stale document
