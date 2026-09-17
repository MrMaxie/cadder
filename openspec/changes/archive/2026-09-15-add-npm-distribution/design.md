## Context

Cadder currently publishes one cargo-dist archive per supported platform. Each archive contains `cadder`, `cadderd`, and the Cadder `caddy` shim; upstream Caddy remains a separate installation. The npm package name `cadder` and the four proposed `@maxiedev/cadder-*` platform names were unclaimed when this change was proposed.

npm is removing direct publication through long-lived bypass-2FA tokens in January 2027. Trusted publishing uses short-lived GitHub Actions OIDC credentials, and stage-only permission adds an interactive 2FA approval before a package becomes public. New package names cannot use trusted or staged publishing until they exist in the registry, so a one-time interactive bootstrap is required. For a package's first version, the registry also retains a required `latest` tag even when publication names a non-default tag.

## Goals / Non-Goals

**Goals:**

- Preserve the existing archive installation journey while adding `npx cadder` and `npm install --global cadder`.
- Install the same version-matched three-command application on Windows x64, Linux x64 GNU, Intel macOS, and Apple Silicon macOS.
- Reuse the exact cargo-dist release binaries rather than rebuilding or downloading unverified binaries during package installation.
- Publish without a long-lived npm credential and require maintainer 2FA approval for every public version.
- Keep npm-specific behavior in Node launchers and release tooling rather than in Cadder's Rust runtime contracts.

**Non-Goals:**

- Bundling, installing, or updating upstream Caddy.
- Adding support for platforms absent from the cargo-dist matrix.
- Using Bun, compiling Rust on the consumer machine, or running a networked postinstall script.
- Replacing GitHub Releases or modifying cargo-dist's generated workflow by hand.
- Migrating TTYGlass publication as part of this Cadder change.

## Decisions

### Use a launcher package with platform packages

| Option | Local fit | Low cost | Low risk | Maintainability | Security/correctness | Maturity | Net value |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Download GitHub assets from an install script | 2 | 4 | 2 | 2 | 2 | 3 | 2 |
| Root launcher plus platform-specific optional dependencies | 5 | 4 | 5 | 5 | 5 | 5 | 5 |
| Compile the Rust workspace during npm installation | 1 | 1 | 2 | 2 | 3 | 3 | 1 |

The public `cadder` package will contain small Node launchers and exact optional dependencies on:

- `@maxiedev/cadder-win32-x64`
- `@maxiedev/cadder-linux-x64-gnu`
- `@maxiedev/cadder-darwin-x64`
- `@maxiedev/cadder-darwin-arm64`

Each platform package will declare its npm `os`, `cpu`, and, where applicable, `libc` constraints and contain the three cargo-dist executables. The root package will expose `cadder`, `cadderd`, and `caddy` command launchers. A launcher will resolve only the package for the current platform, spawn the native executable with `shell: false`, forward standard streams and signals, and preserve its exit status. Missing optional dependencies and unsupported platforms will fail with actionable messages.

The `caddy` launcher will remove only PATH entries whose `caddy` command resolves by file identity to that launcher before it starts the native shim. This prevents the native shim from rediscovering the npm launcher as upstream Caddy without adding npm-specific selection behavior to Rust. Tests will cover this boundary on every supported operating-system family.

Node 24 and Nub will own the launcher workspace and repeatable local package tasks. The npm CLI will be used only for registry-compatible pack, stage, trust, and approval operations. Bun will not be introduced.

### Consume verified GitHub Release assets

A dedicated hand-written npm workflow will run after a GitHub Release is published successfully. It will not edit or duplicate the cargo-dist workflow. It will download the four release archives and checksums, verify their GitHub attestations and SHA-256 values, extract the exact three binaries into platform package staging directories, and create npm tarballs with `npm pack`.

Native matrix jobs will install the locally packed root and matching platform tarballs in fresh consumer directories outside the checkout. They will exercise all three commands with `--version` and `--help`, verify version coherence, and confirm that the `caddy` launcher cannot resolve itself as upstream Caddy. Package-content checks will reject unexpected files, lifecycle scripts, and non-exact platform dependency versions.

### Use stage-only OIDC trusted publishing

