---
impact: major
components:
  - runtime-storage
  - observability
  - caddy-runtime
audiences:
  - operators
observable_impact: user-felt
changelog: include
---

## Changed

### Keep Cadder state in one local database

Cadder stores project registration, activation state, and bounded recent logs in one owner-protected SQLite database. The portable binaries include SQLite and do not require a system SQLite installation.
