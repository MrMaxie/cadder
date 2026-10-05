# Cadder OpenSpec

> Last reviewed: 2026-10-05

OpenSpec records the accepted Cadder 1.0 contract and the evidence that the
implementation satisfies it. Product requirements live here; public docs
explain how people use the finished product.

The active `reset-cadder-architecture` now defines the approved 2.0 Node migration,
not another Rust reset. Its functional scope preserves the released Rust 1.0.5
CLI/TUI, not superseded history/IIS/autostart or machine-output workflows. Its
`acceptance.md` maps the approved scope to stable
requirement IDs, ordered gates and evidence; `tasks.md` counts only Node work.
Main specs retain the 1.0 baseline until the reviewed deltas are synced. Planning
completion is not product readiness or release authorization.

## Contract model

`openspec/specs/` describes the accepted target behavior for Cadder 1.0. A main
spec states what the released product does, even while its implementation is in
progress. The active changes under `openspec/changes/` show which parts still
need implementation or verification.

This target-contract model is deliberate. It differs from the default OpenSpec
convention, where main specs usually describe current implemented behavior.
Cadder keeps that distinction explicit so target documentation can use present
tense without claiming release readiness.

## Change types

| Change | Schema | Use | Archive behavior |
| --- | --- | --- | --- |
| Contract | `spec-driven` | Add, modify, rename, or remove accepted product requirements | Update main specs |
| Implementation | `implementation` | Implement and verify existing requirement IDs | Use `--skip-specs` |

Every product requirement has a stable identifier in its heading, such as
`IPC-001`. Implementation proposals, tasks, tests, and verification evidence
refer to these identifiers. Renaming prose does not change an identifier.

## Workflow

1. Read `openspec list --json`, the relevant main specs, and active changes.
2. Use `spec-driven` for a contract change. Review and archive it into the main
   specs before implementation starts.
3. Create one focused implementation slice with
   `openspec new change <name> --schema implementation`. Reference the main-spec
   requirement IDs instead of copying requirements into the change.
4. Complete a task only after its code, tests, and stated checks pass.
5. Record final evidence in `verification.md`, validate the change, and archive
   implementation work with `--skip-specs`.

Keep only one implementation change active at a time unless two changes touch
independent subsystems and their dependency order is explicit.

## Validation

Run these commands from the repository root:

```sh
openspec doctor
openspec schema validate implementation
openspec validate --specs --strict
mise run openspec-check
```

Also review stable requirement IDs, task-to-acceptance references, verification
coverage and documentation boundaries explicitly; CLI schema validation does not
prove that a plan covers the requested product or that implementation exists.

After every implementation change is archived, the final release gate also
runs `openspec validate --all --strict` against the remaining contract changes
and main specs.

Cadder requires OpenSpec 1.5.0 while the project-local schema contract remains
at version 1. OpenSpec 1.5 does not apply strict delta validation correctly to a
custom implementation change without `specs/`, so `openspec-check` validates
those changes. The repository keeps `spec-driven` as the CLI default and puts
contract-only writing rules in the shared project context because a `specs`
artifact rule is invalid for the custom implementation schema. Implementation
changes therefore select their schema explicitly.

## Content boundaries

- Write specs and change artifacts in English.
- Keep product requirements observable and testable.
- Put implementation choices in design artifacts unless users depend on them.
- Do not include workstation paths, `.local` content, credentials, or private
  operating notes in tracked documentation.
- Keep implementation status in changes and verification artifacts. Public
  product documentation describes the accepted 1.0 behavior in present tense.
