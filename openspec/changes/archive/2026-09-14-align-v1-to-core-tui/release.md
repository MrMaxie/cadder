---
impact: major
components:
  - product-contract
  - operator-tui
  - local-control-plane
audiences:
  - developers
  - operators
observable_impact: user-felt
changelog: include
---

## Added

### Run project Caddyfiles together

Keep a Caddyfile in each repository and use the normal `caddy run` command. Cadder's PATH shim sends each project's routes to `cadderd`, which checks ownership and conflicts, combines the active configuration, and applies it to one separately installed Caddy server. Projects can start and stop independently without competing for the same local HTTP and HTTPS ports.

### See every route in the TUI

`cadder tui` shows registered projects, domains, upstreams, activation state, daemon state, and Caddy state in one workspace. It can enable or disable one project or route without changing the others.

## Removed

### Keep the 1.0 surface focused

Profiles, autostart, history, export, continuous log tailing and watching, machine-readable output, and MCP are outside Cadder 1.0.
