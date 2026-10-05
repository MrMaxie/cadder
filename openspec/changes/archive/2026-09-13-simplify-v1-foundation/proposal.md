## Why

Cadder's current 1.0 foundation carries native installer, signing, provenance, and custom repository-validation machinery that is disproportionate for a small pre-1.0 project. The product already has a workable cross-platform runtime and portable packaging path, so 1.0 should concentrate on a clear, maintainable base that can grow without preserving speculative infrastructure.

## What Changes

- **BREAKING**: Narrow the supported 1.0 distribution contract to versioned portable archives with SHA-256 checksums published through GitHub Releases.
- Remove native MSI, DEB, RPM, and PKG builders, installer metadata, signing readiness gates, and installer-specific release verification from `xtask`, CI, documentation, and download metadata.
- Use the official OpenSpec CLI validation surface instead of maintaining a second repository-specific OpenSpec parser and rule engine.
- Use `cargo-llvm-cov`'s built-in line threshold instead of parsing LCOV records in `xtask`.
- Move large unit-test modules into separate source files without changing runtime behavior or public APIs.
- Replace flat source inclusion and oversized production modules with explicit Rust module boundaries while preserving existing behavior and exports.
- **BREAKING**: Make the live owner-only local endpoint the daemon singleton and remove the unused lock, file-discovery, runtime-guard protocol, containment record, and replacement-proof architecture.
- Remove the stale `secure-local-control-plane` and completed `simplify-runtime-location` change sets after retaining their shipped behavior in the canonical specifications.
- Keep Caddy ownership local to the retained child handle and the process library's process-group or job-object wrapper; Cadder does not discover or signal unrelated processes.
- Make shutdown bounded and diagnosable instead of retaining the daemon indefinitely when a task, rollback, or storage flush does not join.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `distribution-and-upgrades`: Replace the native-installer, signing, SBOM, and provenance requirements with a portable archive and checksum contract suitable for Cadder 1.0.
- `project-registration`: Align the documented shim filename and explicit PATH setup contract with the existing `caddy` binary.
- `daemon-lifecycle`: Replace persistent generation locks and independent owner-loss containment with endpoint ownership and a retained child handle.
- `local-control-plane`: Derive the local endpoint directly and make shutdown bounded without file-discovery cleanup.
- `runtime-storage`: Remove obsolete discovery and process-lock artifacts from the ephemeral-state contract.
- `operator-cli`: Diagnose endpoint readiness directly instead of inspecting a discovery document.

## Impact

The change affects the release workflow, `xtask`, release documentation and download metadata, Rust module organization, daemon lifecycle, and local endpoint ownership. It removes platform packaging tool dependencies and several thousand lines of dormant supervision and publication code while retaining all four supported release targets, the three Cadder binaries, owner-only IPC authentication, the versioned handshake, pinned Caddy verification, process-tree ownership, durable storage recovery, and bounded runtime shutdown.
