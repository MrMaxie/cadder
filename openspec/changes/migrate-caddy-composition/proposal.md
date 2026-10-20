## Why

Preserve the released project Caddyfile -> managed shim -> daemon -> operator
journey. The next capability combines already adapted project routes into a
local, host-guarded route plan and rejects competing active domain owners.
Caddy remains the mandatory separately installed server; this migrates Cadder's
integration, not Caddy itself.

## Requirement IDs

- `CADDY-002`: Compose active non-conflicting project routes (composition only).
- `CADDY-004`: Preserve exact active-host outer guards and IPv4/IPv6 loopback listeners.
- `REG-002`: Canonical domain ownership across active entrypoints.
- `REG-004`: Preserve existing adapted Caddyfile trees and activation behavior.

## Scope

Implement roadmap 3.3 as pure Node Caddy composition/domain functions and focused
tests. Consume the existing registration DTO and complete adapted JSON carriers.
Produce a typed route plan with the retained HTTP/HTTPS server shapes and TLS
subjects, or diagnostics without a usable plan. No process or active-state changes.

The completed protocol and preparation slices remain unarchived because of the
known custom implementation-schema validation blocker. This slice consumes their
leaf contracts without modifying them. Dependency order is explicit; only one
writer uses the checkout, and parent-owned planning/evidence is separate from
worker-owned new composition source/tests.

## Non-goals

No placeholder production routes, Caddyfile parser, adapter subprocess changes,
mutation queue, apply/rollback, storage, registration sessions, Admin API,
certificate generation, server startup, CLI/TUI, dependencies, Rust changes,
packaging, Sandbox or host security changes. A route plan is not an executable
full Caddy configuration and must not be passed to the validation/apply port as
one. Protected administration remains roadmap 3.6-3.7.

## Success criteria

- Canonical case, trailing dots and IDN spelling identify the same domain owner.
- Only registered/activating/active registrations and domains contribute routes.
- Conflicts reject the complete plan; inactive entries cannot cause a conflict.
- Each complete retained registration route tree remains behind its own active
  canonical hosts, including hostless siblings; disabled-only branches cannot
  become hostless catch-alls.
- Project listeners/admin/TLS settings cannot widen the composed route boundary;
  owned listeners are exactly 127.0.0.1/[::1] on ports 80 and 443.
- Input JSON/registrations remain unchanged; missing or invalid prepared input
  does not silently become a placeholder route.
- Focused and full relevant Node checks pass with >=85% own-code line coverage;
  independent MiMo review findings are reconciled.

## Impact

New Node Caddy composition/domain modules and dedicated tests; focused OpenSpec
plan/evidence and the existing roadmap 3.3 completion row only. No new product
requirements, RPC methods or public user commands. G1/G2/G3 and final native,
real-Caddy, distribution and Sandbox acceptance remain open.
