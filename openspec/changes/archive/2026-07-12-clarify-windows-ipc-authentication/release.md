---
category: security
impact: patch
visibility: internal
components:
  - local-control-plane
---

# Clarify Windows IPC authentication

The local control-plane contract now distinguishes the fixed Windows transport-authentication preface from protocol framing and keeps identity verification ahead of request decoding and dispatch.
