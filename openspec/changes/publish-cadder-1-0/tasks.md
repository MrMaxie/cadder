## 1. Seal the release candidate

- [x] 1.1 `[DIST-004]` Add the archived npm distribution outcome to the untagged 1.0.0 manifest, render the changelog, and verify release consistency without creating a tag. (verification: Arcantry release check passes and no tag exists)
- [x] 1.2 `[DIST-001] [DIST-002] [QT-003]` Run the complete repository, coverage, cargo-dist planning, and local artifact gates on one source revision, then review the full release diff. (verification: every local gate passes on the same Git revision)
- [x] 1.3 `[QT-004]` After explicit authorization, commit the reviewed release candidate without unrelated local files. (verification: the staged diff contains only the approved release candidate)
- [x] 1.4 `[QT-004]` After separate authorization, push the candidate and open or update its pull request without publishing a release. (verification: the remote pull request points to the approved candidate commit and no release exists)
- [x] 1.5 `[DIST-002] [QT-004]` Wait for every pull-request gate and candidate artifact, then verify the exact Windows archive outside the checkout. (verification: all pull-request checks pass and the downloaded archive passes isolated smoke verification)

## 2. Seal master and publish GitHub assets

- [ ] 2.1 `[DOC-004] [QT-004]` After explicit authorization, squash merge the fully green candidate and verify CI, documentation, installation links, and SEO on the sealed master revision. (verification: master CI and live pre-npm documentation checks pass on the merge revision)
- [ ] 2.2 `[DIST-001] [DIST-002] [QT-003]` Run the sealed Arcantry, repository, coverage, cargo-dist planning, and local distribution gates on the exact current master revision. (verification: every sealed local gate passes and Arcantry identifies the exact release seal)
- [ ] 2.3 `[QT-004]` Present the exact master commit, obtain separate tag authorization, and push one annotated immutable `v1.0.0` tag pointing to that commit. (verification: the remote annotated tag resolves to the authorized seal commit)
- [ ] 2.4 `[DIST-002] [QT-004]` Verify that the release workflow publishes the complete final GitHub Release, checksums, source archive, manifest, and attestations only after all native artifact jobs pass. (verification: the final non-draft release and every required asset and attestation are present)

## 3. Publish and verify npm

- [ ] 3.1 `[QT-007]` Let the GitHub Release trigger the stage-only npm workflow and verify that all five exact 1.0.0 packages are staged through OIDC without an npm token. (verification: npm shows five staged 1.0.0 packages with the configured trusted publisher identity)
- [ ] 3.2 `[DIST-004] [QT-007]` Review every staged tarball and approve the four platform packages before the root package with interactive 2FA. (verification: all platform versions are public before the root version is approved)
- [ ] 3.3 `[DIST-005] [QT-006] [QT-008]` From fresh caches on all supported platforms, verify exact package resolution, native command behavior, provenance, and replacement of every temporary bootstrap `latest` tag by 1.0.0. (verification: native clean-room jobs pass and every default tag resolves to 1.0.0)
- [ ] 3.4 `[DIST-004] [QT-006]` Verify `npx cadder@1.0.0` and a temporary-prefix global `cadder@1.0.0` installation outside the checkout. (verification: both consumer paths run the exact 1.0.0 native operator outside the repository)

## 4. Publish npm installation guidance

- [ ] 4.1 `[DOC-004]` Update the installation guide and hero to present npm beside portable downloads while distinguishing the Cadder operator, daemon, and shim from separately installed upstream Caddy. (verification: the docs source names only verified public commands and channels)
- [ ] 4.2 `[DOC-004]` Run documentation checks and live desktop and mobile verification for the npm commands, CTA behavior, links, code blocks, footer, and Cadder versus Caddy responsibility boundary. (verification: documentation automation passes and both target viewports satisfy the manual checklist)
- [ ] 4.3 `[DOC-004] [QT-008]` After explicit publication authorization, publish the documentation update and verify the live site points to the exact GitHub and npm releases. (verification: the deployed site links to the verified 1.0.0 GitHub and npm releases)
