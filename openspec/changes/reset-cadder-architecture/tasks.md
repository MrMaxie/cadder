## Node migration implementation

Earlier completed tasks below refer to the Rust baseline. They do not prove the
Node migration. The approved target and gates are in `node-migration.md`.

- [x] N1. Update OpenSpec target, behavior evidence and TS/Nub tooling with exact dependencies.
- [ ] N2. Implement v2 owner-protected runtime, SQLite exclusion, mutual HMAC IPC and system gate tests.
  - [x] N2.1 Implement runtime/IPC foundation and run Windows unit, regression and native child-process tests.
  - [x] N2.2 Run the compiled-JS native runtime gate on Linux x64 with real Unix sockets and permissions.
  - [x] N2.3 Pass the native Windows/Linux/macOS CI matrix on the committed code.
  - [ ] N2.4 Verify same-owner non-elevated contact with elevated daemon and different-account denial in system tests, including Windows Sandbox.
- [ ] N3. Port Caddy composition, transaction queue, loopback mTLS and owned-process lifecycle.
- [ ] N4. Port shim policy, safe resolution, heartbeat, autostart and scoped IIS helpers.
- [ ] N5. Implement history worker, bounded/redacted logs, shared client service and real CLI/TUI.
- [ ] N6. Verify npm staging and all four SEA releases, native consumer smoke and documentation.
- [ ] N7. Pass >=85% coverage, three-platform integration and Windows Sandbox acceptance.
- [ ] N8. Remove Rust/Cargo and legacy packaging only after N1-N7 gates pass.
- [ ] N9. Prepare matching rc.1 artifacts without publishing; await separate release authorization.

## 1. Planning Migration (Rust baseline)

- [x] 1.1 Keep `openspec/` initialized and document it as the repository planning source of truth.
- [x] 1.2 Preserve relevant architecture rationale in OpenSpec proposal, design, and specs.
- [x] 1.3 Remove obsolete planning artifacts and workflow references from project-facing docs.
- [x] 1.4 Remove obsolete product-surface assumptions from repo instructions, docs, release metadata, and validation tasks.
- [x] 1.5 Update `docs/ARCHITECTURE.md` so it follows this OpenSpec change or explicitly points to OpenSpec as authoritative.

## 2. Workspace Topology

- [x] 2.1 Audit every workspace crate and classify it as daemon, shim, operator client, shared protocol/API, test support, docs/tooling, or obsolete.
- [x] 2.2 Remove, merge, or justify crates that do not map to a current product, library, test, docs, or tooling responsibility.
- [x] 2.3 Decide whether `cadder-operator` remains as an internal client library or is renamed/merged into the final `cadder` boundary.
- [x] 2.4 Fix workspace membership and dependency warnings after topology cleanup.
- [x] 2.5 Add validation that prevents reintroducing undocumented product crates.

## 3. Protocol And Daemon

- [x] 3.1 Split protocol code into envelopes, commands, responses, events, errors, log queries, and client traits.
- [x] 3.2 Replace hard protocol equality checks with additive compatibility and typed unsupported-capability errors.
- [x] 3.3 Extract daemon state concerns into focused modules for registrations, config composition, process lifecycle, runtime status, logs, and storage.
- [x] 3.4 Define and implement production runtime locking with stale-lock recovery.
- [x] 3.5 Define local IPC discovery and security for user clients contacting privileged runtime endpoints.
- [x] 3.6 Add daemon contract tests with fake Caddy, fake storage, fake IPC clients, and fake IIS providers.

## 4. Shim And Caddy Integration

- [x] 4.1 Write the shim command policy table for managed, read-only, passthrough, and unsupported Caddy commands.
- [x] 4.2 Implement managed shim commands as requests to `cadderd` without starting unmanaged Caddy.
- [x] 4.3 Implement daemon-unavailable diagnostics and recovery guidance for shim commands.
- [x] 4.4 Harden real Caddy resolution against recursive shim execution.
- [ ] 4.5 Implement config composition and atomic apply behavior with last-known-good recovery.
- [ ] 4.6 Add shim tests for command policy, no-daemon behavior, fallback behavior, and recursion prevention.

## 5. Operator Clients

- [ ] 5.1 Decide and document the `cadder` invocation model for CLI and TUI.
- [ ] 5.2 Build a mockable operator client service boundary and reusable view models.
- [ ] 5.3 Implement CLI unavailable-daemon behavior with actionable start guidance.
- [ ] 5.4 Implement TUI unavailable-daemon behavior with a visible start action.
- [ ] 5.5 Add CLI tests with fake daemon responses and stable output snapshots.
- [ ] 5.6 Add TUI rendering and interaction tests with Ratatui test backend fixtures.

## 6. Logs

- [ ] 6.1 Define the structured log record schema and redaction policy.
- [ ] 6.2 Implement daemon-side log query and tail semantics by all logs, runtime, project, domain, source, and severity.
- [ ] 6.3 Make per-domain log mapping accurate for multi-domain Caddy sites.
- [ ] 6.4 Expose equivalent log workflows in CLI and TUI.
- [ ] 6.5 Add tests proving severity filtering equivalence across CLI and TUI.

## 7. Windows And IIS

- [ ] 7.1 Extract IIS discovery, handoff, restore, and status into a mockable provider boundary.
- [ ] 7.2 Choose and implement the scoped elevation model for IIS operations.
- [ ] 7.3 Keep normal Cadder usage fully user-level when IIS handoff is not enabled.
- [ ] 7.4 Add local tests for IIS policy and fake-provider failure recovery.
- [ ] 7.5 Add system smoke coverage for IIS handoff, privileged runtime contact, autostart, and shim PATH behavior.

## 8. Tooling, CI, And Release

- [ ] 8.1 Decide which custom tooling responsibilities move to mature external tools and which remain project-specific.
- [ ] 8.2 Shrink `xtask` into small modules or a compatibility wrapper with tests.
- [ ] 8.3 Add or update CI jobs for formatting, linting, tests, coverage, docs, Windows Sandbox documentation, and release metadata.
- [ ] 8.4 Add file-size/cohesion validation with documented exceptions for generated files.
- [ ] 8.5 Verify the docs site builds and advertises only workflows covered by accepted specs or active changes.
