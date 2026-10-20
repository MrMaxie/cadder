## Why

The Node backend can prepare and apply Caddy configurations, but no product
handler registers projects or persists their intent. Preserve the released
Caddyfile -> managed shim -> daemon journey by joining the existing boundaries.

## Requirement IDs

- CADDY-003, CADDY-006: verified transactions and protected administration.
- REG-001 through REG-004: live session ownership and durable intent.
- STO-001 through STO-004: bounded SQLite state/log worker, no old-data import.
- OBS-001 through OBS-004: redacted bounded diagnostic streams.
- IPC-003, RUN-001 through RUN-003: existing RPC and owned lifetime.
- CLI-001, TUI-001 through TUI-004, TOP-002: shared client and current operator journey.
- SHIM-001 through SHIM-003, INSPECT-001 through INSPECT-004: command policy, managed sessions and local inspection.
- DIST-001/002/004/005, QT-001/003/006: compiled npm and native SEA candidates.
- DOC-001 through DOC-004: locked Nub documentation and candidate installation guidance.

## Scope

Application storage, registration handlers and daemon integration, followed by
the existing packaged CLI/TUI/shim journey, operator inspection and the two
non-publishing pack processes. Reuse the resolver, composition,
security, IPC and transaction queue. Remove self-testing contract mocks rather
than replacing them with another mock layer.

## Non-goals

No new RPC, state migration, feature expansion, publication or premature Rust
removal. Native acceptance remains open until it is executed.

## Success criteria

Registration, heartbeat, toggles, expiry and unregister share one transaction
boundary; stale sessions cannot replace a live owner. Restart restores intent
without routes. Rejected changes retain committed state. Diagnostic storage is
redacted, bounded and owner-protected. Focused integration and full checks pass.
Actual local/global npm wrappers and extracted Windows SEA entries work outside
the checkout. Record native evidence by platform and source rather than treating
prepared CI as execution. Full matrix, interactive privilege acceptance and the
Rust cutover remain parent-roadmap gates.

## Impact

Node daemon, storage and client/shim entrypoints; English implementation notes.
No additional production dependency or change to accepted contracts.
