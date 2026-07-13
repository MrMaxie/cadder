## MODIFIED Requirements

### Requirement: RUN-005: Shutdown drains in-flight work
The daemon SHALL stop accepting new mutating requests, cancel or finish owned background work, flush durable state, and remove discovery metadata only after it attempts terminal delivery to active connections and completes one normal drain whose absolute budget does not exceed 30 seconds. Interruptible work MUST be cancelled and joined within its assigned phase. If work has already entered a non-interruptible durability primitive or asynchronous rollback whose interruption can expose partial state, the daemon SHALL enter fail-stop containment, retain runtime and publication ownership until that work finishes, and then complete generation-matched cleanup instead of detaching the work or reporting a completed shutdown.

#### Scenario: Stop with active request
- **WHEN** daemon shutdown begins while a bounded request is in progress
- **THEN** the request either completes within its operation deadline or receives a typed cancellation error
- **AND** no detached task continues mutating state after shutdown completes

#### Scenario: Log tail during shutdown
- **WHEN** a client tails logs while the daemon stops
- **THEN** the stream emits a terminal shutdown outcome and closes

#### Scenario: Non-interruptible durability exceeds its phase
- **WHEN** an owned storage flush has entered a non-interruptible durability primitive and exceeds the normal storage phase deadline
- **THEN** Cadder retains the daemon, runtime, discovery metadata, endpoint, and generation lock until the flush joins
- **AND** Cadder continues rejecting new requests and mutations throughout fail-stop containment
- **AND** cleanup removes only resources that still match the contained daemon generation

#### Scenario: Interruptible work exceeds its phase
- **WHEN** owned work that exposes a cancellation or termination seam reaches its shutdown phase deadline
- **THEN** Cadder cancels or terminates and joins that work within the normal shutdown timeline
- **AND** it does not use fail-stop containment to extend ordinary slow work
