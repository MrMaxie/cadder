---
id: TASK-4
title: Support HTTPS-required IIS applications during handoff
status: Done
assignee:
  - '@agent'
created_date: '2026-06-12 10:32'
updated_date: '2026-06-15 14:34'
labels:
  - bug
  - iis
  - ssl
milestone: m-5
dependencies: []
references:
  - crates/cadder-daemon/src/caddy.rs
  - crates/cadder-daemon/src/iis.rs
  - crates/cadder-daemon/src/state.rs
  - docs/verification/tui-smoke.md
documentation:
  - docs/ARCHITECTURE.md
  - docs/site/src/content/docs/cookbooks/windows/iis.mdx
modified_files:
  - crates/cadder-protocol/src/lib.rs
  - crates/cadder-daemon/src/iis.rs
  - crates/cadder-daemon/src/state.rs
  - crates/cadder-daemon/src/caddy.rs
  - docs/ARCHITECTURE.md
  - docs/site/src/content/docs/cookbooks/windows/iis.mdx
  - docs/verification/tui-smoke.md
priority: high
ordinal: 28800
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Read-only verification on 2026-06-12 found an active IIS handoff route for `default.local-dev.thinksmart.com` on both Caddy `:80` and `:443`, with the upstream configured as plain `127.0.0.1:41956` reverse proxy traffic. Public HTTPS through Caddy reached IIS (`Server: Microsoft-IIS/10.0`, `Via: 1.1 Caddy`), direct HTTP to the loopback backend with the route host succeeded, and direct HTTPS to the same backend port failed the TLS handshake. Reading IIS bindings through `WebAdministration` without elevation was denied, so the live IIS binding details were not inspected. The current architecture and cookbook explicitly describe IIS handoff as creating a loopback HTTP backend binding. IIS applications or modules that require HTTPS can therefore see a non-TLS backend request and show `SSL is required` even when the browser used HTTPS to reach Caddy. Fix Cadder so HTTPS IIS handoff preserves HTTPS semantics for backend applications, or rejects/diagnoses unsupported SSL-required cases clearly before leaving a broken route.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 A handed-off IIS `https:443` binding whose application requires HTTPS can be reached through Caddy without failing solely because Cadder converted the IIS backend leg to plaintext HTTP.
- [x] #2 Cadder preserves or communicates the original HTTPS request semantics to IIS applications in a documented, testable way for both concrete host bindings and wildcard or empty-host bindings with an explicit route host.
- [x] #3 If Cadder cannot safely satisfy HTTPS-required IIS semantics for a binding, the daemon/TUI reports a typed issue with recovery guidance instead of silently producing a route that returns `SSL is required`.
- [x] #4 Existing IIS `http:80` handoff behavior, restore metadata, rollback behavior, and non-IIS Caddy route behavior remain unchanged.
- [x] #5 Automated tests cover the HTTPS IIS handoff path with fake IIS/Caddy boundaries, including an SSL-required application simulation or equivalent assertion, and the IIS handoff documentation/cookbook describes verification steps and limitations.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
## Goal
Support IIS HTTPS handoff without converting HTTPS-required applications to a plaintext backend leg. For HTTPS IIS bindings, Cadder should either preserve TLS semantics end to end between Caddy and IIS or reject the handoff with a typed, actionable issue before mutating IIS.

## Scope
- Preserve existing `http:80` IIS handoff behavior.
- Fix `https:443` IIS handoff for concrete host bindings and wildcard or empty-host bindings that receive an explicit route host.
- Keep restore metadata, rollback, daemon restart hydration, and non-IIS Caddy routes compatible with the current behavior.
- Update automated tests and IIS handoff documentation.

## Key Files
- `crates/cadder-daemon/src/iis.rs`: IIS binding model, backend binding construction, provider mutation scripts, restore metadata persistence.
- `crates/cadder-daemon/src/state.rs`: IIS handoff orchestration, route host handling, failure reporting, rollback and restore flow.
- `crates/cadder-daemon/src/caddy.rs`: generated Caddy reverse-proxy routes for IIS handoffs.
- `crates/cadder-protocol/src/lib.rs`: only if an existing issue kind is not expressive enough for the unsupported HTTPS case.
- `crates/cadder-tui/src/main.rs` and `crates/cadder-tui/src/model.rs`: only if a new issue kind or message path requires UI coverage.
- `docs/ARCHITECTURE.md`, `docs/site/src/content/docs/cookbooks/windows/iis.mdx`, `docs/verification/tui-smoke.md`: operator behavior and verification docs.

## Implementation Steps
1. Replace the HTTP-only backend helper with a backend binding planner that derives the loopback binding from the original IIS protocol.
2. For `http:80`, keep the existing `127.0.0.1:<port>` HTTP backend binding and Caddy plaintext reverse proxy behavior unchanged.
3. For `https:443`, create a loopback HTTPS backend binding on the same IIS site using the selected route host and copy the discovered TLS certificate metadata from the original binding.
4. Before writing restore metadata or mutating IIS, reject HTTPS handoff when the original binding does not expose usable TLS certificate metadata. Return a typed `IisIssue` with recovery guidance instead of producing a route that can fail as `SSL is required`.
5. Extend the IIS restore metadata and daemon restart hydration path to preserve and reuse the planned backend binding protocol, port, host, and TLS certificate data.
6. Extend `CaddyConfigCoordinator::set_iis_proxy_route` so IIS routes can carry backend protocol information. Generate a TLS-enabled HTTP transport for HTTPS IIS backends, with `server_name` set to the route host. Use loopback-appropriate TLS verification behavior and document the security tradeoff.
7. Keep rollback and restore batches operating from the stored backend binding rather than reconstructing an HTTP-only binding.
8. Update TUI-facing messages only if the new typed issue or success text needs clearer operator guidance.
9. Update architecture, cookbook, and smoke verification docs to describe HTTPS backend handoff, concrete host and wildcard route-host behavior, direct backend verification, and known limitations.

