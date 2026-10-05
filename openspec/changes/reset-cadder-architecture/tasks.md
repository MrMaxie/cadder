# Cadder 2.0 implementation tasks

Only Node migration work is counted here. Previous N1-N9 map to groups 1-9;
historical Rust checkboxes are available in Git snapshot `f850418` and the
archived Rust reset. Checked foundation tasks cite existing evidence, not a
completed product. Every remaining task needs its stated verification before
its checkbox can be checked. Gate dependencies are in `acceptance.md`.

## 1. Contract, behavior catalog and TypeScript base (G1)

- [x] 1.1 Reconcile and strictly validate proposal, Node design, delta specs, release story, tasks and acceptance against the corrected released-1.0.5 functional scope; review that superseded features no longer govern this change. Documentation-only evidence is in verification.md under Scope correction verification; prior broader planning evidence is superseded.
- [x] 1.2 Establish the root TS/TSX ESM workspace with exact Nub dependencies/lock, TypeScript 6.0.3, typecheck, lint, formatter, Vitest/V8 and outside-checkout coverage; foundation evidence is in verification.md.
- [ ] 1.3 Freeze fixtures for released 1.0.5 CLI grammar/exit codes and human output, bare help, tui --start-daemon, unsupported-command rejection, configuration precedence, shim argument streams, project/domain schemas and eight-operation IPC; compare expected Node behavior with current source evidence, not superseded historical scope (CLI-001, REG-004, SHIM-001, INSPECT-001..004).
- [ ] 1.4 Review and archive the contract deltas into main specs before further product implementation; preserve the open roadmap as planning, not completed implementation, and create only one focused implementation slice at a time, referencing stable requirement IDs rather than duplicating contracts.
- [ ] 1.5 Define mockable Caddy, platform, storage and client-service ports, typed method/result/error catalog and ownership/import boundaries; prove with contract tests before handlers depend on them (TOP-001/002, IPC-003).

G1 exit: reviewed target, reproducible tooling and compatibility fixture catalog.
Planning readiness alone does not close 1.3-1.5 or any runtime gate.

## 2. Runtime, exclusion and authenticated IPC (G2)

- [x] 2.1 Implement isolated v2 paths, owner-only secret creation and fail-closed Unix/Windows permission adapters; unit/regression evidence exists (IPC-001, RUN-005).
- [x] 2.2 Hold a separate SQLite BEGIN EXCLUSIVE lifetime lock; recover diagnostic residue only after acquisition and release resources after failed start/shutdown; exclusion and crash tests exist (RUN-001).
- [x] 2.3 Implement protocol 3/security policy 2 mutual HMAC with fresh directional challenges and bounded, sequenced NDJSON transport; fake endpoint, wrong proof, replay and pre-auth RPC tests exist (IPC-001/002).
- [ ] 2.4 Tighten the generic fixture RPC into the closed product catalog with Zod, bounded params/results, unknown-method/field rejection and exactly one correlated result/error; test malformed and incompatible peers (IPC-003).
- [ ] 2.5 Verify startup/teardown phase failure cleanup, runtime link/owner/ACL denial and full disk/storage failure without claiming unsafe state ready; verify independent endpoints/databases for separate installations using targeted fault tests plus native filesystem cases (TOP-001, RUN-001/003, STO-003).
- [x] 2.6 Run compiled-JS child/process/SQLite/socket gates on Windows x64, Linux x64 and macOS arm64 without a TS loader; existing CI evidence is linked in verification.md.
- [x] 2.7 Verify explicit-owner root daemon contact by a normal owner and denial to another account on native Linux/macOS; existing system evidence is linked in verification.md (RUN-005).
- [ ] 2.8 Execute the isolated Windows Sandbox gate for real UAC, same-SID user-to-elevated contact, different-account denial and cancellation; record results without changing host UAC/accounts/IIS (RUN-005, IPC-001).
- [ ] 2.9 Record G2 acceptance for the exact source revision, with concurrency, forced crash/restart, stale/misleading metadata, replay, spoofed endpoint and unsafe ACL cases; require all native/security evidence before group 3.

