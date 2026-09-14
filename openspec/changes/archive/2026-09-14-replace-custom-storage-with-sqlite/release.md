---
category: changed
impact: major
visibility: public
components:
  - runtime-storage
  - observability
  - caddy-runtime
---

# Replace custom storage with SQLite

Cadder stores durable activation and bounded recent logs in one owner-protected SQLite database instead of maintaining a custom segmented file store.
