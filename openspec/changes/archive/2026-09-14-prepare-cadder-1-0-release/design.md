## Context

Cadder ships one portable application containing `cadder`, `cadderd`, and the PATH-facing `caddy` shim for four native targets. The upstream Caddy server remains a separate installation. The release candidate must therefore prove both archive structure and executable behavior without resolving binaries from the source checkout or connecting to a contributor's live Cadder runtime.

## Goals / Non-Goals

**Goals:**

- Build downloadable candidate artifacts for release pull requests.
- Verify the exact tagged archives before public announce on native runners.
- Bind published artifacts to the release workflow through GitHub attestations.
- Make integration tests deterministic and isolated from the user's runtime.
- Keep the 1.0.0 release story derived from accepted OpenSpec outcomes.

**Non-Goals:**

- Installers, Homebrew, Scoop, crates.io, Authenticode, or macOS notarization.
- Upgrading cargo-dist without an evidenced blocker.
- Adding Arcantry to Cadder's Cargo workspace, CI, or release workflow.
- Publishing, tagging, pushing, merging, or changing branch protection as part of local implementation.

## Decisions

### Use cargo-dist candidate upload and announce phases

Keep cargo-dist 0.32.0 pinned. Set pull-request runs to upload artifacts, move GitHub Release creation to announce, and place a reusable artifact verification job in the generated publish hook immediately before announce. Cargo-dist 0.32.0 and 0.33.0 include custom host jobs in `announce.needs` but omit their result from the generated `announce.if` expression, so a failed host job would not block publication. A global-artifact hook is enforced, but it runs alongside the global build and therefore cannot verify its checksums. The publish hook is the first generated phase that has the complete artifact set and whose failure is enforced before announce. The custom job performs verification only; it does not publish externally.

### Verify on native GitHub-hosted runners

Each target is downloaded, checksum-verified, extracted into a fresh directory, checked for the exact public file set, and exercised with `--version` and `--help`. Unix runners also verify executable permissions. The verifier derives the version from the cargo-dist plan rather than hard-coding 1.0.0.

### Attest only after verification

GitHub attestations run in the announce phase for archives, checksums, source archive, and manifest. The public release is created only after the native verifier succeeds.

### Isolate operator tests at the process boundary

Every test process receives a unique `CADDER_RUNTIME_DIR` and a controlled PATH containing copied test binaries. Commands have a bounded timeout, and cleanup targets only the isolated daemon started from that runtime.

## Risks / Trade-offs

- Native release verification increases release workflow duration. The four-runner matrix keeps failures target-specific and prevents one platform from standing in for another.
- Pull-request artifacts are candidates, not public releases. Final tagged artifacts are rebuilt and must pass the same structural and executable checks before announce.
- GitHub-hosted runner images can change. The verifier uses platform-native archive and checksum facilities already present on supported runners.

## Migration Plan

1. Validate the local schema and release-preparation change.
2. Fix and prove isolated operator integration tests.
3. Configure cargo-dist, generate its workflow, and validate the reusable verifier.
4. Complete and archive accepted OpenSpec changes, then render the 1.0.0 release manifest and changelog.
5. Run local gates and stop before external repository mutations that require separate authorization.

## Open Questions

None.
