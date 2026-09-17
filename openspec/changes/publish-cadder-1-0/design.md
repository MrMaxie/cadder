## Context

The npm distribution code, package construction, trusted publishers, token restrictions, and non-executable namespace bootstrap are complete in the archived `add-npm-distribution` change. Arcantry can now add that accepted outcome to the untagged 1.0.0 release manifest. The remaining work is an ordered external rollout: seal the source revision, publish the verified GitHub release, stage and approve the five npm packages, verify the exact registry artifacts, and only then advertise npm installation.

## Requirements

- `DIST-001`, `DIST-002`, and `QT-003` define the version-matched portable artifact set and its owning release tool.
- `QT-004` defines the pull-request candidate gate and tagged announce boundary.
- `DIST-004`, `DIST-005`, `QT-006`, and `QT-007` define coherent npm packaging, archive identity, clean-room verification, and stage-only trusted publishing.
- `QT-008` requires the first verified release to replace the temporary bootstrap default before npm is advertised.
- `DOC-004` keeps public installation guidance behind exact channel verification and distinguishes Cadder from upstream Caddy.

## Goals and non-goals

### Goals

- Produce one sealed Cadder 1.0.0 source revision with coherent Cargo and npm versions and a complete changelog.
- Publish the existing portable archives only after their native artifact gate succeeds.
- Replace every temporary npm bootstrap `latest` tag with the exact verified 1.0.0 package through stage-only trusted publishing and maintainer 2FA approval.
- Verify the public npm packages outside the checkout before publishing npm installation guidance.

### Non-goals

- Changing Cadder runtime behavior or package construction.
- Adding installers, Homebrew, Scoop, crates.io, signing, notarization, or another release channel.
- Reusing or moving an immutable tag after code or artifact changes.
- Advertising the non-executable bootstrap as a Cadder release.

## Ownership and trust boundaries

GitHub Actions owns immutable archive construction, native artifact verification, attestations, and GitHub Release publication. The npm trusted publisher is bound to the exact GitHub workflow and protected environment and can create staged packages only. The maintainer owns interactive review and 2FA approval. Arcantry owns local release meaning and changelog rendering but cannot tag, push, or publish. Public documentation remains blocked until the exact registry artifacts pass clean-room verification.

## Data flow and public contracts

The sealed source revision produces the GitHub Release artifacts. The npm workflow consumes only those released archives, verifies checksums and attestations, assembles version-matched platform packages, and stages them through OIDC. After platform-first and root-last approval, fresh consumers verify registry resolution and native commands. Only that evidence unlocks npm installation guidance. No step changes the Cadder runtime protocol or the requirement that upstream Caddy be installed separately.

## Decisions

The archived npm implementation outcome is added to the existing 1.0.0 release manifest before the source revision is sealed. The final source diff, full repository gate, coverage gate, cargo-dist plan, and local distribution build must pass before commit, push, merge, or tag operations.

The release uses the existing pull-request candidate gate and cargo-dist announce phase. The exact merge commit becomes the immutable release seal. An annotated `v1.0.0` tag may be pushed only after separate authorization naming that commit. GitHub Actions owns archive construction, native verification, attestations, and GitHub Release publication.

The npm workflow consumes only the published and attested GitHub Release assets. Its trusted publisher may create staged packages but cannot publish directly. A maintainer reviews the stages and approves the four platform packages before the root `cadder` package. Approval requires interactive 2FA. The resulting 1.0.0 promotion replaces the temporary bootstrap `latest` tags; `bootstrap` may remain as an explicit historical tag.

Registry verification uses fresh caches and consumer directories outside the checkout on every supported platform. It checks exact version resolution, native execution, provenance, `npx cadder`, and a temporary-prefix global installation. Public documentation remains on download and documentation actions until these checks pass. The npm installation guide and hero action are published in a separate post-release documentation commit so an unavailable package is never advertised.

## Failure and recovery

A failed candidate or native verification stops before publication. A failed npm stage remains unpublished and can be inspected without changing registry consumers. Registry propagation retries read only the immutable exact version. If code or package bytes must change after `v1.0.0` is pushed, the tag is preserved and the correction becomes `1.0.1`.

## Migration and rollback

The non-executable bootstrap versions remain historical. The first verified 1.0.0 publication replaces each temporary default tag; it does not delete or overwrite bootstrap package versions. Before the immutable tag exists, local release preparation can be reverted normally. After the tag exists, product corrections require a new version.

## Test strategy

Run the complete repository and coverage gates, cargo-dist planning, and local distribution build on one revision. Verify pull-request artifacts on native CI runners and smoke-test the exact Windows candidate outside the checkout. Verify GitHub Release checksums and attestations before npm staging. Then use fresh caches and consumer directories on every supported platform to verify package identity, provenance, all three native commands, `npx`, and a temporary-prefix global installation. Finally validate and inspect the public documentation on desktop and mobile.

## Risks and trade-offs

- GitHub Releases and npm cannot publish the complete multi-platform set atomically. Native pre-publication checks and platform-first, root-last npm approval prevent the public root package from referencing missing native packages.
- Registry metadata can take time to propagate. Verification retries only reads of the immutable exact version; it never republishes or overwrites 1.0.0.
- The temporary bootstrap remains in package history. It contains no executable, and the verified release replaces its default tag before documentation changes.
- If code or package bytes must change after the tag is pushed, the tag is preserved and the correction becomes 1.0.1.

## Open questions

None.
