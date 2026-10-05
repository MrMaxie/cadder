## 1. CLI Contract And Dependencies

- [x] 1.1 Add exact, focused socket and process dependencies through Cargo and isolate their use to the operator client.
- [x] 1.2 Split command parsing and dispatch from the TUI entrypoint, preserving successful bare help, version, and TUI behavior.

## 2. Read And Activation Workflows

- [x] 2.1 Implement compact status, project list, and domain list output from a current daemon snapshot.
- [x] 2.2 Implement explicit project and domain enable/disable commands through the existing operator API.
- [x] 2.3 Replace stale API recovery guidance with the delivered public command vocabulary.
- [x] 2.4 Expose existing daemon lifecycle, diagnostics, and bounded redacted log operations through explicit CLI commands.

## 3. Bidirectional Inspection

- [x] 3.1 Implement cross-platform socket-owner discovery and process metadata behind a testable client-side boundary.
- [x] 3.2 Implement local-upstream parsing and one shared correlation model for ports, registrations, Caddyfiles, domains, and process owners.
- [x] 3.3 Implement port-first inspection that remains useful when the daemon is unavailable.
- [x] 3.4 Implement Caddyfile-first and domain-first inspection with independent registration, activation, runtime, configuration, and listener states.

## 4. Guarded Process Control

- [x] 4.1 Implement expected-PID port-owner revalidation and bounded process signaling with existing exit categories.
- [x] 4.2 Cover ownership changes, multiple owners, hidden process information, local aliases, and remote upstream exclusion in deterministic tests without live sockets.

## 5. Product Consistency And Documentation

- [x] 5.1 Remove the placeholder MCP status from the TUI and keep only real `cadderd` and Caddy states.
- [x] 5.2 Update CLI help, user documentation, and durable specs for the delivered commands and diagnostic boundary.
- [x] 5.3 Review the final diff, compile the affected targets, and perform command-level and rendered TUI verification.
- [x] 5.4 Run the workspace and Docker E2E suites inside isolated containers and repair discovered portability, fixture, and assertion failures.