| Option | Local fit | Low cost | Low risk | Maintainability | Security/correctness | Maturity | Net value |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Long-lived granular token with direct publish | 1 | 4 | 1 | 1 | 1 | 2 | 1 |
| OIDC trusted publisher with direct publish | 4 | 5 | 4 | 5 | 4 | 5 | 4 |
| OIDC trusted publisher with staged publish and 2FA approval | 5 | 4 | 5 | 5 | 5 | 5 | 5 |

The publish job will use a GitHub-hosted runner, `contents: read`, `id-token: write`, an exact Node and npm toolchain, and the protected `npm-production` environment. Each npm package will trust only `MrMaxie/Cadder`, the exact npm workflow filename, and that environment. The trust relationship will allow `npm stage publish` but not direct `npm publish`. No `NPM_TOKEN` will be created or stored. Trusted publishing will generate npm provenance automatically.

The workflow will stage all five packages for the release version. A maintainer will review the staged tarballs and approve the four platform packages with 2FA before approving the root package. The root package is approved last so its exact optional dependencies are already public. Final verification will install the exact registry version with a fresh cache and consumer directory before documentation presents npm as available.

### Bootstrap package names without publishing a product release

Because npm requires a package to exist before trusted or staged publishing can be configured, maintainers will interactively publish minimal `0.0.0-bootstrap.0` packages under a non-default `bootstrap` tag using account 2FA. Bootstrap packages will contain only truthful metadata and no executable product. npm may also retain `latest` on that first version because the package has no other published version. This temporary default tag is acceptable only while it points to the same non-executable bootstrap. The first verified release must replace it with the exact approved Cadder version before npm installation is documented publicly. After all names exist, maintainers will configure the five stage-only trusted publishers, set package publishing access to require 2FA and disallow traditional tokens, and verify the configuration before the real release workflow runs.

The bootstrap action is a separate, explicitly authorized registry mutation. It will happen only after the manifests and packed contents pass local review.

### Keep versions and release meaning coherent

The five npm manifests will be explicit Arcantry `json-package@1` version sources alongside the Cargo workspace. Root optional dependency versions will be exact and validated against the release version. Since public latest is still `v0.8.0`, this accepted change can be added to the untagged `1.0.0` manifest before the seal; it must not create a second product version for the same source revision.

## Risks / Trade-offs

- npm cannot publish the five-package set atomically. Approving platform packages first and the root package last prevents a public root package from referencing missing native packages.
- Users who omit optional dependencies cannot run Cadder. The launcher will diagnose the package-manager setting and name the required platform package.
- `npx cadder` starts only the requested operator command; persistent daily use is better served by the global installation because the `caddy` shim must remain on PATH.
- The npm package adds Node as an installation-channel requirement, but the installed Cadder runtime remains native and upstream Caddy remains independent.
- Bootstrap versions remain visible in registry history, and npm may retain `latest` on the first version. Non-executable contents prevent that temporary tag from exposing a false Cadder application, and the first verified release replaces it before npm installation is advertised.

## Migration Plan

1. Implement and validate the launcher workspace, platform manifests, package-content rules, and isolated local consumer tests without publishing.
2. Add the dedicated stage-only npm workflow and validate its event, permissions, pinned actions, release-asset inputs, and dry-run package plan.
3. Add all npm manifests to the release version sources, then archive this implementation change so the release rollout can add its accepted outcome to the untagged 1.0.0 release story.
4. With separate authorization, publish the five bootstrap packages interactively under the `bootstrap` tag using 2FA, accept a registry-required temporary `latest` only while it points to the same non-executable bootstrap, and replace it during the first verified release.
5. Configure stage-only trusted publishers for the exact GitHub workflow and `npm-production` environment, then disallow traditional publishing tokens.
6. In the follow-up `publish-cadder-1-0` change, run the release workflow, review staged packages, approve platform packages first and the root package last, and verify exact clean-room installations.
7. In that follow-up, update the public hero and installation documentation to advertise npm alongside GitHub downloads only after registry verification.
8. If any package for a version is incorrect after approval, do not reuse or overwrite that version; correct the source and publish the next patch version.

## Open Questions

None.
