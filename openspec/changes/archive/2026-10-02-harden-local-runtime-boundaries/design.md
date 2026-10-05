## Context

Cadder runs one real Caddy process for trusted projects on a personal development workstation. Project Caddyfiles remain trusted inputs, but Cadder owns the shared listeners, active-host boundary, retained diagnostics, and managed daemon launch. The current generated configuration listens on wildcard addresses, recursively edits project routes without guarding their full route tree, captures unbounded adaptation output, and may launch `cadderd` from PATH even though the supported installation keeps version-matched executables together.

## Goals / Non-Goals

**Goals:**

- Preserve managed `caddy run` while making every Cadder-owned HTTP and HTTPS listener loopback-only.
- Guarantee that one registration cannot handle a host outside its active domain set.
- Complete the existing bounded-output and redaction contracts without building a general secret scanner.
- Keep terminal control bytes inert at the presentation boundary.
- Align automatic daemon launch with the documented version-matched sibling installation.

**Non-Goals:**

- Protect one operating-system account from another account on a shared host.
- Replace the loopback Caddy administration endpoint.
- Treat trusted project files or the current user as adversarial code.
- Add new ports, commands, IPC fields, installation forms, or an executable snapshot cache.

## Decisions

Cadder emits explicit IPv4 and IPv6 loopback listeners for both existing ports. Listener ownership remains in configuration composition, so adapted project input cannot widen it.

Each registration contributes one outer host-matched route whose subroute contains its filtered adapted routes. Recursive filtering remains useful for removing inactive host values, but the outer matcher is the security and isolation boundary even when an adapted route contains hostless sibling handlers.

`caddy adapt` uses the existing bounded process-tree capture with a 32 MiB limit per output stream. Limit failures retain the existing process cleanup guarantee and never parse partial output.

Redaction uses a small set of precompiled regular expressions plus a focused private-key block recognizer. The options were manual string parsing, the mature `regex` crate, and a general secret-scanning framework. Manual parsing adds more delimiter and escaping cases, while a framework introduces configuration and false-positive policy beyond the accepted contract. Focused regular expressions add one established dependency and cover only authorization, cookie, token, password, and private-key forms named by OpenSpec.

Terminal sanitation remains presentation-only. A small standard-library state machine removes ANSI CSI and OSC sequences and remaining C0/C1 controls from diagnostic and log messages before `comfy-table` renders them. Persisted redacted logs remain otherwise unchanged.

Automatic launch accepts the version-matched sibling `cadderd`. The existing explicit path stays available to focused tests and foreground diagnostics. Native PATH discovery is removed from this launch path, while trusted real-Caddy resolution remains unchanged.

## Risks / Trade-offs

- Binding IPv4 and IPv6 loopback can expose a platform-specific listener failure that wildcard binding previously hid. Startup already reports Caddy configuration failures and must fail rather than widen the listener.
- Focused redaction cannot recognize every possible secret name. Tests cover only the forms promised by OBS-001, and the implementation does not claim general detection.
- Stripping terminal controls can change the visual representation of a diagnostic. The retained redacted source remains available through the existing storage contract.
- A 32 MiB adaptation response can still be large, but it bounds daemon memory while allowing substantial generated configurations.

## Migration Plan

1. Add focused failing tests for listener addresses, guarded route trees, adaptation limits, required redaction forms, terminal sanitation, and sibling-only daemon discovery.
2. Implement each boundary using existing configuration, process-tree, log-store, and CLI ownership seams.
3. Raise the minimum and integration fixture Caddy patch version to 2.11.4.
4. Run focused crate tests followed by the complete repository gate.

Rollback restores the previous implementation and minimum Caddy patch version. No stored data or wire migration is required.

## Open Questions

None.
