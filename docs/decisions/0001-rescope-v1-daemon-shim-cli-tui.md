# ADR 0001: Rescope Cadder to Daemon, Shim, CLI, and TUI

## Status

Accepted.

## Context

Cadder accumulated too many surfaces before the daemon and shim contract became
stable. The project had daemon code, PATH shim behavior, CLI/TUI/web/desktop
experiments, MCP assumptions, and Backlog.md task history competing for product
direction.

The active source of truth is the OpenSpec change
`reset-cadder-architecture`. This ADR records the durable decision in the docs
tree, but OpenSpec remains authoritative for requirements, design details, and
implementation tasks.

## Decision

Cadder is reset around three product surfaces:

- `cadderd`: daemon and owner of runtime state, real Caddy coordination, logs,
  and OS integrations.
- `caddy`: PATH-facing Caddy-compatible shim that routes Cadder-managed Caddy
  definitions to `cadderd`.
- `cadder`: operator executable with CLI and TUI workflows.

Web and Tauri GUI surfaces are deferred. They may return only as clients of the
same daemon protocol and reusable view-model contracts used by CLI and TUI.

MCP and Backlog.md are removed from product and planning scope. Automation for
people, scripts, and agents uses the `cadder` CLI and daemon protocol model.

Windows remains the primary platform target for now, especially least-privilege
operation, named-pipe security, autostart behavior, shim PATH behavior, Windows
Sandbox smoke tests. Cross-platform primitives remain
preferred where they do not hide platform security or lifecycle differences.

## Consequences

- The daemon is the single source of truth for Cadder-managed runtime state.
- The shim cannot silently start unmanaged Caddy for Cadder-managed commands.
- CLI and TUI must handle daemon-unavailable states as normal user-facing states.
- Bounded redacted logs remain available through the daemon protocol for
  diagnostics. The primary TUI does not expose a dedicated log view.
- Multiple Cadder runtimes are limited to explicit dev/debug profiles.
- Large custom orchestration must be reduced or justified against mature tools.
- Future implementation work starts from OpenSpec, not Backlog.md.
