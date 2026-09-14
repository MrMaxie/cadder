## Context

Cadder has a mature OpenSpec tree, an existing `v0.8.0` tag and project-specific build and publication tooling. It does not have an Arcantry configuration, a quick intake queue or explicit release artifacts for archived changes. The repository also has private `.local` state and a dirty working tree that must be preserved.

## Goals / Non-Goals

**Goals:**

- Make OpenSpec the configured accepted-intent source, `todo.txt` the default quick queue and `CHANGELOG.md` the managed public history.
- Establish an auditable `0.8.0` baseline and record release meaning without inventing public historical prose.
- Give agents shared routing plus a private local launcher and guidance.

**Non-Goals:**

- Change Cadder product behavior, Cargo tooling, CI, tags or publication.
- Create `.local/arcantry.toml`, which would shadow the shared configuration.
- Infer release meaning from Git diffs or expose private paths in shared artifacts.

## Decisions

The tracked `arcantry.toml` manages `openspec`, root `todo.txt` and `CHANGELOG.md`. Its release adapter reads Cargo's workspace package version and stores manifests under `releases/`. Optional private intake remains `.local/todo.txt` and is discovered independently without a private configuration.

The `0.8.0` manifest is a baseline with no public change assignments. All five archived changes receive internal release metadata and remain unassigned for the next local release plan. Active changes receive preliminary metadata that must be reviewed again at closeout.

Root guidance contains the shared managed Arcantry section and explicit skill routing. `.local/AGENTS.md` and `.local/bin/arcantry.cmd` remain private. The launcher calls the locally built Arcantry CLI against the Cadder root and does not modify Cadder's toolchain.

## Risks / Trade-offs

- [Preliminary SemVer metadata can drift from delivery] - Review each active `release.md` before archiving the change.
- [A private config would hide shared responsibilities] - Do not create `.local/arcantry.toml`; use independently discovered private sources.
- [Arcantry CLI remains a local dependency] - Keep the path only in ignored local files and leave CI unchanged.
