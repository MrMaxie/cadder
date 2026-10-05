## Context

Cadder owns the daemon process, its Caddy child, local IPC publication, runtime lock, and durable file store as one shutdown boundary. Most work is cancellable and fits one 30-second timeline. A filesystem durability primitive or an asynchronous rollback can cross its phase deadline after it has reached a point where aborting the task would detach state mutation or expose partial ownership.

The existing implementation keeps the process alive in that exceptional state, while the canonical requirements currently describe every drain as unconditionally bounded. The contract needs to name the exception and preserve the stronger safety invariant: Cadder never exits while owned work can still mutate runtime state.

## Goals / Non-Goals

**Goals:**

- Preserve one absolute 30-second normal-operation shutdown timeline.
- Cancel and join all interruptible request, stream, runtime, storage, and cleanup work within its phase boundary.
- Retain runtime ownership when already-started durability or rollback work cannot be interrupted safely.
- Make containment observable without exposing local paths, secrets, or peer identity.
- Keep cleanup generation-matched after contained work finishes.

**Non-Goals:**

- A general extension mechanism for shutdown deadlines.
- Detaching or force-aborting filesystem durability calls or rollback tasks.
- Changing the IPC wire format, file format, or operator commands.
- Treating ordinary slow clients, handlers, or child processes as containment cases.

## Decisions

### One absolute normal timeline

Shutdown records one monotonic start instant before an IPC shutdown acknowledgement. The accept, handler, runtime, storage, and cleanup boundaries derive from that instant. Each later phase uses the earlier of its own duration cap and the absolute cumulative boundary, so a slow earlier phase never resets the 30-second clock.

This is preferred over independent relative timers because relative timers allow scheduling overhead and a slow acknowledgement to extend normal shutdown beyond the advertised limit.

### Fail-stop is a narrow ownership state

Only work already inside a non-interruptible durability primitive or an asynchronous rollback can enter fail-stop containment. Cadder keeps the daemon, runtime process tree, discovery metadata, endpoint, and generation lock owned until that work joins. It continues rejecting new requests and mutations throughout containment.

Force-exiting at the deadline is rejected because the abandoned task can finish against files or runtime state after ownership appears released. Detaching cleanup is rejected for the same reason.

### Cleanup remains generation-matched

After contained work finishes, discovery and lock cleanup compare the owning generation before removal. A replacement generation is never removed by an older guard.

### Verification uses explicit seams

Focused tests stall acknowledgement delivery, runtime stop, storage flush, rollback, discovery cleanup, and replacement-generation publication. The tests prove the normal deadline does not reset, interruptible work joins, containment retains ownership, and cleanup never removes a newer generation.

## Risks / Trade-offs

- [Risk] A failed kernel or filesystem call can keep Cadder alive beyond 30 seconds. → Cadder reports fail-stop containment through owner-only diagnostics, rejects new work, and retains ownership rather than presenting a false stopped state.
- [Risk] Broad containment criteria can hide ordinary shutdown defects. → Only post-commit durability and rollback seams qualify; slow clients, handlers, and child processes remain cancellable within the normal timeline.
- [Risk] Requirement wording can be read as a deadline waiver. → Specs state that 30 seconds remains absolute for normal work and that containment is an ownership invariant, not successful shutdown completion.
