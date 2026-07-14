## 1. Protocol And Plans

- [ ] 1.1 Add bounded IIS status, preview-plan, apply, restore, operation, and result DTOs with closed schemas and wire fixtures.
- [ ] 1.2 Add the four production IIS operations, capability metadata, dispatch deadlines, and compatibility tests.
- [ ] 1.3 Implement the owner-only plan store with deterministic semantic hashes, five-minute expiry, single-use consumption, 32-plan capacity, and bounded cleanup tests.

## 2. Elevated Helper Boundary

- [ ] 2.1 Add the typed one-shot helper protocol for allowlisted IIS binding operations, commit, rollback, and terminal results.
- [ ] 2.2 Implement the owner-restricted Windows helper channel with daemon/helper process identity, instance, nonce, hash, expiry, and one-connection validation.
- [ ] 2.3 Add hidden elevated `cadderd` helper mode that never initializes or inherits the normal control plane and refuses non-elevated execution.
- [ ] 2.4 Resolve Windows tooling from absolute system paths, disable user profiles and module search, and remove the elevated temporary-script prototype.

## 3. Transactional IIS State

- [ ] 3.1 Implement preview-only handoff, restore, and orphaned-loopback recovery classification without mutating IIS or Caddy.
- [ ] 3.2 Revalidate exact pre-state and backend-port availability in the daemon and helper before applying a consumed plan.
- [ ] 3.3 Keep one helper alive across IIS mutation and Caddy route commit, with reverse rollback on partial failure, timeout, cancellation, or daemon loss.
- [ ] 3.4 Publish active restore metadata only after verified commit and preserve unrelated sites, bindings, certificates, and external changes.
- [ ] 3.5 Reject normal elevated daemon startup while allowing only the hidden one-shot helper mode.

## 4. Operator Surfaces

- [ ] 4.1 Add shared operator IIS status, preview, apply, and restore methods, views, typed guidance, and stable exit-code mapping.
- [ ] 4.2 Implement the specified `cadder iis status`, preview, apply, and restore command hierarchy with human and JSON output tests.
- [ ] 4.3 Add the authoritative IIS TUI tab, immutable plan overlay, explicit confirmation, pending state, and authoritative refresh tests.
- [ ] 4.4 Update Windows IIS documentation to the production command flow and recovery behavior.

## 5. Verification And Migration

- [ ] 5.1 Add fake-provider and fake-helper tests for expiry, replay, drift, denial, rollback, Caddy failure, restore, and unrelated-binding preservation.
- [ ] 5.2 Add Windows helper transport tests for identity mismatch, second connection, timeout, cancellation, and inherited-control-plane denial.
- [ ] 5.3 Add an ignored real-IIS smoke that performs handoff, serves the IIS route through Caddy, restores the binding, and leaves unrelated IIS state unchanged.
- [ ] 5.4 Verify one daemon and one real Caddy concurrently serve the local IIS Workflow Performance URL and Smarketing domains registered by the PATH shim.
- [ ] 5.5 Run workspace formatting, linting, tests, release validation, documentation checks, and fresh-eyes review before marking the change complete.
