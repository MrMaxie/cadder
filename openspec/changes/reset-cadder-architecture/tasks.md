## 1. Planning Migration

- [ ] 1.1 Keep `openspec/` initialized and document it as the repository planning source of truth.
- [ ] 1.2 Preserve relevant architecture rationale in OpenSpec proposal, design, and specs.
- [ ] 1.3 Remove obsolete planning artifacts and workflow references from project-facing docs.
- [ ] 1.4 Remove obsolete product-surface assumptions from repo instructions, docs, release metadata, and validation tasks.
- [ ] 1.5 Update `docs/ARCHITECTURE.md` so it follows this OpenSpec change or explicitly points to OpenSpec as authoritative.

## 2. Workspace Topology

- [ ] 2.1 Audit every workspace crate and classify it as daemon, shim, operator client, shared protocol/API, test support, docs/tooling, or obsolete.
- [ ] 2.2 Remove, merge, or justify crates that do not map to a current product, library, test, docs, or tooling responsibility.
- [ ] 2.3 Decide whether `cadder-operator` remains as an internal client library or is renamed/merged into the final `cadder` boundary.
- [ ] 2.4 Fix workspace membership and dependency warnings after topology cleanup.
- [ ] 2.5 Add validation that prevents reintroducing undocumented product crates.

## 3. Protocol And Daemon

- [ ] 3.1 Split protocol code into envelopes, commands, responses, events, errors, log queries, and client traits.
- [ ] 3.2 Replace hard protocol equality checks with additive compatibility and typed unsupported-capability errors.
- [ ] 3.3 Extract daemon state concerns into focused modules for registrations, config composition, process lifecycle, runtime status, logs, and storage.
- [ ] 3.4 Define and implement production runtime locking with stale-lock recovery.
- [ ] 3.5 Define local IPC discovery and security for user clients contacting privileged runtime endpoints.
- [ ] 3.6 Add daemon contract tests with fake Caddy, fake storage, fake IPC clients, and fake IIS providers.

## 4. Shim And Caddy Integration

- [ ] 4.1 Write the shim command policy table for managed, read-only, passthrough, and unsupported Caddy commands.
- [ ] 4.2 Implement managed shim commands as requests to `cadderd` without starting unmanaged Caddy.
- [ ] 4.3 Implement daemon-unavailable diagnostics and recovery guidance for shim commands.
- [ ] 4.4 Harden real Caddy resolution against recursive shim execution.
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
