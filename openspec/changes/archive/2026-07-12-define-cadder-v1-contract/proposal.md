## Why

Cadder has working daemon, shim, protocol, and operator building blocks, but it
does not have an accepted main specification for the product they form. The
previous architecture change mixed target requirements with implementation
progress, which made documentation, task status, and release readiness drift
apart.

Cadder needs one stable 1.0 contract before further implementation. Stable
requirement IDs let focused changes prove one slice at a time while product
documentation describes the accepted release behavior consistently.

## What changes

- Define the complete target behavior for the three Cadder 1.0 surfaces:
  `cadderd`, the Caddy compatibility shim, and the `cadder` CLI/TUI operator.
- Define security, lifecycle, Caddy integration, storage, observability, IIS,
  distribution, and documentation contracts as separate capabilities.
- Give every requirement a stable identifier that implementation changes,
  tests, documentation, and verification evidence can reference.
- Record supported platforms and release boundaries without claiming that the
  current implementation already satisfies the contract.
- Establish main specs as the accepted Cadder 1.0 target. Active implementation
  changes remain the source for delivery status and evidence.

## Scope

This change defines observable product and release behavior. It covers Windows
x64, Linux x64, macOS x64, and macOS arm64. IIS integration applies only to
Windows. Future local clients may reuse the protocol and view models, but 1.0
ships only the daemon, shim, CLI, and TUI.

## Non-goals

- Implementing or refactoring production code.
- Shipping Web, Tauri, remote API, or MCP product surfaces.
- Bundling the real Caddy executable.
- Defining DEB, RPM, PKG, or an in-application updater for 1.0.
- Publishing documentation, packages, tags, or releases.

## Success criteria

- Twelve focused capability specs exist and pass strict OpenSpec validation.
- Every requirement has one unique stable ID and at least one observable
  scenario.
- The specs define product behavior without workstation-specific instructions
  or unresolved implementation decisions.
- Archiving this completed contract change populates `openspec/specs/` without
  marking implementation work complete.

## Capabilities

### New capabilities

- `product-topology`: Product surfaces, ownership boundaries, supported
  platforms, and excluded 1.0 surfaces.
- `daemon-lifecycle`: Per-user daemon identity, profiles, locking, lifecycle,
  autostart, and degraded operation.
- `project-registration`: Shim registration, command policy, heartbeats,
  conflicts, disconnects, and safe alias setup.
- `local-control-plane`: Local transport security, discovery, framing,
  compatibility, capabilities, errors, limits, and shutdown.
- `caddy-runtime`: Trusted real-Caddy resolution, supported configuration,
  private administration, transactional apply, recovery, and drift handling.
- `runtime-storage`: Durable state, schema migration, integrity, backup, and
  corruption recovery.
- `operator-cli`: Command hierarchy, output contracts, exit codes, diagnostics,
  and platform-specific behavior.
- `operator-tui`: Shared view models, real runtime state, interaction,
  responsive layout, and accessible alternatives.
- `observability`: Structured events, redaction, queries, tailing, history,
  backpressure, and retention.
- `windows-iis-handoff`: Preview, scoped elevation, authenticated execution,
  apply, restore, rollback, and unsupported-platform behavior.
- `distribution-and-upgrades`: Release application layout, installers,
  platform matrix, upgrades, uninstall, signing, SBOM, and provenance.
- `documentation-experience`: Audience boundaries, present-tense target
  documentation, executable examples, accessibility, and publication gates.

### Modified capabilities

None. Cadder has no existing main specs.

## Evidence and assumptions

Confirmed repository evidence:

- The workspace contains separate daemon, shim, protocol, operator service, and
  operator executable packages.
- The archived `reset-cadder-architecture` change preserves 20 completed tasks
  and 23 open tasks without updating main specs.
- Current validation covers the workspace and documentation, but release-facing
  package coverage and delivery gates remain incomplete.

Accepted product assumptions:

- Apache-2.0 is the project license.
- The normal daemon runs without elevation; IIS uses a one-shot helper.
- User documentation describes the accepted 1.0 product in present tense and
  stays unpublished until the release gate passes.

## Impact

- `openspec/specs/` becomes the stable product contract for all subsequent work.
- Implementation changes reference accepted requirement IDs and archive without
  modifying main specs.
- Public CLI, IPC DTOs, storage schemas, installer behavior, and documentation
  gain explicit compatibility and verification obligations.
- Existing code and documentation are evaluated against this contract through
  the focused implementation changes that follow.
