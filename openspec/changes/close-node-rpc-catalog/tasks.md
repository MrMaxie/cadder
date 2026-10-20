## 1. Implementation

- [x] 1.1 `[IPC-003]` Define the closed eight-operation Zod DTO/error catalog and method-indexed types (verification: all eight round trips and strict unknown-field/schema negative tests pass; migration verification.md, Closed RPC verification).
- [x] 1.2 `[IPC-001, IPC-002, IPC-003]` Validate authenticated requests before dispatch and correlate one typed result/error; type and validate client calls (verification: authenticated malformed/unknown-method, malformed-response and oversized-response tests pass without invalid handler invocation; independent MiMo reviews pass).
- [x] 1.3 `[IPC-003]` Migrate runtime tests and compiled fixtures to query-state/shutdown catalog requests without product mock fields (verification: runtime-system tests and compiled-JS Windows smoke pass).
- [x] 1.4 `[TOP-001, TOP-002, IPC-003]` Define exactly four mockable leaf ports (Caddy, platform, storage and client service) with typed method/result/error contracts, stable desired-state storage shape and protocol-only imports (verification: focused contract fakes and static import-direction checks pass).

## 2. Documentation and migration

- [x] 2.1 `[IPC-003]` Reconcile catalog fixtures and record only executed evidence in the migration roadmap (verification: compatibility tests pass and missing native/Sandbox gates remain open; Closed RPC verification).
- [x] 2.2 `[IPC-002]` Verify no Rust IPC negotiation, old-data migration, dependency or release change was introduced (verification: parent and independent MiMo scoped diff/import reviews).

## 3. Final verification

- [x] 3.1 `[IPC-001, IPC-002, IPC-003]` Run nub run check, focused compiled smoke and strict main-spec checks; independently review the integrated slice (verification: parent checks and final independent MiMo Pro integrated review pass; verification.md records the existing OpenSpec 1.5 spec-free implementation validation limitation and teardown transport cancellation without adding duplicate requirement deltas).
