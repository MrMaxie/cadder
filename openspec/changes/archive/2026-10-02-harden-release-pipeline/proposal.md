## Why

Cadder's release automation grants repository write access to build jobs and interpolates release refs into shell programs, while npm assembly accepts attestations that are not tied to the selected release identity. Public artifacts need least-privilege construction and exact provenance without replacing cargo-dist as the release owner.

## What Changes

- Keep cargo-dist as the release generator, update it to 0.33.0, and apply one deterministic Cadder hardening pass to the generated GitHub workflow.
- Default release permissions to read-only, with write and attestation permissions only on publishing jobs.
- Pass release tags through validated environment values instead of embedding GitHub context expressions in shell source.
- Check out the selected npm release tag and bind artifact attestation verification to its source ref, commit digest, repository, and release workflow.
- Refresh the documentation lockfile so the vulnerable transitive `devalue` 5.9.2 is replaced by at least 5.9.4.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `quality-tooling`: Generated release automation receives a focused, verified least-privilege hardening pass.
- `distribution-and-upgrades`: npm packages consume artifacts proven to belong to the exact selected Cadder release.

## Impact

This change affects cargo-dist configuration and generated workflow verification, the npm release workflow and assembly script, release-focused tests, and the documentation dependency lockfile. It does not publish a release, create a tag, change package versions, or alter runtime behavior.
