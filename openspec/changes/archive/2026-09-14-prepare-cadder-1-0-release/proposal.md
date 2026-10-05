## Why

Cadder 1.0 needs a reproducible release gate that proves the portable archives work outside the source checkout before they are published. The existing cargo-dist workflow plans releases correctly, but it does not build complete candidates on pull requests or verify the exact tagged archives before announce.

## What Changes

- Build and upload the complete portable artifact set on release pull requests.
- Verify each tagged archive on its native operating system before GitHub Release publication.
- Publish GitHub artifact attestations for release archives, checksums, the source archive, and the cargo-dist manifest.
- Keep release publication separate from validation and preserve cargo-dist as the owner of archive generation.
- Isolate the operator integration test from any Cadder runtime already running on a contributor machine.
- Add a project-local release-bearing OpenSpec schema without adding Arcantry to Cadder's build or CI dependencies.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `quality-tooling`: Add candidate artifact production and a pre-publication native verification gate.
- `distribution-and-upgrades`: Require native archive checks and GitHub artifact attestations for published release assets.

## Impact

- Cargo-dist configuration and its generated GitHub Actions workflow.
- A reusable GitHub Actions artifact-verification workflow.
- Operator integration-test process isolation and timeout behavior.
- OpenSpec schema and release metadata used to prepare version 1.0.0.
