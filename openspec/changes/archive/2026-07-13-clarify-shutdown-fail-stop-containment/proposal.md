## Why

The shutdown requirements promise a bounded normal drain, while the accepted design retains ownership when a durability call or rollback cannot be interrupted safely. The canonical contract needs to distinguish these cases so Cadder never trades runtime integrity for a misleading process-exit deadline.

## What Changes

- Define 30 seconds as the complete normal-operation shutdown budget shared by terminal delivery, handler drain, runtime stop, storage flush, and owned cleanup.
- Require interruptible work to stop within its phase deadline without leaving detached tasks.
- Define a narrow fail-stop containment exception for work that has already entered a non-interruptible durability call or rollback.
- Require Cadder to retain the runtime, discovery metadata, endpoint, and generation lock until contained work finishes.
- Keep new requests and mutations rejected throughout containment and expose the degraded shutdown state through owner-only diagnostics.
- Exclude general deadline waivers, detached cleanup, and interruption of an in-progress durability primitive.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `daemon-lifecycle`: Clarify when bounded drain completes normally and when ownership remains in fail-stop containment.
- `local-control-plane`: Clarify the absolute 30-second normal timeline and the endpoint-retention exception for contained work.

## Impact

The change updates the canonical shutdown contract, the implementation task wording, and focused verification. It does not change the IPC wire format, storage file format, public CLI, dependency set, or supported platforms.
