# operator-tui Specification

## Purpose
Define a real-state terminal interface that shares operator contracts with the CLI and remains usable across errors, permissions, and small terminals.

## Requirements
### Requirement: TUI-001: Explicit TUI surface with shared operator contracts
The full-screen operator SHALL start only through `cadder tui` and SHALL use the same runtime selection, IPC client, typed errors, domain resolution, and view-model contracts as the CLI. It SHALL NOT read daemon storage directly, infer runtime state independently, or contain embedded production data.

#### Scenario: TUI attaches to the selected runtime
- **WHEN** an operator runs `cadder tui` with a profile or runtime-directory override
- **THEN** the TUI attaches to the same runtime that an equivalent CLI invocation selects and presents state derived from shared operator view models

#### Scenario: No daemon is available at startup
- **WHEN** `cadder tui` starts while the selected daemon is not running
- **THEN** it enters an explicit offline state without creating mock rows or starting the daemon implicitly

### Requirement: TUI-002: Authoritative initial and live data
The TUI SHALL obtain its initial runtime, configuration, storage, entrypoint, domain, and diagnostic state from one authoritative daemon snapshot manifest and every bounded page for its snapshot token. It SHALL replace displayed authoritative state only after the complete generation is assembled. While connected, it SHALL apply ordered state notifications and targeted log queries to keep the displayed model current, and it SHALL refresh the full snapshot when notification continuity cannot be proven.

#### Scenario: Initial snapshot is displayed
- **WHEN** the TUI attaches to a compatible running daemon
- **THEN** the displayed status, counts, entrypoints, domains, configuration, and storage information match the daemon snapshot rather than local fixtures or generated values

#### Scenario: State notification has a sequence gap
- **WHEN** the TUI receives a notification whose sequence does not continue from the last applied event
- **THEN** it marks the affected view as refreshing, requests a complete snapshot, and does not present the incomplete sequence as authoritative

#### Scenario: Initial snapshot expires during paging
- **WHEN** the daemon reports that the snapshot token expired before every page arrived
- **THEN** the TUI discards the incomplete generation and requests a new manifest
- **AND** it never combines pages from different runtime generations

### Requirement: TUI-003: Explicit offline, incompatible, and degraded states
The TUI SHALL distinguish connecting, connected, daemon-not-running, connection-failed, permission-denied, protocol-incompatible, and degraded-runtime states using text in addition to styling. After losing a connection, it SHALL preserve the last snapshot as visibly stale, disable unavailable mutations, provide recovery guidance, and retry attachment with bounded backoff until the operator quits or requests an immediate retry. It SHALL NOT start or replace the daemon without an explicit operator action.

#### Scenario: Connected daemon exits
- **WHEN** the daemon connection closes while the TUI is running
- **THEN** the TUI remains responsive, marks retained data as stale, disables daemon-owned mutations, and presents reconnect and explicit start actions

#### Scenario: Protocol is incompatible
- **WHEN** attachment fails because the daemon and TUI have no compatible protocol or required capability version
- **THEN** the TUI displays both compatibility ranges and upgrade guidance and does not repeatedly send unsupported operations

#### Scenario: Runtime becomes degraded
- **WHEN** the daemon reports a degraded runtime or configuration state while IPC remains available
- **THEN** the TUI continues read-only inspection, displays the causal diagnostics, and prevents mutations that the daemon marks unsafe

### Requirement: TUI-004: Complete keyboard navigation and visible focus
Every interactive TUI element SHALL be reachable and operable without a mouse. Left/Right and Tab/Shift+Tab SHALL move between primary views, Up/Down SHALL move the current selection, PageUp/PageDown SHALL page scrollable content, Enter SHALL open or confirm the focused action, Space SHALL request enable or disable for a toggleable selection, `r` SHALL refresh or retry the current connection, `?` SHALL open keyboard help, Esc SHALL close the topmost overlay before exiting from the root view, and Ctrl+C SHALL always request a clean exit. The current focus and context-appropriate shortcuts SHALL remain visibly identified.

#### Scenario: Operator navigates using only the keyboard
- **WHEN** an operator starts the TUI and uses only the documented keys
- **THEN** the operator can move between views, select rows, inspect details, scroll logs, invoke an available action, close overlays, and exit without an inaccessible control

#### Scenario: Escape closes nested UI first
- **WHEN** a detail, help, error, or confirmation overlay is open and the operator presses Esc
- **THEN** only the topmost overlay closes and the underlying selection and view remain active

