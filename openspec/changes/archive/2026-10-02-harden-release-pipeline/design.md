## Context

cargo-dist owns Cadder's release planning, artifacts, and generated GitHub workflow. Its current generated workflow grants `contents: write` globally, exports `GH_TOKEN` to planning and build jobs, and inserts a GitHub ref expression directly into shell source. The npm workflow verifies repository attestations but does not bind them to the selected tag, commit, or signer workflow.

## Goals / Non-Goals

**Goals:**

- Preserve cargo-dist ownership of release planning and artifact construction.
- Give build jobs read-only repository access and no publication token.
- Treat a release tag as validated data rather than shell program text.
- Prove npm inputs belong to the exact selected release and expected signer workflow.
- Keep local and CI checks deterministic after the focused hardening pass.

**Non-Goals:**

- Publish, tag, stage, or approve a release.
- Replace cargo-dist with a hand-maintained release pipeline.
- Add a general workflow generation framework.
- Upgrade unrelated Rust dependencies or adopt breaking dependency releases.

## Decisions

The options were waiting for cargo-dist support, maintaining the complete workflow by hand, and applying a focused deterministic postprocessor. Waiting leaves accepted findings open, while a hand-maintained workflow duplicates cargo-dist policy. A standard-library Node.js postprocessor is selected because Node is already pinned by the repository, it changes only known permission and tag-handling fragments, fails closed when the generated shape changes, and preserves cargo-dist ownership of every artifact operation.

The postprocessor has apply and check modes. Apply mode hardens a freshly generated workflow. Check mode verifies both the exact structural transformations and the resulting security invariants. The supported `mise` tasks call the script through the pinned Node.js runtime and keep the committed workflow equal to the generated-then-hardened result. Commands that consume the intentionally customized workflow pass cargo-dist's `--allow-dirty` flag only after the dedicated repository check proves that the committed workflow equals generated output after hardening.

The workflow places the tag expression in `RELEASE_TAG`, validates the accepted Cadder `v<semver>` form once, and passes the quoted value to cargo-dist. Planning and build jobs receive `contents: read`; only the announce path receives the write and attestation permissions needed to publish immutable assets.

The npm prepare job checks out the selected tag, resolves its commit, and passes the tag ref, commit digest, and expected release workflow to the assembly script. Each `gh attestation verify` invocation requires all three identities in addition to the repository.

The documentation lockfile is refreshed with npm so the existing compatible dependency range resolves `devalue` at or above 5.9.4. No direct documentation dependency or TypeScript major changes are included.

## Risks / Trade-offs

- The postprocessor is intentionally coupled to cargo-dist's generated workflow shape. Unexpected upstream changes fail the check and require a reviewed adapter update rather than silently weakening permissions.
- A custom adapter is one additional repository concept. It remains narrow, contains no artifact policy, and is removed when cargo-dist can express the same invariants.
- Exact signer and source binding rejects otherwise valid attestations produced by a different workflow. That is the intended release identity boundary.

## Migration Plan

1. Update cargo-dist to 0.33.0 and regenerate the official workflow.
2. Add the postprocessor and tests, then commit only the hardened generated result.
3. Bind npm checkout and attestation verification to the selected release identity.
4. Refresh the documentation lockfile and confirm the vulnerable transitive version is absent.
5. Run release workflow checks, npm tests and audit, cargo-dist planning, OpenSpec validation, and the complete repository gate.

Rollback restores the previous generated workflow, cargo-dist version, npm provenance checks, and lockfile. No published artifact is modified.

## Open Questions

None.
