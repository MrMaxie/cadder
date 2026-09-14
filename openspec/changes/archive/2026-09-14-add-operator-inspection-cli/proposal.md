## Why

Cadder already holds the relationships between projects, Caddyfiles, domains, and upstreams, but exposes them only through the TUI. Developers need a scriptable way to start from either a local port or a Caddyfile, understand the registered route, and deliberately stop an occupying process without guessing from unrelated system tools.

The existing `caddy run` and `cadder tui` journey remains intact. The smallest end-to-end addition is a read-oriented CLI that exposes the daemon state and correlates it with local socket owners, plus one explicitly guarded process-stop action.

## What Changes

- **BREAKING** Replace the operator CLI requirement that only `tui` is accepted with a focused status and inspection command set.
- Add general status, project, and domain listing commands backed by the daemon snapshot.
- Add explicit daemon start, stop, and restart commands through the existing operator API.
- Add on-demand diagnostics and bounded redacted log reads so detailed operational data stays out of the routine TUI.
- Add port inspection that reports local socket owners and any matching Cadder registrations.
- Add Caddyfile inspection that reports whether the file is registered, enabled, and contributing active domains, including the corresponding upstream ports and process owners.
- Add an explicit port-owner stop command that requires the caller to provide the expected process ID and revalidates ownership before signaling the process.
- Keep process discovery and termination local to the invoked CLI command; `cadderd` does not discover or terminate unrelated processes.
- Remove the placeholder MCP status from the TUI. MCP remains outside the product boundary.
- Replace recovery guidance that references unavailable commands with commands delivered by this change.

Success means a developer can enter through a project, domain, Caddyfile, or port and reach the same current Cadder registration data without exposing private runtime implementation details in the routine TUI.

Non-goals are machine-readable output, continuous log tailing or watching, history, profiles, autostart, automatic process termination, and adding an MCP surface.

## Capabilities

### New Capabilities

- `operator-inspection`: Correlate Cadder registrations with local ports, Caddyfiles, and explicitly managed process-owner actions.

### Modified Capabilities

- `operator-cli`: Expand the supported operator commands beyond the TUI while preserving help, version, and TUI behavior.
- `product-topology`: Recognize the read-oriented operator CLI as a user-facing surface while retaining MCP outside the product.

## Impact

- The `cadder` executable gains command parsing, text rendering, local socket inspection, and guarded process termination modules.
- The client depends on focused cross-platform crates for socket ownership and process control instead of custom OS command parsing.
- Operator documentation and recovery guidance gain real commands for current state inspection.
- The daemon protocol and ownership model remain unchanged.