G2 is currently open. Prepared Sandbox scripts/packages do not satisfy 2.8.

## 3. Caddy adapter and transactional updates (G3, requires G2)

- [ ] 3.1 Port smol-toml configuration and Caddyfile adaptation/validation with fixture-proven precedence and bounded output/process cleanup; test invalid files and retained formats (REG-004, CADDY-001/005).
- [ ] 3.2 Port trusted real-Caddy resolution, executable pinning and npm-wrapper/SEA/file-identity recursion rejection; test aliases, PATH ordering and untrusted selectors (CADDY-001, SHIM-003).
- [ ] 3.3 Port multi-project composition, canonical domain ownership/filtering and exact-host/loopback guards; test disabled domains, conflicting projects and hostless sibling routes (CADDY-002/004, REG-002).
- [ ] 3.4 Implement the single prepare/validate/apply/verify/persist/publish queue and pending-state isolation; use concurrent fake-adapter mutations to prove serial ordering (CADDY-003).
- [ ] 3.5 Keep last-known-good on rejection and restore after post-apply persistence failure; test rollback failure fencing and active-state reconciliation after an ambiguous response before another change (CADDY-003).
- [ ] 3.6 Generate the internal CA/server/client material with X.509/WebCrypto and protected permissions, without system trust installation; test certificate validation and material ownership (CADDY-006).
- [ ] 3.7 Configure loopback mTLS admin and prevent project admin overrides/plaintext fallback; prove real Caddy denies no/wrong certificates and accepts only the authorized client (CADDY-006).
- [ ] 3.8 Port start/reload/inspect/graceful stop and bounded forced teardown using owned child handles only; prove unrelated processes survive success, error, Ctrl+C and shutdown (RUN-003, CADDY-002).
- [ ] 3.9 Pass multi-project registration/reload/rejection/rollback/ambiguity scenarios with fake adapters and real Caddy on supported native platforms; record G3 evidence.

## 4. Shim and current inspection adapters (G4, requires G3)

- [ ] 4.1 Freeze and port managed/read-only/passthrough/unsupported command policy; test unknown command refusal, argument arrays, stdin/stdout/stderr and exact exit status (SHIM-001).
- [ ] 4.2 Add managed missing-daemon startup with version-matched packaged entry resolution; prove no independent Caddy fallback and no PATH daemon substitution in npm or SEA (RUN-002, SHIM-002).
- [ ] 4.3 Port registration session nonce, heartbeat, reconnect, expiry and cleanup; test stale owner, simultaneous projects, Ctrl+C and daemon restart (REG-001/003).
- [ ] 4.4 Port operator port/Caddyfile inspection and PID-revalidated termination into small non-native-addon platform adapters; prove attach-only behavior and daemon ownership isolation (INSPECT-001..004).
- [ ] 4.10 Verify native shim/inspection parity and record G4 evidence, including lost-session cleanup and PID revalidation without expanding daemon ownership (SHIM-001..003, REG-001, INSPECT-001..004).

Former tasks 4.5-4.9 are withdrawn, not completed: IIS and autostart are outside
the corrected migration scope. Retain existing task IDs for evidence continuity.

## 5. State, diagnostic logs, shared client and routes CLI/TUI (G5, requires G4)

