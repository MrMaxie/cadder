---
id: TASK-5
title: Handle transient IIS restore batch file-in-use failures
status: To Do
assignee: []
created_date: '2026-06-15 18:16'
labels:
  - bug
  - iis
  - windows
  - elevation
milestone: m-5
dependencies:
  - TASK-1.24
  - TASK-4
references:
  - crates/cadder-daemon/src/state.rs
  - crates/cadder-daemon/src/iis.rs
  - crates/cadder-daemon/src/caddy.rs
  - crates/cadder-protocol/src/lib.rs
  - crates/cadder-tui/src/main.rs
documentation:
  - docs/ARCHITECTURE.md
  - docs/site/src/content/docs/cookbooks/windows/iis.mdx
  - docs/verification/tui-smoke.md
  - >-
    backlog/tasks/task-1.24 -
    Support-mixed-elevation-operations-with-minimal-admin-prompting.md
  - >-
    backlog/tasks/task-4 -
    Support-HTTPS-required-IIS-applications-during-handoff.md
priority: high
ordinal: 29800
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
A Windows IIS HTTPS handoff restore can fail after administrator approval when the privileged IIS restore batch hits a transient file-in-use or port-owner condition such as HRESULT 0x80070020. In the observed failure, Cadder restored the Caddy proxy route and exposed RetryRestore/RetryElevation follow-ups, but recovering the machine still required manual operator cleanup: free the front-door HTTPS port, recreate the original IIS public binding, remove the loopback backend binding, and clear Cadder handoff metadata. Improve the restore failure and retry path so Cadder remains recoverable and can complete the restore after the external lock is cleared without manual metadata edits. Related completed context: TASK-1.24 provides mixed-elevation step reporting and TASK-4 provides the HTTPS IIS backend/restore metadata model.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 A privileged IIS restore batch failure caused by file-in-use diagnostics such as HRESULT 0x80070020 leaves the handoff recoverable: restore metadata is retained until successful restore, the binding remains reported as handed off or recoverable, and the Caddy IIS proxy route is restored if Cadder still owns the front door.
- [ ] #2 The daemon response reports accurate operation steps for this failure: restore metadata read and proxy-route removal are represented separately from failed privileged IIS mutation steps, approved admin status is preserved, metadata cleanup is skipped, and follow-ups include retry restore and retry elevation when applicable.
- [ ] #3 Retrying restore after the transient file lock or port owner is cleared succeeds without manual runtime-state edits: the public IIS binding is restored, the loopback backend binding is removed, the proxy route is removed, and restore metadata is cleared only after the restore completes.
- [ ] #4 The TUI status message and IIS row state accurately summarize the restore failure and available recovery actions without implying that restore completed successfully.
- [ ] #5 Automated tests cover restore-binding failure and loopback-cleanup failure inside the privileged restore batch using fake IIS/Caddy boundaries; real IIS verification, if performed, is documented as a disposable Windows smoke scenario.
<!-- AC:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [ ] #1 Tests or explicit verification were run for the changed behavior
- [ ] #2 Coverage was measured and remains at or above the project threshold
<!-- DOD:END -->
