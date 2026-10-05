## 1. Align Shutdown Contract

- [x] 1.1 Update the active secure-control-plane task wording so interruptible work remains bounded and non-interruptible durability or rollback work uses fail-stop containment.
- [x] 1.2 Verify focused shutdown tests cover the absolute normal timeline, bounded ACK delivery, interruptible task joins, ownership retention, and generation-matched cleanup.

## 2. Validate Contract Change

- [x] 2.1 Validate the contract change, main specs, implementation schema, and repository-specific evidence with the schema-aware strict checks documented in `openspec/README.md`.
- [x] 2.2 Run the full project gate and confirm the contract change introduces no code, protocol, storage-format, or user-documentation regression.
