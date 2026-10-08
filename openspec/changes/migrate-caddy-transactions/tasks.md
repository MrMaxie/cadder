## 1. Serialized commits

- [x] 1.1 `[CADDY-003]` Define and implement the narrow internal runtime observation/apply-outcome boundary and one transaction queue over existing carriers/validator/storage signatures (verification: explicit phase order, fresh committed prepare inputs and controlled concurrent promises).
- [x] 1.2 `[CADDY-003]` Isolate pending/caller/adapter data and publish only after independent verification and atomic persistence (verification: pending reads, input/result mutation, malformed/hash-mismatched candidate and receipt rejection; a failed operation does not poison the tail).

## 2. Last-known-good recovery

- [x] 2.1 `[CADDY-003]` Restore and verify the previous runtime target after persistence failure, including an initially idle runtime (verification: no rejected candidate publication and rollback apply/readback failure matrices).
- [x] 2.2 `[CADDY-003]` Fence ambiguous outcomes and failed recovery; serialize explicit active-state reconciliation before another mutation (verification: queued mutation refusal, observation/restoration errors, last-known-good restoration, clearing only after verification and post-recovery success).

## 3. Evidence and closeout

- [x] 3.1 `[CADDY-003]` Reconcile MiMo opinions/review, focused/full Node checks and requirement-to-evidence coverage (verification: >=85% own-code lines, strict main/roadmap/schema, diff review and explicit backend/native/archive limits).

Checked tasks deliver roadmap 3.4-3.5 orchestration only. Caddy secure administration,
owned server lifecycle, SQLite application storage, product handlers and whole G3
acceptance remain open. No production fake backend or ready-state stub is permitted.
