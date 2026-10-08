---
impact: major
components:
  - distribution-and-upgrades
  - daemon-lifecycle
  - operator-cli
  - operator-tui
audiences:
  - developers
  - operators
observable_impact: user-felt
changelog: include
---

These are planned 2.0 outcomes, not shipped functionality. Both channels target
2.0.0-rc.1 first and 2.0.0 only after acceptance and separate publication approval.

## Changed

### Install one application through npm or standalone downloads

Cadder 2.0 will provide the same operator, daemon and Caddy shim through one npm
package requiring Node 24 LTS >=24.18.0 or standalone GitHub downloads requiring
no installed Node/npm. Upstream Caddy will remain a separate prerequisite.

### Switch runtimes without deleting the previous installation's data

Before switching, users will stop the previous daemon. Projects will register
again from their existing configuration files. Previous runtime data and releases
will remain available, but the new daemon will not import the old database or
communicate with the Rust daemon.

The current command scope will remain: bare `cadder` prints help, `cadder tui`
opens the routes workspace, and `cadder tui --start-daemon` starts or attaches
before opening it. Logs and diagnostics remain explicit CLI commands. The
migration will not add history, IIS, autostart or machine-output commands.

## Fixed

### Keep Caddy administration within the authenticated daemon boundary

Cadder 2.0 will protect its local Caddy administration channel with mutual TLS
and retain the previous verified configuration when an update is rejected.
