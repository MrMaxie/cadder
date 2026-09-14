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
