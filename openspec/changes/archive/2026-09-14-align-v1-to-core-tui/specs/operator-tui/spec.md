## MODIFIED Requirements

### Requirement: TUI-001: Explicit TUI surface with shared operator contracts
The full-screen operator SHALL start only through `cadder tui` and SHALL use the shared typed local client for daemon attachment, launch, requests, and errors. It SHALL expose exactly the Status, Domains, and Logs primary views and SHALL NOT read daemon storage directly, infer state from processes, or contain embedded production data.

#### Scenario: TUI attaches to the installation runtime
- **WHEN** an operator runs `cadder tui` while the daemon is ready
- **THEN** the TUI displays state returned by the authenticated daemon client

#### Scenario: No daemon is available at startup
- **WHEN** `cadder tui` starts while the daemon is not running
- **THEN** it displays an offline state and an explicit Start action without creating mock rows

### Requirement: TUI-002: Authoritative initial and refreshed data
The TUI SHALL obtain one bounded runtime snapshot containing status, entrypoints, domains, configuration health, and storage health. One runtime SHALL contain at most 128 entrypoints and 1,024 domains, and its serialized snapshot MUST remain within the 1,048,576-byte response-frame limit. Registration or activation that would exceed any limit MUST fail before Caddy apply or durable commit. The TUI SHALL request at most 200 recent log entries for the selected stream. It SHALL refresh these bounded responses after a completed mutation, an explicit refresh, reconnect, or its documented polling interval; it SHALL NOT depend on subscriptions, paging, snapshot tokens, or inferred local state.

#### Scenario: Initial snapshot is displayed
- **WHEN** the TUI attaches to a compatible daemon
- **THEN** Status and Domains match one bounded daemon snapshot

#### Scenario: Refresh fails
- **WHEN** a bounded refresh request fails
- **THEN** the TUI marks retained data as stale and preserves the last authoritative values without presenting them as current

#### Scenario: Candidate would exceed snapshot bounds
- **WHEN** a registration or activation would exceed the entrypoint, domain, or encoded snapshot limit
- **THEN** the daemon rejects it before applying Caddy or changing durable state
- **AND** the last authoritative snapshot remains representable by one response frame

### Requirement: TUI-003: Explicit offline, incompatible, and degraded states
The TUI SHALL distinguish connecting, connected, daemon-not-running, connection-failed, permission-denied, exact-version-incompatible, stopping, restarting, and degraded-runtime states using text in addition to styling. It SHALL disable unavailable mutations and present Start, Stop, Restart, Retry, or diagnostic detail only when that action applies.

#### Scenario: Connected daemon exits
- **WHEN** the daemon connection closes unexpectedly
- **THEN** the TUI remains responsive, marks retained data stale, disables daemon-owned mutations, and presents Start or Retry

#### Scenario: Protocol is incompatible
- **WHEN** the executable and daemon protocol versions differ
- **THEN** the TUI tells the operator to use all binaries from one Cadder archive
- **AND** it does not send state requests or repeatedly retry the incompatible daemon

#### Scenario: Runtime becomes degraded
- **WHEN** the daemon reports degraded storage, configuration, or Caddy state
- **THEN** the TUI preserves read-only Status and Logs and blocks only the mutations reported unsafe

### Requirement: TUI-004: Complete keyboard navigation and visible focus
Every interactive TUI element SHALL be reachable and operable without a mouse. Left/Right and Tab/Shift+Tab SHALL move between primary views, Up/Down SHALL move the current selection, PageUp/PageDown SHALL scroll visible content, Enter SHALL invoke or confirm the focused action, Space SHALL request enable or disable for a toggleable selection, `r` SHALL refresh or retry, Esc SHALL close the topmost detail or confirmation before exiting from the root view, and Ctrl+C SHALL request a clean TUI exit. Current focus and available shortcuts SHALL remain visibly identified.

