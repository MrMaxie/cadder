---
impact: patch
components:
  - caddy-runtime
  - observability
  - daemon-lifecycle
audiences:
  - developers
  - operators
observable_impact: user-felt
changelog: include
---

## Fixed

### Keep managed development routes local and isolated

Cadder now binds its shared HTTP and HTTPS listeners only to loopback, keeps every project behind its active host names, redacts the documented credential forms from retained logs, and renders diagnostic text without active terminal control sequences.
