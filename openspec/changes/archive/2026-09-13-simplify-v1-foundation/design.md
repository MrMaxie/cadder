## Context

Cadder already builds one portable application containing `cadder`, `cadderd`, and the `caddy` shim for the four supported targets. The repository also contains custom native installer builders, installer manifests, signing readiness flags, release-asset rules, an LCOV parser, and a repository-specific OpenSpec validation engine. Those paths add several thousand lines of code and platform-tool dependencies before they add user value to the working runtime.

This change keeps the working runtime and portable archive format. It removes speculative distribution infrastructure, applies the repository's existing separate-test-module pattern to the largest Rust modules, and makes the lifecycle contract follow the simpler endpoint-lease and owned-child implementation already used by the production entrypoint. Validation must not start or replace a local Cadder/Caddy instance.

## Goals / Non-Goals

**Goals:**

- Make portable archives plus SHA-256 checksums the complete Cadder 1.0 distribution contract.
- Align the release and shim documentation with the existing `caddy` binary name.
- Keep release automation small enough to inspect and maintain in a personal project.
- Delegate schema validation and coverage threshold enforcement to the tools that own those formats.
- Use focused ecosystem crates where they replace platform-specific utility code.
- Keep executable entrypoints as orchestration layers and move pure policy/parsing logic into focused modules.
- Move large unit-test modules into separate files without changing behavior.
- Give oversized production modules small facade files and explicit responsibility-based submodules.
- Make the live local endpoint the only daemon singleton mechanism.
- Remove dormant runtime-guard, generation-proof, legacy lock, and file-discovery code.
- Keep Caddy control limited to the child handle created by the daemon.
- Keep shutdown attempts bounded even when an owned task does not join.

**Non-Goals:**

- Add a service manager, supervisor framework, watchdog, or second daemon.
- Migrate file storage to SQLite.
- Add a packaging framework, updater, dependency-injection framework, or new architectural layer.
- Rewrite existing unit tests or split the large IPC integration suite in the same change.
- Publish, install, or copy binaries into `.local/bin`.

## Decisions

### Portable archives are the only 1.0 distribution form

The release matrix continues to build the four supported targets and packages the three binaries, sample configuration, license, and checksum. Native MSI, DEB, RPM, PKG, shell installer, PowerShell installer, and Homebrew support are removed from the 1.0 contract and implementation.

Alternatives considered:

| Option | Local fit | Low cost | Low risk | Maintainability | Correctness | Maturity | Net value |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Keep custom native installers | 2 | 1 | 2 | 1 | 3 | 2 | 1 |
| Portable archives with current `xtask` | 5 | 5 | 5 | 5 | 4 | 5 | 5 |
| Migrate releases to `cargo-dist` | 3 | 2 | 3 | 4 | 4 | 5 | 3 |

`cargo-dist` remains a possible later migration, but the current workspace intentionally publishes three binaries from separate packages as one application. Adopting it now would be a larger release-model decision than the value needed for 1.0.

### Format owners enforce their own contracts

`cargo xtask openspec-check` retains the pinned OpenSpec version, `doctor`, implementation-schema validation, and strict canonical-spec validation, but removes the second custom parser and rule engine. Repository validation uses the official `openspec validate --specs --strict --json` command. Proposed changes are validated individually before implementation, so an early planning artifact without deltas does not block unrelated work.

Coverage continues to emit an LCOV report for inspection, while `cargo-llvm-cov --fail-under-lines 85` owns threshold calculation. `xtask` no longer parses `SF` and `DA` records.

### Focused libraries replace narrow platform utilities

The daemon uses `same-file` for cross-platform file identity while retaining its explicit Windows reparse-point validation. `xtask` uses `which` for executable discovery, `fs-err` for path-aware filesystem errors, `xshell` for command construction, and the already-present `toml` crate for workspace metadata. These are narrow substitutions, not new framework boundaries.

### Executable entrypoints remain thin

The PATH-facing shim keeps process startup, IPC, and exit-code orchestration in its executable module. Pure Caddy-command policy and run-registration parsing move into focused modules. This applies a functional-core/imperative-shell boundary without introducing traits, dependency injection, or a new architectural layer.

Repeated snapshot lookups in the reusable API and TUI data model use small local query helpers. Repeated runtime state construction uses named constructors and shared constants. These changes keep the existing types and behavior while making the common paths explicit.

### Test files move without test rewrites

Large `#[cfg(test)] mod tests` blocks move to `src/<module>/tests.rs` and retain `use super::*`. The production module keeps only `#[cfg(test)] mod tests;`. The change does not alter assertions, fixtures, visibility, timing, or runtime seams. The largest IPC module may move as one mechanical block, but its production code and integration suite remain unchanged.

### Production modules expose narrow internal seams