### Requirement: TUI-005: Authoritative mutations and CLI equivalence
Every TUI mutation SHALL invoke the same operator operation and validation used by its documented CLI equivalent. The TUI SHALL display a pending state, prevent duplicate submission, wait for the daemon result, and reconcile the view from an authoritative response or snapshot instead of committing an optimistic local toggle. Every TUI action SHALL have a non-interactive CLI path with equivalent target selection, permission checks, and result semantics.

#### Scenario: Domain toggle succeeds
- **WHEN** an operator toggles a uniquely selected domain and the daemon accepts the request
- **THEN** the row remains pending until the authoritative state confirms the new activation state and the result matches `cadder domain enable` or `cadder domain disable`

#### Scenario: Mutation is rejected
- **WHEN** the daemon rejects a TUI action because of ambiguity, conflict, permission, configuration, or protocol policy
- **THEN** the prior authoritative state remains displayed, the pending marker clears, and the TUI presents the typed error and CLI-equivalent recovery guidance

#### Scenario: Detached entrypoint is forgotten
- **WHEN** an operator confirms forget for an entrypoint without a live lease
- **THEN** the TUI invokes the same operation as `cadder entrypoint forget`
- **AND** it refreshes the authoritative entrypoint list while retained logs and history remain available

### Requirement: TUI-006: Structured log inspection
The TUI SHALL render structured daemon log events for runtime, entrypoint, and domain streams. It SHALL support stream and minimum-severity selection, cursor-based continuation, inclusive time-range queries, tail follow, manual pause through scrolling, and explicit notices for empty, stale, removed, read-error, retention-gap, and truncation states. Scrolling away from the newest entry SHALL pause follow mode, and returning to the end SHALL resume it.

#### Scenario: Log view starts empty
- **WHEN** the selected stream has no retained entries
- **THEN** the log view displays the stream identity and an explicit empty-state message rather than a blank terminal panel

#### Scenario: Operator inspects earlier entries
- **WHEN** the operator scrolls upward while following an active stream
- **THEN** the visible entries remain stable while new entries are retained off-screen and the TUI indicates that follow mode is paused

#### Scenario: Log continuity is lost
- **WHEN** the daemon reports a cursor gap or retention truncation
- **THEN** the TUI inserts a non-color-only continuity notice before the next available entries and does not imply that the displayed history is complete

### Requirement: TUI-007: Responsive and bounded terminal layout
At `80x24` and larger, the TUI SHALL present its full navigation, status, content, and shortcut regions without overlap. At narrower or shorter sizes, it SHALL use a compact or stacked layout, preserve the focused item where possible, truncate text with an explicit visual indication, and keep essential status and exit guidance reachable. If the terminal cannot fit the minimum interactive layout, it SHALL render a resize notice and SHALL NOT panic or render outside the available buffer.

#### Scenario: Terminal is resized narrower
- **WHEN** a running TUI is resized below the full-layout width
- **THEN** it switches to its compact layout, preserves the active view and selection, and renders every widget within the new terminal bounds

#### Scenario: Terminal is extremely small
- **WHEN** the available area cannot contain the minimum interactive layout
- **THEN** the TUI displays a bounded resize message, continues processing resize and exit keys, and does not attempt a mutation from hidden controls

### Requirement: TUI-008: Accessible status and color behavior
The TUI SHALL communicate focus, activation, health, pending work, errors, and log severity with text or symbols in addition to color. It SHALL honor the CLI `--no-color` mode, maintain readable contrast in supported terminal themes, avoid meaning that depends on animation, and keep the CLI as a documented accessible alternative for every TUI action.

#### Scenario: Color is disabled
- **WHEN** the operator launches `cadder tui --no-color`
- **THEN** all status, focus, selection, severity, and error meanings remain distinguishable without ANSI color styling

#### Scenario: Operator requires a non-full-screen alternative
- **WHEN** an operator cannot use the full-screen interface
- **THEN** visible help identifies the equivalent CLI command for the focused action or workflow

### Requirement: TUI-009: Safe terminal lifecycle and bounded background work
The TUI SHALL restore the terminal's input mode, cursor, and alternate-screen state after normal exit, Ctrl+C, initialization failure, rendering failure, or a captured panic. Background state and log work SHALL be cancellable, bounded, and unable to block keyboard handling or terminal restoration. Closing the TUI SHALL cancel outstanding reads and SHALL NOT stop the daemon or real Caddy process.

#### Scenario: Rendering fails after terminal initialization
- **WHEN** a rendering or event-processing error occurs after raw mode and the alternate screen are enabled
- **THEN** Cadder restores the terminal before returning a classified error to the shell

#### Scenario: Operator exits during an in-flight request
- **WHEN** the operator presses Ctrl+C while a state refresh or log request is pending
- **THEN** the TUI cancels its background work, restores the terminal, leaves daemon-owned processes running, and returns control to the shell without partial escape sequences
