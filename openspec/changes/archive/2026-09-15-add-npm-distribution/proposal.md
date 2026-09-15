## Why

Cadder currently requires users to download and place a portable archive manually even when Node-based tooling is already available. A verified npm distribution would preserve the existing three-executable installation while adding `npx cadder` and `npm install --global cadder` without introducing long-lived registry credentials.

## What Changes

- Publish a public `cadder` launcher package plus four platform-specific packages containing the exact verified `cadder`, `cadderd`, and Cadder `caddy` shim binaries.
- Support `npx cadder` for one-off operator use and `npm install --global cadder` for a persistent installation that exposes the complete three-command Cadder surface.
- Publish from GitHub Actions through npm trusted publishing with OIDC, automatic provenance, stage-only permission, and explicit maintainer approval with 2FA.
- Bootstrap the package names once with interactive 2FA so the first real Cadder release can use trusted publishing; do not create or store an `NPM_TOKEN`.
- Verify packed and staged package contents, native command behavior, version coherence, and isolated consumer installation before the public npm tag is promoted.
- Keep GitHub portable archives as the canonical release artifacts and keep upstream Caddy as a separate installation.
- Do not use Bun, compile Rust during npm installation, download binaries from arbitrary install scripts, or add other installer channels in this change.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `distribution-and-upgrades`: Add npm as a verified installation and upgrade channel for the same coherent application release.
- `documentation-experience`: Document npm and portable-archive installation without implying that Cadder includes upstream Caddy.
- `quality-tooling`: Add package construction, isolated consumer verification, trusted staging, and publication gates.

## Impact

This change adds Node-based launcher and platform package manifests, package verification tooling, and a dedicated GitHub Actions workflow that consumes verified cargo-dist release assets. It changes the public installation contract and npm registry configuration, but does not change Cadder's Rust runtime protocol, Caddy ownership boundary, or cargo-dist archive contents.
