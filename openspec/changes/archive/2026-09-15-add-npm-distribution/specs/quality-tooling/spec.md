## ADDED Requirements

### Requirement: QT-006: npm packages pass clean-room verification

The npm package gate SHALL verify the exact packed file set, platform metadata, exact optional dependency versions, absence of install lifecycle scripts, and matching license and repository identity before any package is staged.

On every supported native runner, the gate MUST install locally packed packages in a fresh consumer directory outside the checkout and MUST exercise `--version` and `--help` for `cadder`, `cadderd`, and the Cadder `caddy` shim. The gate MUST prove that the npm `caddy` launcher cannot resolve itself as upstream Caddy.

#### Scenario: Package candidate is verified

- **WHEN** npm package tarballs are prepared for a release
- **THEN** every supported platform passes native clean-room installation and command checks
- **AND** unexpected files, lifecycle scripts, version drift, or launcher recursion fail the gate before registry staging

### Requirement: QT-007: npm publication uses staged trusted publishing

Cadder npm publication SHALL use npm trusted publishing from the exact GitHub-hosted workflow and protected release environment through short-lived OIDC credentials. The trusted publisher MUST allow staged publishing only, package publishing access MUST disallow traditional tokens, and no long-lived npm publish credential may be stored in the repository or GitHub Actions.

The workflow MUST stage packages only after it verifies the corresponding GitHub Release assets, SHA-256 checksums, and attestations. npm provenance MUST identify the public repository and publishing workflow. A maintainer MUST review and approve each stage with 2FA, approving all platform packages before the root package.

#### Scenario: Automated staging

- **WHEN** a verified tagged release reaches the npm workflow
- **THEN** GitHub Actions obtains a short-lived OIDC publishing identity
- **AND** it stages rather than directly publishes all version-matched packages
- **AND** no npm access token is available to the job

#### Scenario: Human publication approval

- **WHEN** the staged package set is ready for publication
- **THEN** a maintainer reviews the staged artifacts and approves them with 2FA
- **AND** the root package cannot become public before every referenced platform package

### Requirement: QT-008: New npm names use an explicit bootstrap

Before the first trusted release, each new npm package name SHALL be created interactively with account 2FA using a non-executable bootstrap version and an explicit non-default distribution tag. If the registry also assigns its required default tag to the package's only published version, that tag MAY temporarily point to the same non-executable bootstrap. Trusted publisher configuration and token restrictions MUST be applied only after the package names exist and before a real Cadder version is staged. The first verified release MUST replace the temporary default tag before npm installation is documented publicly.

#### Scenario: Package namespace is initialized

- **WHEN** maintainers bootstrap the npm package set
- **THEN** the default tag exposes no executable Cadder application and may point only to the non-executable bootstrap until the first verified release
- **AND** each package can be bound to the exact stage-only GitHub trusted publisher
