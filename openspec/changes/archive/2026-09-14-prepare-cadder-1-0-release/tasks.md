## 1. Release Contract

- [x] 1.1 Add a project-local release-bearing `spec-driven` schema and validate it.
- [x] 1.2 Record the candidate, native verification, attestation, and publication requirements.
- [x] 1.3 Add public release metadata to the operator inspection change.

## 2. Test Isolation

- [x] 2.1 Give each operator integration-test process a unique runtime directory and controlled binary PATH.
- [x] 2.2 Add bounded command waits and best-effort isolated daemon cleanup.
- [x] 2.3 Prove the operator test cannot connect to or stop the user's running daemon.

## 3. Release Automation

- [x] 3.1 Configure cargo-dist for pull-request uploads, announce-phase publication, enforced pre-announce verification, and attestations.
- [x] 3.2 Add native reusable verification for all four portable archives.
- [x] 3.3 Pin generated workflow actions to full commit SHAs and regenerate the workflow with cargo-dist.
- [x] 3.4 Validate cargo-dist planning and build the local target archive without publishing.

## 4. Release Story

- [x] 4.1 Reconcile accepted public outcomes since 0.8.0 without exposing internal implementation history.
- [x] 4.2 Sync and archive the operator inspection change after its isolated test gate passes.
- [x] 4.3 Complete and validate this release-preparation change so it can be synced and archived before the release cut.
- [x] 4.4 Verify that the accepted release inputs resolve from 0.8.0 to 1.0.0.

## 5. Final Local Gate

- [x] 5.1 Run focused checks for the release-preparation implementation before archiving it.
- [x] 5.2 Review the release-preparation diff and keep every external repository mutation outside this change.
