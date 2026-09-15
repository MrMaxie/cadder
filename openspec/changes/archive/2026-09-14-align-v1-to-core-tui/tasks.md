## 1. Local client and protocol

- [x] 1.1 Reduce `cadder-ipc` to the exact-version handshake, one typed request envelope, one correlated response envelope, and the eight retained operations; cover decoding, correlation, bounds, and incompatibility with focused tests
- [x] 1.2 Remove legacy envelopes, capability negotiation, minor-version compatibility, subscriptions, paging, snapshot tokens, history, autostart, export, watch, and unused wire DTOs; verify no removed operation remains in the generated contract fixture
- [x] 1.3 Move client transport, handshake, request execution, daemon launch, and readiness polling behind `cadder-api`; verify focused client tests with fake endpoints and launchers
- [x] 1.4 Remove daemon implementation dependencies from the shim and operator and remove duplicate presentation view models; verify the affected crates build independently against `cadder-api` and `cadder-ipc`

## 2. One runtime and managed registration

- [x] 2.1 Remove public profile selection and the single-variant runtime-profile model while preserving owner authentication, endpoint leases, test seams, and independent installation-directory identities
- [x] 2.2 Reduce trusted configuration to one immutable non-profile runtime snapshot and align real-Caddy resolution without admitting project-controlled selectors
- [x] 2.3 Make supported `caddy run` start the archive-owned daemon when attachment fails, perform the exact-version readiness check, and retain bounded registration, heartbeat, reconnect, and detach behavior
- [x] 2.4 Remove shim alias setup, forget, tombstone, historical identity, and unretained command guidance while preserving domain conflict and real-Caddy delegation safety
- [x] 2.5 Verify managed run, reconnect, expiry, activation persistence, domain conflicts, daemon launch failure, mixed-version rejection, and unrelated-Caddy protection with fake Caddy fixtures

## 3. Operator TUI

- [x] 3.1 Limit the `cadder` parser and help output to help, version, and `tui`, removing machine output, runtime selection, and unavailable command code
- [x] 3.2 Map bounded daemon responses directly into Status, Domains, and Logs views and remove paging, subscriptions, filters, history, tail, export, help-overlay, and CLI-equivalence models not retained by the specs
- [x] 3.3 Implement explicit idempotent Start, confirmed bounded Stop, and ordered Restart actions through the shared client boundary, including expected-disconnect handling
- [x] 3.4 Preserve keyboard-only navigation, visible focus, text-independent color meaning, bounded layouts, pending mutation state, stale/offline states, and terminal restoration; verify focused TUI state and rendering tests

## 4. Documentation and release surface

- [x] 4.1 Update architecture and public documentation to the portable archive, trusted real-Caddy configuration, managed `caddy run`, TUI, manual upgrade, and foreground diagnostic journeys
- [x] 4.2 Remove public CLI, profile, autostart, history, export, tail, alias-setup, installer, mock-backend, hidden-flag, and machine-contract claims; verify audience and private-context boundaries
- [x] 4.3 Align portable archive contents and verification with version-matched `cadder`, `cadderd`, and `caddy`, license, sample configuration, and SHA-256 checksum while preserving the existing release mechanism

## 5. Verification and handoff

- [x] 5.1 Run focused tests after each protocol, registration, client, and TUI slice without starting a real Cadder or Caddy runtime
- [x] 5.2 Run formatting, workspace static checks, workspace tests, coverage at the repository threshold, strict OpenSpec validation, documentation validation, and portable archive verification
- [x] 5.3 Review the final diff for removed-contract remnants, cross-platform ownership regressions, local-context leakage, accidental generated artifacts, and changes belonging to SQLite, mutation-actor, or repository-tooling follow-ups
- [x] 5.4 Do not mark the 1.0 foundation release-ready until `replace-custom-storage-with-sqlite` is applied and its migration verification passes
