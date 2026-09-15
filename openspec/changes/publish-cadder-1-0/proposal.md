## Why

The npm distribution implementation must be archived before Arcantry can include its accepted outcome in the 1.0.0 release manifest, while public npm verification can occur only after that sealed release is tagged and published. A separate rollout change removes this lifecycle cycle and keeps registry publication distinct from implementation.

## Requirement IDs

- `DIST-001`: Release binaries are version matched.
- `DIST-002`: Archives are portable and verifiable.
- `DIST-004`: npm installs the coherent native application.
- `DIST-005`: npm and portable archives share release identity.
- `DOC-004`: Installation guidance distinguishes npm, Cadder, and Caddy.
- `QT-003`: Maintained tools own validation and release formats.
- `QT-004`: Local and CI task parity.
- `QT-006`: npm packages pass clean-room verification.
- `QT-007`: npm publication uses staged trusted publishing.
- `QT-008`: New npm names use an explicit bootstrap.

## Scope

- Add the archived npm distribution outcome to the untagged 1.0.0 manifest and rendered changelog.
- Seal, merge, tag, and publish the exact Cadder 1.0.0 release through the existing GitHub release gate.
- Stage the five npm packages through the configured stage-only trusted publishers, review them, and approve platform packages before the root package with 2FA.
- Verify the exact public npm version, provenance, `npx cadder`, and a temporary-prefix global installation outside the checkout.
- Publish npm installation guidance and the hero npm action only after every registry verification passes.

## Non-goals

- Changing Cadder runtime behavior or package construction.
- Adding another distribution channel.
- Bypassing the existing release, verification, or approval gates.
- Moving or replacing an immutable release tag.

## Success criteria

- One reviewed source revision passes the complete repository, coverage, cargo-dist, and local artifact gates before publication.
- GitHub publishes the exact version-matched 1.0.0 portable artifacts only after native verification.
- npm stages the exact five 1.0.0 packages through OIDC without a long-lived token, and maintainer 2FA approval publishes them platform-first and root-last.
- Fresh-cache consumers on every supported platform verify exact package resolution, native commands, provenance, and replacement of the bootstrap default tag.
- Public documentation advertises npm only after the exact registry packages pass verification.

## Impact

This change affects the local 1.0.0 release manifest and changelog, GitHub release publication, npm staged packages and distribution tags, and the public installation documentation. It does not change Rust runtime contracts or package construction.