- [ ] 5.1 Implement the single node:sqlite application-state/diagnostic-log worker with schema creation, integrity validation, bounded jobs and orderly close; test corrupt/newer schemas, rollback and worker errors (STO-001/002/003).
- [ ] 5.2 Persist stable desired state and bounded diagnostic logs without restoring live sessions or introducing history queries; prove the old Rust runtime/database is neither imported nor deleted (STO-004, REG-003, OBS-001/002).
- [ ] 5.3 Port pre-write redaction and deterministic log retention, including supported text/JSON/URL credentials and private keys; test boundaries and terminal-safe presentation (OBS-001/002/004).
- [ ] 5.4 Preserve bounded canonical runtime/project/domain stream queries and correct domain attribution; test 1-200 limits, newest-row selection and ascending order, without source/severity/all-log filters or TUI log views (OBS-003).
- [ ] 5.5 Implement shared client-service methods/models over the eight-operation RPC catalog for current lifecycle, projects, domains, diagnostics and bounded CLI logs; test offline/malformed states without importing daemon storage or adding RPC methods (IPC-003, TOP-002).
- [ ] 5.6 Implement Commander released-command parity and human-readable output/exit contracts against the group 1 catalog; test arguments, snapshots and rejection of removed commands/options from fake client responses (CLI-001).
- [ ] 5.7 Preserve attach-only state/inspection commands and explicit daemon start/restart, managed shim startup, tui --start-daemon and offline-TUI start; test bounded launch behavior and bare help with/without TTY never spawning a daemon (CLI-001, RUN-002).
- [ ] 5.8 Implement the current routes-first Ink/React TUI through the same client service with real state, tree rows and no separate activity/logs/history/platform views; remove production mocks/random statuses and test Space/Enter toggles, offline Enter-to-start, pending actions and small terminals (TUI-001/002/003/004).
- [ ] 5.9 Preserve bare cadder help on TTY/non-TTY and explicit cadder tui with optional --start-daemon; prove terminal restoration on normal exit, error and Ctrl+C with rendering tests plus a real terminal smoke (CLI-001, TUI-004).
- [ ] 5.10 Pass an actual Node daemon/shim/operator journey with multiple projects, domain changes, bounded CLI logs and shutdown; record G5 parity against the catalog, not only fixture-runtime success.

## 6. Equal npm/SEA distributions and documentation (G6, requires G5)

- [ ] 6.1 Emit npm JS using tsc import-extension rewriting exclusively into external staging; generate the consumer manifest and three bin entries with matching version/license/repository/assets (DIST-004, QT-003).
- [ ] 6.2 Pack one npm tarball with the publication packer and verify its file allowlist, no optional platform packages/postinstall/download/build/loader/real Caddy; test local/global installation and PATH recursion outside checkout (QT-006, DIST-004).
- [ ] 6.3 Select, verify and pin the Node 26 SEA builder patch independently of the Node 24.18.0 npm minimum; record source/runtime compatibility and builder inputs (QT-001, DIST-001).
- [ ] 6.4 Bundle the same sources with esbuild and build three SEA entries plus assets outside checkout; prove state/log SQLite workers and owned child launch without a system Node executable (DIST-002).
- [ ] 6.5 Build Windows x64, Linux x64, macOS x64 and arm64 archives with checksums/provenance and exact accepted asset sets; reject version/source drift (DIST-001/002/005).
- [ ] 6.6 Run native SEA smoke for every variant with Node/npm absent from PATH, covering all entries, actual CLI/TUI, SQLite workers, Caddy adapter children and platform helper launch (DIST-002, QT-006).
- [ ] 6.7 Run the same current-scope functional catalog through packed npm and extracted SEA on native runners, including local/global npm wrappers and daemon start/restart; record cross-channel parity (DIST-005).
- [ ] 6.8 Move Astro/Starlight dependencies and tasks to Nub with exact locks; replace Bun/legacy instructions and document both channels equally, including separate Caddy prerequisite and safe transition (DOC-001/002/004).
- [ ] 6.9 Build/check docs from the supported workflow and verify each public command/example against real product acceptance, without fixture/private/hidden diagnostic seams (DOC-003/004).
- [ ] 6.10 Make CI validate the same non-publishing pack process used for publication, with native archive verification and least-privilege/provenance safeguards; record G6 only after both channels pass (QT-004/006/007).