#### Scenario: Operator uses only the keyboard
- **WHEN** an operator uses only the documented keys
- **THEN** the operator can move between all views, inspect status and logs, select a domain, invoke an available lifecycle or activation action, cancel a confirmation, and exit

### Requirement: TUI-005: Authoritative mutations
Every TUI mutation SHALL invoke one typed daemon operation, display a pending state, prevent duplicate submission, and reconcile from the authoritative response or a fresh snapshot instead of committing an optimistic local change. Supported mutations SHALL be entrypoint activation, domain activation, daemon start, daemon stop, and daemon restart.

#### Scenario: Domain toggle succeeds
- **WHEN** an operator toggles a selected domain and the daemon accepts the request
- **THEN** the row remains pending until authoritative state confirms the new activation state

#### Scenario: Mutation is rejected
- **WHEN** the daemon rejects an activation or lifecycle action
- **THEN** the prior authoritative state remains displayed and the TUI shows the user-relevant outcome and recovery action

#### Scenario: Daemon restart succeeds
- **WHEN** an operator confirms Restart
- **THEN** the TUI waits for the owned daemon to stop before launching it again
- **AND** it reports connected only after the replacement daemon answers the exact-version readiness handshake

### Requirement: TUI-006: Bounded log inspection
The Logs view SHALL render the most recent redacted events returned by one bounded query for the selected runtime, entrypoint, or domain stream, up to 200 entries. It SHALL support selection and scrolling and SHALL distinguish empty, stale, read-error, and truncated results. It SHALL NOT expose tail follow, time-range queries, cursor continuation, export, history, or configurable filters in Cadder 1.0.

#### Scenario: Log view starts empty
- **WHEN** the selected stream has no retained entries
- **THEN** the view identifies the stream and displays an explicit empty state

#### Scenario: Query reaches the TUI limit
- **WHEN** more than 200 retained entries exist for the selected stream
- **THEN** the TUI displays the newest 200 in stable order and identifies the view as recent logs rather than complete history

### Requirement: TUI-007: Responsive and bounded terminal layout
At `80x24` and larger, the TUI SHALL present navigation, status, content, and shortcut regions without overlap. At smaller sizes it SHALL preserve the active view and selection where possible, truncate text visibly, and keep status and exit guidance reachable. If the terminal cannot fit the minimum layout, it SHALL render a resize notice and SHALL NOT panic or invoke hidden actions.

#### Scenario: Terminal becomes too small
- **WHEN** the running TUI cannot fit its minimum interactive layout
- **THEN** it renders a bounded resize message and continues processing resize and exit keys

### Requirement: TUI-008: Accessible status and color behavior
The TUI SHALL communicate focus, activation, health, pending work, lifecycle state, errors, and log severity with text or symbols in addition to color. It MUST NOT require animation or color perception to distinguish an available action or runtime state.

#### Scenario: Terminal colors are unavailable
- **WHEN** the terminal renders without the expected color palette
- **THEN** labels, symbols, focus, and layout still distinguish status, selection, severity, and errors

### Requirement: TUI-009: Safe terminal lifecycle and bounded background work
The TUI SHALL restore input mode, cursor, and alternate-screen state after normal exit, Ctrl+C, initialization failure, rendering failure, or a captured panic. Background refresh and log work SHALL be cancellable and bounded. Closing the TUI normally SHALL leave the daemon and Caddy running; only a confirmed Stop or Restart action SHALL request daemon shutdown.

#### Scenario: Operator exits during an in-flight request
- **WHEN** the operator presses Ctrl+C while a refresh or log request is pending
- **THEN** the TUI cancels its background work, restores the terminal, and leaves daemon-owned processes running

#### Scenario: Operator confirms Stop
- **WHEN** the operator confirms the visible Stop action
- **THEN** the TUI waits for the daemon's bounded shutdown acknowledgement
- **AND** the expected connection closure is presented as stopped rather than as an unexpected failure
