## MODIFIED Requirements

### Requirement: IPC-009: Shutdown closes the control plane deterministically
During the shutdown behavior defined by `RUN-005`, the control plane SHALL use one absolute 30-second normal-operation timeline to stop accepting new connections, reject new requests on existing connections, attempt a terminal outcome on active streams, and drain or cancel owned handlers. Discovery metadata and the endpoint MUST remain until terminal outcomes are attempted and MUST be removed only by the owning daemon generation. A stalled or disconnected client MUST NOT extend the normal timeline. If `RUN-005` enters fail-stop containment for an already-started non-interruptible durability primitive or asynchronous rollback, the endpoint and discovery metadata SHALL remain owner-restricted beyond the normal timeline until the contained work joins, and the control plane SHALL continue rejecting all new requests.

#### Scenario: New request during shutdown
- **WHEN** an authenticated client sends a request after shutdown begins
- **THEN** the daemon returns a non-retryable shutting-down error without invoking the handler

#### Scenario: Active subscription during shutdown
- **WHEN** shutdown begins while a responsive client holds an active state or log subscription
- **THEN** the stream receives a terminal shutdown outcome and closes

#### Scenario: Subscription does not accept the terminal outcome
- **WHEN** a stream is stalled or disconnects during terminal delivery
- **THEN** the daemon ends the bounded delivery attempt and continues shutdown
- **AND** endpoint cleanup does not wait beyond the 30-second normal timeline unless `RUN-005` has entered fail-stop containment

#### Scenario: Handler does not drain
- **WHEN** an in-flight handler exceeds its assigned shutdown phase
- **THEN** the daemon cancels and joins its owned task before process exit
- **AND** no detached task can mutate the runtime after endpoint cleanup

#### Scenario: Contained work outlives the normal timeline
- **WHEN** `RUN-005` enters fail-stop containment after terminal outcomes have been attempted
- **THEN** discovery metadata and the endpoint remain restricted to the owner and continue reporting shutdown behavior
- **AND** cleanup runs only after the contained work joins and only for the matching daemon generation
