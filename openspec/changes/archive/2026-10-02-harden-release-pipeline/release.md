---
impact: patch
components:
  - quality-tooling
  - distribution-and-upgrades
audiences:
  - maintainers
observable_impact: maintenance
changelog: omit
---

## Changed

### Harden release credentials and artifact provenance

Release builds now use least-privilege GitHub permissions and validated tag data, while npm assembly accepts only assets attested for the exact selected release commit by Cadder's release workflow.
