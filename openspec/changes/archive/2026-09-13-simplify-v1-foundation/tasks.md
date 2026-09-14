## 1. Contract and release surface

- [x] 1.1 Update the canonical distribution contract to the portable-archive-only 1.0 scope and align the project license metadata
- [x] 1.2 Remove native installer jobs and signing-readiness gates from the release workflow
- [x] 1.3 Update contributor, architecture, release, and end-user documentation to describe archives and checksums only
- [x] 1.4 Remove native-installer download matching and presentation from the documentation site

## 2. Repository tooling

- [x] 2.1 Remove native installer commands, builders, metadata, release verification, and installer-specific tests from `xtask`
- [x] 2.2 Replace the custom OpenSpec repository validator with pinned official strict validation
- [x] 2.3 Replace the custom LCOV threshold parser with the `cargo-llvm-cov` fail-under option
- [x] 2.4 Use structured TOML parsing and focused executable/file-identity libraries instead of manual utility implementations

## 3. Test organization

- [x] 3.1 Move selected client, daemon-entrypoint, and shim unit tests into separate files
- [x] 3.2 Move the Caddy, runtime, and storage unit-test modules into separate module files
- [x] 3.3 Move the IPC unit-test module into a separate file without changing its integration suite

## 4. Verification

- [x] 4.1 Run formatting, static Rust checks, official OpenSpec validation, and focused unit tests without starting a real Cadder/Caddy runtime
- [x] 4.2 Review the final diff for behavior changes, private workspace leakage, generated artifacts, and accidental `.local/bin` changes

## 5. Production code simplification

- [x] 5.1 Replace repetitive `xtask` filesystem and command-runner plumbing with focused ecosystem crates
- [x] 5.2 Split shim command policy and registration parsing from the executable orchestration layer
- [x] 5.3 Consolidate repeated snapshot lookup and runtime-state construction logic
- [x] 5.4 Keep the extracted policy and registration logic covered by isolated unit tests
- [x] 5.5 Run formatting, static Rust checks, focused unit tests, and a fresh-eyes review without starting Cadder or Caddy

## 6. Final reduction pass

- [x] 6.1 Move the remaining large inline unit-test modules into separate files without rewriting their scenarios
- [x] 6.2 Move the flat `xtask` command layer behind a real Rust module boundary without changing commands
- [x] 6.3 Split IPC client and daemon-launch responsibilities from the server dispatcher without changing public exports
- [x] 6.4 Review mature library alternatives and avoid dependencies that do not materially reduce code or concepts
- [x] 6.5 Run formatting, static Rust checks, focused unit tests, strict OpenSpec validation, and a final fresh-eyes review

## 7. Explicit module boundaries

- [x] 7.1 Replace the remaining `xtask` source and unit-test `include!` fragments with ordinary Rust modules
- [x] 7.2 Split Caddy resolution and configuration composition behind the existing Caddy facade
- [x] 7.3 Split process lifecycle, transactional receipts, process I/O, and mock runtime implementations behind the existing runtime facade
- [x] 7.4 Split storage worker, persistence/recovery, and platform filesystem helpers behind the existing storage facade
- [x] 7.5 Split IPC connection, client protocol, stream, dispatch, handshake, and framing responsibilities behind the existing IPC facade
- [x] 7.6 Replace broad dead-code allowances with narrow self-checking expectations where the retained code is contract-owned
- [x] 7.7 Simplify the resulting module seams and run formatting, static checks, focused unit tests, strict OpenSpec validation, and a fresh-eyes review

## 8. Simple lifecycle foundation

- [x] 8.1 Replace the runtime guard, generation lock, file-discovery, and fail-stop containment requirements with the production endpoint-lease and retained-child model, and remove the superseded `secure-local-control-plane` and `simplify-runtime-location` change sets
- [x] 8.2 Remove dormant guard protocol, identity, record, legacy lock, file-discovery, and compatibility seams from the daemon
- [x] 8.3 Bound handler, runtime, and storage shutdown without indefinite containment retries
- [x] 8.4 Use `process-wrap` for both Unix process groups and Windows Job Objects and remove the custom Windows process-tree implementation
- [x] 8.5 Update architecture and runtime documentation, remove obsolete tests, and remove dependencies or platform features that no longer carry production behavior
- [x] 8.6 Run formatting, static checks, unit tests, strict OpenSpec validation, and a fresh-eyes review without starting Cadder or Caddy
- [x] 8.7 Resolve the separately scoped IPC wire-contract mismatch where the checked fixture declares 5 operations and the generated contract declares 12, then rerun `cargo test --workspace` and `cargo xtask check`; do not sync or archive this change until both commands pass
