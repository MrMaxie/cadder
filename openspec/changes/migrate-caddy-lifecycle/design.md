## Existing boundaries

ConfigurationTransactions owns mutation ordering and publication. Its required
ConfigurationRuntime.apply accepts CaddyConfig or null; readActive returns an
independently observed hash, positively proved idle, or a failure.
CaddyAdminClient supplies bounded verified HTTPS /load and /config/ but not
process ownership. PinnedCaddyImage verifies the executable around native
creation. runOwnedCommand owns a Windows Job or Unix process group and waits for
owned cleanup. Reuse these boundaries.

## Lifecycle increment

One lifecycle object holds one owned execution and one prepared secure admin
identity/environment snapshot. It starts Caddy with a complete normalized secure
JSON candidate, never --resume, project environment or plaintext fallback.
Runtime exclusion is held by the caller throughout this object's lifetime.
Guard malformed/security-changing candidates before side effects. Freeze/copy
caller inputs. Initial and reload observation use the existing HTTPS readback.

Only add the minimal long-lived mode to existing owned execution needed to avoid
a configuration-command deadline or unbounded lifetime output accumulation.
Keep adapt/validate command defaults and output limits unchanged. Preserve pin
checks and existing group/Job finalizers. No second native launcher, generic
process registry, monitoring framework or process enumeration is introduced.
Windows uses private child pipes so its verified-resume handshake cannot race
native output. Wrapper success follows Job and stream settlement; a bounded
four-byte footer carries the native exit code separately from helper failures.
A fixed-buffer stdout payload counter preserves immediate short-command limits;
long-lived execution drains without accumulating lifetime output.

Before launching, start performs a bounded read-only conflict check and refuses
an already-responsive authenticated endpoint. Persisted TLS identity proves the
peer identity, not ownership of a newly resumed process. This check grants no
permission to adopt or stop the existing process. Active readback begins only
after verified native creation/resume and continues to watch owned settlement.
Failure cancels and settles that execution before another start or staging
cleanup. A process that exits unexpectedly is not automatically adopted or
restarted. A later explicit apply/reconciliation may start the pinned image again.
Unknown or incomplete teardown remains failure/ambiguity, never proof of idle.

The existing transaction queue's explicit reconciliation may restore the last
committed target after an unavailable initial observation, not just a mismatch.
The runtime still refuses unsafe or unproved teardown; restoration must succeed
and independent final readback must match before the queue clears its fence.
This narrow integration correction neither publishes an abandoned candidate nor
persists new intent or introduces automatic recovery.

Reload delegates to the existing HTTPS client. Transaction verification,
rollback and persistence remain in ConfigurationTransactions. Null performs a
real graceful /stop, waits for owned settlement, then forces through the existing
owned control boundary if needed. close stops admissions, cancels startup or
in-flight admin work, and drains teardown; it does not release the daemon lock
or close the caller-owned resolver/storage. No global Ctrl+C handlers belong to
the leaf adapter; callers invoke close from their shutdown/signal path.

## Security and staging

Use the secure policy's isolated environment for validation and execution.
Stage complete JSON only in protected runtime-owned storage and retain it until
owned execution settles. Strict product-created path checks remain unchanged.
Known generated Caddy storage paths use the accepted read-only descendant check;
no ACL rewrite, general tree scan or secret-bearing diagnostic is added.

## Verification

Use fake Caddy/process/admin seams in unit tests and disposable Node-native
children for lifecycle ownership evidence; tests require neither real Caddy nor
private paths. Prove idle/start/reload, unexpected exit, startup timeout/failure,
closed admission, cancellation, forced stop and unrelated process survival.
Integrate with the real transaction queue for idle rollback and reconciliation.
Native real-Caddy evidence is optional for this first adapter increment and must
be distinguished from fake fixtures and the previously completed admin-only
Windows proof. Whole roadmap 3.8/G3 acceptance remains open until its full native
scenarios pass. Known implementation-schema archive/validator issues are not
fixed by copying requirements or changing tooling in this slice.
