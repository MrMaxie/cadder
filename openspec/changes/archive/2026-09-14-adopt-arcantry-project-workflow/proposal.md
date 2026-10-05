## Why

Cadder already uses OpenSpec for accepted intent, but quick intake, release meaning and agent routing are not connected by one explicit project workflow. Adopting Arcantry provides that organizational layer while preserving the existing Cadder CLI, build, CI and release publication journeys unchanged.

## What Changes

- Add a shared Arcantry configuration that manages the existing OpenSpec tree, a root `todo.txt` queue and the project changelog.
- Record a local release baseline at the existing `v0.8.0` tag and backfill internal release meaning for archived changes.
- Add preliminary release meaning to active changes and route agents through the matching Arcantry capture, promotion, release and reconciliation skills.
- Add a private local launcher and guidance without creating a private configuration that would shadow the shared source of truth.

The smallest end-to-end capability is: capture a thought in `todo.txt`, promote accepted intent to OpenSpec, and derive local release planning from archived OpenSpec release artifacts.

## Capabilities

### New Capabilities

- `project-workflow`: Repository-operational rules for Arcantry-backed intake, accepted intent and local release story. This is not a Cadder product capability.

### Modified Capabilities

None. Existing product requirements and user journeys remain unchanged.

## Impact

The change affects repository guidance, local-only agent setup, OpenSpec release metadata, `arcantry.toml`, `todo.txt`, `CHANGELOG.md` and `releases/`. It does not affect Rust code, Cargo build behavior, Cadder commands, GitHub Actions or release publication.