Large daemon modules split at existing responsibility boundaries: IPC connection/stream/dispatch mechanics, Caddy resolution/config composition, runtime process/mock implementations, and storage worker/file/recovery/platform helpers. Facade modules retain the existing public exports. Cross-module implementation details use the narrowest practical Rust visibility and do not introduce traits or dependency injection solely to enable the split.

`xtask` source files become ordinary Rust modules instead of textual `include!` fragments. Shared helpers remain crate-private and command behavior stays unchanged. Test files may remain colocated with their owning module, but textual inclusion is not used as a substitute for module boundaries.

### Endpoint ownership replaces persistent daemon-generation machinery

The production entrypoint already claims its owner-only local endpoint before initializing storage or Caddy and retains that listener until shutdown completes. That live kernel object is the daemon singleton. Cadder no longer writes a second daemon lock, lock metadata, containment lock, containment record, or endpoint-discovery document.

Clients derive the endpoint from the selected runtime directory and still authenticate through the operating-system transport and complete the versioned handshake before dispatch. On Unix, startup may remove a stale socket only after no owner answers readiness. On Windows, the named pipe itself supplies exclusive ownership.

### The daemon owns Caddy through one retained child handle

`ProcessRuntime` starts Caddy through the pinned-image spawn gate and stores the resulting `ProcessTreeChild`. The `process-wrap` library supplies a Unix process group and Windows Job Object, both with kill-on-drop behavior. Normal shutdown first requests graceful Caddy stop, then escalates only through that retained child wrapper. Cadder never enumerates or signals Caddy by name or an unverified PID.

The removed independent runtime guard would have provided a stronger promise after abrupt daemon termination, but it required a hidden process mode, a second framed protocol, executable attestation, two more locks, persistent generation proofs, and platform evidence. For a personal v1 project, that cost is larger than the value. A service manager or future supervisor can be proposed later if real deployments demonstrate the need.

### Shutdown is bounded rather than fail-stop

The daemon stops admission, attempts terminal stream delivery, cancels handlers, stops its owned runtime, flushes storage, and releases the endpoint within the configured shutdown timeline. If a handler, rollback, process wait, or storage flush misses its phase, Cadder reports shutdown failure and continues process teardown. Storage recovery already preserves complete records and discards only an incomplete final transaction tail.

Unit-test helpers may wait for a storage worker to finish so tests can release temporary files deterministically. Production shutdown does not retain endpoint ownership in an unbounded retry loop.

## Risks / Trade-offs

- [Risk] Users do not receive guided native installation in 1.0. -> Keep archive names, checksums, extraction, upgrade, alias, and removal instructions explicit and platform-specific where necessary.
- [Risk] Removing custom OpenSpec rules loses repository-specific policy checks. -> Keep only policies that represent an active product contract in OpenSpec and rely on review plus official strict validation; reintroduce a focused check only after a concrete escaped defect.
- [Risk] Official coverage calculation may differ slightly from the custom LCOV accumulator. -> Treat `cargo-llvm-cov` as the canonical calculation and keep the generated LCOV report for diagnosis.
- [Risk] Large file moves make the review diff noisy. -> Keep moved test contents byte-for-byte equivalent apart from module wrappers and formatting.
- [Risk] A focused library changes edge behavior. -> Retain current error context and platform-specific security checks, and cover the helper with unit tests.
- [Risk] Caddy can survive an abrupt daemon kill on platforms where operating-system teardown does not close the process boundary promptly. -> Keep kill-on-drop process groups/job objects, document the limitation, never signal an unverified process, and add an external supervisor only after a concrete deployment need.
- [Risk] A timed-out storage worker may not finish its final flush before process exit. -> Preserve complete transaction records and recover only an incomplete final line on the next start.

## Migration Plan

1. Update the distribution contract and user documentation to portable archives only.
2. Remove installer jobs, commands, builders, verifiers, download metadata, and their tests.
3. Replace custom OpenSpec and LCOV parsing with official command options.
4. Apply the existing separate-test-module convention to selected large modules.
5. Replace repetitive `xtask` plumbing and split pure shim policy/parsing from executable orchestration.
6. Split oversized production modules and replace textual source inclusion with explicit module boundaries.
7. Replace the dormant guard, lock, and discovery contract with the endpoint-lease and retained-child model.
8. Remove the superseded `secure-local-control-plane` and `simplify-runtime-location` change sets instead of maintaining plans whose shipped behavior is already represented by canonical specifications.
8. Use `process-wrap` for both Unix process groups and Windows Job Objects, and remove custom Windows job code.
9. Run formatting, Clippy/checks, official OpenSpec validation, and only unit tests that do not start a real Cadder/Caddy instance.

Rollback is a source-level revert before release. Existing published artifacts and the working `.local/bin` installation are not changed by this work.

## Open Questions

None for this slice. Storage migration, integration-test subdivision, and a future packaging framework remain separate decisions.
