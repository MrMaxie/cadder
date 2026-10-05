---
impact: minor
components:
  - distribution-and-upgrades
  - documentation-experience
  - quality-tooling
audiences:
  - developers
  - operators
observable_impact: user-felt
changelog: include
---

## Added

### Install Cadder through npm

Run `npx cadder` for one-off operator access or install `cadder` globally to place the version-matched `cadder`, `cadderd`, and Cadder `caddy` shim commands on PATH. npm packages use the same verified native binaries as the portable GitHub archives; the upstream Caddy server remains a separate installation.
