---
impact: patch
components:
  - distribution-and-upgrades
  - quality-tooling
audiences:
  - maintainers
observable_impact: maintenance
changelog: omit
---

## Changed

### Gate portable releases before publication

Release pull requests now expose the complete candidate artifact set for review, while tagged archives must pass native checksum, content, permission, version, and help checks before GitHub publishes the release. Published assets also carry GitHub artifact attestations.
