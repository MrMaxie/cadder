## Context and requirements

The accepted reset roadmap supplies protocol 3 and ConfigurationTransactions.
CaddyRuntime already implements verified apply, idle and owned teardown. This
slice supplies the missing product orchestration against those contracts.

## Ownership and data flow

The daemon holds the existing installation lock before opening one SQLite worker.
Worker startup validates owner protection, schema and integrity. A bounded job
mailbox serializes intent/log work; graceful close drains jobs and closes SQLite
before exclusion is released. Existing Rust paths are never inspected.

Registration uses canonical source paths for stable project identity. The daemon
adapts the source itself and derives domains rather than trusting supplied route
metadata. The live session nonce owns heartbeat/unregister; durable intent holds
only source paths and activation. All registration and activation changes prepare
inside ConfigurationTransactions. Reads expose committed snapshots. Expiration
removes live routes without forgetting user activation choices.

Existing secure policy assembly handles each route plan using one runtime identity
and admin endpoint. Idle readiness opens protected application storage first;
the first active registration initializes Caddy/PKI lazily. Subsequent mutations
reuse that protected identity. CaddyRuntime and ConfigurationTransactions remain the only
apply/lifecycle queue. Shutdown stops admission, drains transactions, closes owned
Caddy/storage resources, then releases local IPC/exclusion.

## Decisions and alternatives

Use node:sqlite and worker_threads, not a new persistence library or job framework.
Store validated stable intent as one JSON row and bounded structured diagnostic
rows. Redact before insertion and sanitize terminal output at presentation.
Keep module/import/error conventions and installed Zod/Vitest tools.

## Failure and recovery

Reject unsupported/partial/corrupt schemas without repair. SQLite writes rollback
on failure; pending worker requests fail if the worker exits. The existing queue
restores last-known-good after persistence failure and fences uncertain recovery.
Do not restore session nonces or process ownership from storage. Worker teardown
failure must not release exclusion while owned work remains unsettled.

Explicit launch resolves the daemon from the physical npm package or SEA sibling,
never PATH. Only its post-launch readiness loop waits through incomplete Windows
ACL publication; every RPC still refuses unsafe paths before reading the secret
or authenticating. Pre-existing unsafe paths fail immediately. Failed readiness
terminates only the newly spawned child. Mutation RPCs allow bounded cold Caddy
preparation (120 seconds); reads remain bounded at 15 seconds.

## Operator and packaging

Commander and routes-only Ink use one client service. The managed shim owns its
session nonce, heartbeat and unregister; delegated upstream commands retain raw
streams. Socket inspection and explicit expected-PID termination stay in the
operator, outside daemon-owned process teardown.

Use tsc for the single compiled npm tarball and esbuild with Node v26.12.0 for
three standalone SEA entries. Embed the same SQLite worker as a SEA asset.
Stage only in system temp, include source identity and checksums, and retain
Node/bundled dependency notices. Reuse installed packages and platform tools.
Documentation moves to exact Nub locks; the existing Astro output directory and
publication workflow stay intact while the Rust release remains the baseline.

## Verification

Prefer database and handler behavior tests covering persistence, retention,
redaction, stale ownership, conflicts, toggles and restart over declaration or
source-string assertions. Keep security/fault regressions. Run focused checks
after increments, then the complete Node gate. Native product/distribution checks
remain separate evidence; local fixtures cannot accept unexecuted platforms.

## Open questions

None for these accepted contracts. Cross-platform acceptance and distribution
cutover remain governed by the parent roadmap.