## Validation Steps
- Add focused unit tests in `iis.rs` for HTTP backend compatibility, HTTPS backend certificate preservation, wildcard or empty-host route host handling, and missing certificate rejection.
- Add focused tests in `caddy.rs` asserting that HTTP IIS routes remain plaintext and HTTPS IIS routes include TLS transport configuration.
- Add daemon orchestration tests in `state.rs` covering HTTPS concrete host handoff, HTTPS wildcard or empty-host handoff with explicit route host, typed rejection before mutation, daemon restart hydration, and restore or rollback behavior.
- Add or update protocol/TUI tests only if the response contract or rendered safety messages change.
- Run `cargo fmt --check`.
- Run focused Cargo tests for `cadder-daemon`, `cadder-protocol`, and `cadder-tui` as applicable.
- Run `cargo clippy --workspace --all-targets -- -D warnings`.
- Run `cargo test --workspace`.
- Run `cargo run -p xtask -- check`.
- Measure coverage with the project coverage workflow and confirm it remains at or above the 85% threshold.

## Risks And Checks
- Reusing IIS TLS certificate metadata must work for SNI and wildcard or empty-host bindings; otherwise the handoff must fail before mutation.
- Caddy proxies HTTPS upstreams over TLS only when its reverse proxy transport is configured for TLS. Plain `127.0.0.1:<port>` must not be used for HTTPS-required IIS backends.
- Loopback HTTPS certificates may not validate against `127.0.0.1`; the generated Caddy route should use the route host as TLS server name and document any local trust or insecure-verification choice.
- Restore and rollback must not lose metadata if IIS or Caddy apply fails mid-handoff.

## Out Of Scope
- No broad IIS management UI changes.
- No support for unsupported IIS protocols or non-80/non-443 ports.
- No changes to general Caddyfile adaptation or non-IIS route behavior unless required by the IIS route data model.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Implementation plan approved by the user and recorded before code changes.

Started implementation for TASK-4 after confirming the task is In Progress, assigned to @agent, and has an approved recorded plan. Initial code review found that IIS certificate discovery already exists, but backend planning and Caddy route generation still force IIS handoff backends to plaintext HTTP.

Implemented HTTPS-aware IIS handoff. HTTP handoff still uses plaintext loopback backends. HTTPS handoff now requires usable IIS TLS certificate metadata, creates an HTTPS loopback backend binding with the copied certificate metadata, persists that backend binding for restore/restart hydration, and configures Caddy's IIS proxy route with TLS transport using the route host as SNI and loopback-only insecure certificate verification. Missing HTTPS certificate metadata is reported as the typed issue `missingTlsCertificate` before metadata writes or IIS mutation.

Validation completed: `cargo fmt --check`; `cargo test -p cadder-daemon iis`; `cargo test -p cadder-protocol serializes_iis_handoff_contracts`; `cargo test -p cadder-daemon`; `cargo test -p cadder-protocol`; `cargo test -p cadder-tui`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`; `cargo run -p xtask -- check`; `cargo run -p xtask -- coverage`. Coverage summary reports total line coverage 86.83187020780086%, above the 85% threshold.

Closeout validation on 2026-06-15 completed before commit. Fresh-eyes review found no substantive local issues to patch. Verified with `git diff --check`, `cargo fmt --check`, `cargo test -p cadder-daemon iis`, `cargo test -p cadder-protocol serializes_iis_handoff_contracts`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo run -p xtask -- check`, and `cargo run -p xtask -- coverage`. Coverage summary reports total line coverage 86.83187020780086%, above the 85% threshold.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
## Summary
- Added HTTPS-aware IIS handoff so Cadder creates a TLS loopback IIS backend for `https:443` bindings and configures Caddy to use TLS to that backend with the route host as SNI.
- Preserved existing HTTP IIS handoff and non-IIS Caddy route behavior while keeping restore metadata, rollback, restart hydration, and restore flows tied to the stored backend binding.
- Added typed `missingTlsCertificate` handling so HTTPS handoff is rejected before metadata writes or IIS mutation when usable TLS certificate metadata is unavailable.
- Updated architecture, IIS cookbook, and TUI smoke verification docs with HTTPS backend behavior, loopback verification, certificate requirements, and limitations.

## Validation
- `git diff --check`
- `cargo fmt --check`
- `cargo test -p cadder-daemon iis`
- `cargo test -p cadder-protocol serializes_iis_handoff_contracts`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo run -p xtask -- check`
- `cargo run -p xtask -- coverage` reports total line coverage 86.83187020780086%, above the 85% threshold.

## Risks And Follow-ups
- HTTPS upstream certificate verification is intentionally skipped only for the local loopback hop because IIS certificates commonly do not validate for `127.0.0.1`; this is documented as a local development tradeoff.
- No follow-up task is required for TASK-4 closeout.
<!-- SECTION:FINAL_SUMMARY:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [x] #1 Tests or explicit verification were run for the changed behavior
- [x] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
