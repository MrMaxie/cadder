---
impact: major
components:
  - operator-cli
  - operator-inspection
  - product-topology
audiences:
  - developers
  - operators
observable_impact: user-felt
changelog: include
---

## Added

### Inspect and control the local Cadder runtime

Use focused terminal commands to inspect projects, domains, Caddyfiles, upstream ports, daemon state, diagnostics, and bounded redacted logs. Cadder can also stop a known local port owner after the process identity is explicitly supplied and revalidated. The TUI remains the overview for interactive work.
