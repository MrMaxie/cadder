---
impact: minor
components:
  - operator-cli
  - documentation-experience
audiences:
  - developers
  - operators
observable_impact: user-felt
changelog: include
---

## Added

### Start Cadder when opening the TUI

Run `cadder tui --start-daemon` to start or attach to the Cadder daemon in the background and open the interactive operator without an additional key press. Plain `cadder tui` keeps its existing read-first behavior.
