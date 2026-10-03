## Why

Cadder coordinates trusted local development projects, but its generated Caddy configuration currently widens local routes to all network interfaces, can retain sibling catch-all branches, and leaves several bounded-output and presentation guarantees incomplete. The existing `caddy run` journey must remain unchanged while the daemon enforces the local-only and owner-selected boundaries promised by the product.

## What Changes

- Bind Cadder-owned HTTP and HTTPS servers to loopback without changing their ports or the managed `caddy run` workflow.
- Place each adapted project route set behind an exact active-host guard.
- Bound `caddy adapt` output, redact the credential forms named by the observability contract, and neutralize terminal control characters only when the CLI renders diagnostic text.
- Launch the managed daemon only from the version-matched sibling executable or an explicit diagnostic override, without automatic PATH fallback.
- Raise the minimum tested Caddy patch release to 2.11.4 while retaining the existing trusted Caddy resolution and loopback administration endpoint.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `caddy-runtime`: Cadder-owned listeners and composed project routes remain local and host-scoped.
- `observability`: Required credential forms are redacted before retention, while terminal rendering is control-character safe.
- `daemon-lifecycle`: Managed startup uses the version-matched sibling daemon rather than PATH discovery.

## Impact

This change affects Caddy configuration composition, adaptation process limits, daemon launch resolution, log redaction, CLI rendering, focused runtime tests, and the tested Caddy version. It does not change the IPC schema, Caddy administration transport, public commands, ports, or installation model.
