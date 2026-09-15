# Documentation experience

## Purpose
Define accurate, audience-scoped documentation for users and contributors.

## Requirements

### Requirement: DOC-001: User documentation follows the retained journey
User documentation SHALL describe portable extraction, trusted real-Caddy configuration, managed `caddy run`, the TUI, and coherent manual upgrades without removed CLI, profile, alias installer, autostart, history, export, tail, watch, mock, or hidden-flag claims.

#### Scenario: New user follows Quick Start
- **WHEN** a user follows the published steps
- **THEN** every command exists in Cadder 1.0 and no repository-only seam is required

### Requirement: DOC-002: Contributor tooling is direct and reproducible
Contributor documentation SHALL use pinned mise tasks and direct maintained tools for Rust, OpenSpec, Astro, coverage, and cargo-dist.

#### Scenario: Contributor discovers tasks
- **WHEN** a contributor runs `mise tasks`
- **THEN** formatting, linting, tests, specs, docs, coverage, TUI web preview, and distribution tasks are visible

### Requirement: DOC-003: Private context does not leak
Published documentation MUST NOT contain personal absolute paths, `.local`, credentials, mock-only commands, or agent-only operational details.

#### Scenario: Documentation is built
- **WHEN** Astro validates the site
- **THEN** only portable reader-facing content is included

### Requirement: DOC-004: Installation guidance distinguishes npm, Cadder, and Caddy

Public installation guidance SHALL present `npx cadder`, global npm installation, and portable GitHub archives only when the corresponding channel has passed its release verification. It MUST explain that npm installs the Cadder operator, daemon, and `caddy` shim while the upstream Caddy web server remains a separate prerequisite.

The hero and getting-started path MUST give users adjacent actions for npm installation or download and documentation without presenting an unpublished package as available.

#### Scenario: npm channel is not yet public

- **WHEN** the current Cadder version is not verified in the public npm registry
- **THEN** published documentation does not instruct users to install that version from npm

#### Scenario: npm channel is verified

- **WHEN** the exact Cadder version passes clean-room registry installation on every supported platform
- **THEN** the hero and installation guide may present npm as a supported option
- **AND** they retain a direct GitHub download path and the separate upstream Caddy requirement
