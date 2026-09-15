---
category: changed
impact: patch
visibility: internal
components:
  - daemon-lifecycle
  - local-control-plane
---

# Clarify bounded shutdown containment

The daemon lifecycle contract now defines bounded fail-stop shutdown and containment behavior without transferring ownership of unrelated processes.
