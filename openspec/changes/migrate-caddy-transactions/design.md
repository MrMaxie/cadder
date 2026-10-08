## Evidence and ownership

CADDY-003 and the roadmap design require one prepare/adapt/compose/validate/apply/
verify/persist/publish queue. Retained Rust references are
crates/cadder-daemon/src/state/registrations.rs execute_registration_transaction
and apply_runtime_transition, plus runtime/process/receipts.rs rollback. They
separate candidate registration/coordinator state from final publication and
restore prior runtime configuration after persistence failure. Do not copy the
Rust mock backend or swallow failed rollback as acceptance.

Existing Node CaddyPort has validation and an apply hash receipt, but neither
independent active observation nor definite-rejection/ambiguity classification.
StoragePort persists stable DesiredState only. Composition yields a route plan,
not a runnable secure CaddyConfig. Do not reinterpret that plan as an apply-ready
configuration or implement insecure administration to close this orchestration.

## Internal contracts

A transaction snapshot contains a complete CaddyConfig or null (idle), stable
DesiredState and current in-memory registration DTOs. The daemon supplies an
already independently observed, durably committed initial snapshot; this module
is not startup or lease restoration. Persist only DesiredState, never registration leases,
nonces or process ownership. Future handlers construct candidate state inside
preparation using the latest committed snapshot and existing adaptation/composition.

Use the existing validation and persistence signatures without editing shared
ports. A small internal runtime interface supplies:

- Apply a complete config or the idle/null target; return explicit applied,
  definitely rejected or ambiguous outcome. Unexpected throws and outcomes without
  an explicit no-transition guarantee are ambiguous, including typed failures
  from the existing CaddyPort. Error kinds alone never prove rejection.
  Null is a desired runtime target, not a mock success; the later owned lifecycle
  backend must implement it.
- Independently read the observed active effective-config hash, or null only for
  positively observed idle. Unavailable or undeterminable state is an observation
  failure, never a successful null value.
  Observation comes from protected runtime state, not cached receipts, candidate
  bytes or desired intent. Backend normalization must make the observed hash
  comparable with the target; real readback/mTLS implementation remains later.

No product RPC, CaddyPort method or production backend is added. All dependencies
are required; missing runtime/storage adapters cannot become successful defaults.

## Queue and publication

Preparation callbacks run only after preceding queue work settles and receive
an isolated committed snapshot. Copy/validate/freeze candidate data before any
side effect; protect it from caller and adapter mutation. Validate complete
bounded/hash-consistent JSON using existing preparation checks and verify a
validator receipt matches that target. Skip config validation only for idle/null.

Serialize apply -> authoritative observation -> atomic storage commit -> one
synchronous internal snapshot publication. There is no fallible external publish
callback after persistence; getters/results expose isolated snapshots. Read-only
queries see last committed data, plus a separate fence diagnostic when necessary,
not a pending state masquerading as authoritative.

Keep the queue tail settled after errors, and reject queued mutations at execution
if an earlier operation fenced the runtime. An explicit close/drain may refuse new
admissions and await owned work; it does not release runtime exclusion or claim
product shutdown. Do not add a generic queue framework, event bus, background
worker, timer-based lease, receipt store or new configurable workflow machinery.
Existing phase adapters own deadlines and atomic settlement. Do not race a timer
and start later mutations while an older adapter can still perform side effects.

## Failure state decisions

- Prepare, shape/integrity or validation rejection: no apply, persistence or
  publication; retain the prior committed snapshot and continue the queue.
- Definite apply rejection: backend guarantees no runtime transition; preserve
  last-known-good without publishing or fencing solely for that rejection.
- Ambiguous apply/throw or failed/mismatched observation: keep committed state,
  report a bounded typed diagnostic, fence mutations and require reconciliation.
- Persistence failure after verified apply: restore the prior complete config or
  idle target and independently verify it. Keep the old committed snapshot and
  return the storage failure. Failed/uncertain rollback remains fenced.
- Reconciliation is explicitly enqueued through the module's reconcile method by
  the daemon caller, not automatically appended by a failure. FIFO mutations
  before reconciliation are refused while fenced; a later queued mutation may
  execute only after successful reconciliation clears the fence. This adds no RPC.
  Observe current runtime; if it is not last-known-good, restore that target, then
  observe again. Only independent proof of last-known-good clears the
  fence. Never publish the abandoned candidate merely because it was observed.
  Observation/restoration failure preserves the fence and last committed state.

The storage adapter must satisfy STO-002: failed/rejected persistence is atomic
and leaves prior durable intent committed. This slice tests orchestration under
that contract, not the future SQLite worker's integrity/commit implementation.
Typed and thrown errors are bounded/redacted; arbitrary exception text must not
leak through results. A success receipt alone never proves apply or rollback.

## Verification and migration

Dedicated tests use fake adapters only in test code, with explicit deferred
promises rather than sleeps to prove single flight, fresh prepare inputs and
pending-read isolation. Cover every rejection/throw/mismatch, persistence failure,
first-apply idle rollback, rollback rejection/ambiguity/observation failure,
queued fencing/reconciliation, retry after recovery, mutation isolation and queue
drain. Include a prepare path consuming existing pure composition so callback
serialization is not merely a bare promise scheduler test.

MiMo Flash critiques only this bounded ownership/failure plan; MiMo Pro reviews
completed source/tests independently. Parent integrates concrete findings,
performs simplification/fresh-eyes review and runs focused/full Node checks,
strict main/roadmap/schema and diff inspection. No real Caddy, SQLite worker,
registration handler, native privilege or public-product proof is inferred.

Additive daemon source leaves the Rust baseline and previous Node leaf modules
unchanged. Actual secure backend/handler integration remains 3.6-3.9 and groups
4-5. Known OpenSpec implementation-schema archive failures remain explicit;
no fake delta requirements or custom validator repair is included.