## 7. Whole-product acceptance (G7, requires G6)

- [ ] 7.1 Enforce >=85% line coverage for own TS/TSX, including hard runtime/platform/queue modules; run contract/config/log/CLI/TUI tests with fake adapters and publish the exact source coverage evidence (QT-003).
- [ ] 7.2 Pass Windows/Linux/macOS native integration for independent installation runtimes, simultaneous starts, forced crash recovery, many projects, reload/rollback, heartbeat, Ctrl+C and bounded shutdown (TOP-001, RUN-001/003, REG-001, CADDY-003).
- [ ] 7.3 Pass wrong-secret, replay, spoofed endpoint, other-user, unsafe ACL and ordinary-owner-to-elevated-daemon cases, plus real Caddy mTLS denial without a client certificate (IPC-001/002, RUN-005, CADDY-006).
- [ ] 7.4 Close real Windows Sandbox owner/elevated access, UAC cancellation and other-account denial for the actual product, not only the runtime fixture; verify that no IIS/autostart operations were introduced (RUN-005, IPC-001, TOP-002).
- [ ] 7.5 Verify pack/build leaves tracked files unchanged and emits no generated Node JS/bundles/archives in checkout; review dependency exactness and prohibited runtime dependencies (QT-003/006).
- [ ] 7.6 Reconcile every requirement and approved-plan row in acceptance.md with actual evidence at one release-candidate revision; any missing channel/platform/capability keeps G7 open.

## 8. Replace and remove the Rust product (G8, requires G7)

- [ ] 8.1 Inventory Cargo/workspace/crates, old npm native packages, release scripts/workflows, Rust fixtures and documentation references; map each retained behavior to Node evidence or an approved removal, preserving original Git history (QT-005).
- [ ] 8.2 Inspect and classify the pre-existing npm directory before replacing its tracked legacy content; preserve unrelated untracked files and private operational state (QT-005).
- [ ] 8.3 Remove Rust source/workspace, Cargo manifests/lock/toolchain/tasks and obsolete packaging only after 8.1/8.2; retain necessary non-generated behavioral fixtures in the Node tests (TOP-001, QT-005).
- [ ] 8.4 Replace Cargo CI/release gates with Node/npm/SEA validation and small Cadder-specific packaging scripts; move shared release version-source configuration from retired native manifests to the Node product after verifying the release adapter (QT-003/004/007).
- [ ] 8.5 Update shared contributor/architecture guidance and remove stale Rust/Bun/xtask claims from the current product workflow while preserving historical archives (DOC-002).
- [ ] 8.6 Re-run complete tests, coverage, docs, packed npm and every SEA gate from the Node-only source revision and consumers without Rust/Cargo installed; ensure no package/build/runtime task depends on deleted code (QT-005).
- [ ] 8.7 Record G8: no permanent parallel Rust/Node product, no tracked generated JS, all approved functionality present and both equal distributions ready. Do not mark the migration complete at an earlier foundation checkpoint.

## 9. Release preparation and explicit publication boundary (G9, requires G8)

- [ ] 9.1 Verify the consumer transition checklist: stop the Rust daemon and re-register existing configurations; test leaving old runtime/database and rollback releases intact without adding IIS/autostart steps (DIST-003, STO-004).
- [ ] 9.2 Prepare matching 2.0.0-rc.1 npm/SEA artifacts and source-based release outcome/version manifests through the configured Arcantry adapter after its version sources are updated; no tag or publication (DIST-005).
- [ ] 9.3 Review artifact checksums, native evidence, docs and release story; reconcile the requirement ledger and separately request any missing commit/push/tag/npm/GitHub publication authority (QT-004/007).
- [ ] 9.4 Prepare 2.0.0 only after RC acceptance passes in both channels; retain old releases for rollback with no automatic deprecation, and publish only under separate explicit approval.
