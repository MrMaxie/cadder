## 1. Package workspace and launchers

- [x] 1.1 Add a Node 24 and Nub workspace for the public `cadder` package and the four exact-version platform packages, with no Bun configuration or install lifecycle scripts.
- [x] 1.2 Implement shared launcher code for `cadder`, `cadderd`, and `caddy` that selects the supported optional package, spawns without a shell, forwards process behavior, and reports unsupported or omitted-package failures clearly.
- [x] 1.3 Make the `caddy` launcher remove only its own PATH entry by file identity before native execution, and add focused cross-platform tests proving it cannot be resolved as upstream Caddy.
- [x] 1.4 Add manifest and packed-content tests for exact versions, operating-system metadata, CPU and libc constraints, repository and license identity, allowed files, and the absence of lifecycle scripts.

## 2. Release asset assembly and native verification

- [x] 2.1 Build package staging directories only from the four verified cargo-dist release archives, checking GitHub attestations, SHA-256 files, exact archive contents, and unchanged executable digests.
- [x] 2.2 Pack the root and platform packages deterministically and reject any unexpected tarball entry or dependency version drift.
- [x] 2.3 On all four native runners, install the locally packed root and matching platform package in a fresh consumer directory outside the checkout and verify `--version` and `--help` for all three commands.
- [x] 2.4 Add the package checks to the maintained local and CI validation paths without duplicating cargo-dist policy.

## 3. Stage-only trusted publishing

- [x] 3.1 Add a dedicated npm workflow triggered by a successfully published GitHub Release, using pinned actions, a GitHub-hosted runner, the protected `npm-production` environment, `contents: read`, and job-scoped `id-token: write`.
- [x] 3.2 Make the workflow download and verify the exact release assets, run the complete package gate, and call `npm stage publish` for all five packages without reading an `NPM_TOKEN`.
- [x] 3.3 Expose the five stage results for maintainer review and document the required platform-first, root-last 2FA approval order.
- [x] 3.4 Validate workflow syntax, permissions, trigger filtering, package order, failure containment, and a non-publishing dry-run plan.

## 4. Version and public documentation alignment

- [x] 4.1 Register all five npm manifests as `json-package@1` release version sources and verify that Arcantry keeps Cargo and npm versions coherent.
- [x] 4.2 Keep the public hero on verified download and documentation actions until the exact npm version passes registry smoke tests; defer the npm installation action and public installation guidance to `publish-cadder-1-0`.

## 5. Registry bootstrap and release gate

- [x] 5.1 Produce and review minimal non-executable `0.0.0-bootstrap.0` tarballs for all five unclaimed names, with a non-default `bootstrap` tag and no product binaries.
- [x] 5.2 After separate publication authorization, interactively create the five npm packages with account 2FA, confirm that each explicit `bootstrap` tag points to the non-executable bootstrap, and record the registry-required temporary `latest` assignment for replacement by the first verified release.
- [x] 5.3 Configure each package to trust only `MrMaxie/Cadder`, the exact npm workflow filename, and `npm-production`, allow staged publishing only, require 2FA, and disallow traditional publishing tokens.
