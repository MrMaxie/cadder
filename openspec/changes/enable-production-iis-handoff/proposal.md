## Why

Cadder already contains tested IIS discovery, scoped elevation, proxy-route composition, rollback, and restore logic, but production IPC deliberately returns `HandoffUnavailable` and the operator exposes no IIS command. As a result, a normal Windows setup can leave IIS on its loopback backend while Cadder cannot restore the public front door or run IIS and shim-managed projects together.

The existing test-only mutation path writes an elevated temporary PowerShell script and does not provide the preview, state revalidation, or one-shot helper boundary required by the product contract. This change implements the production preview and apply flow instead of promoting that unsafe test seam.

## What Changes

- Enable production IIS binding discovery, handoff preview, apply, restore preview, and restore through the daemon IPC operation fence.
- Expose `cadder iis status`, `cadder iis preview handoff <binding-id> [--route-host <host>]`, `cadder iis preview restore <binding-id>`, `cadder iis apply <plan-id>`, and `cadder iis restore <plan-id>`.
- Add an IIS view to the real TUI and route its actions through the same operator client.
- Keep the daemon, shim, CLI, and TUI unelevated; request one scoped UAC approval only after the operator reviews an immutable plan.
- Launch the installed `cadderd` as a one-shot elevated helper that accepts only the selected allowlisted IIS binding batch over a private authenticated channel; do not elevate a generated PowerShell script.
- Persist enough original binding metadata before mutation to restore only the selected binding, revalidate the expected pre-state before mutation, and retain transactional rollback on Caddy apply failure.
- Add a Windows integration smoke that proves an IIS route and shim-managed Caddyfile run concurrently through one Cadder-owned Caddy process.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `windows-iis-handoff`: Require the installed allowlisted helper to execute typed IIS operations directly and prohibit elevation of generated temporary scripts; the existing operator CLI, TUI, and control-plane requirements are implemented without changing their public contracts.

## Impact

- Affected crates: `cadder-protocol`, `cadder-daemon`, `cadder-operator`, `cadder`, and `cadderd`.
- Affected Windows systems: IIS binding discovery and the selected public/loopback binding pair; unrelated sites and bindings remain untouched.
- The public operator command surface implements its specified IIS commands and the TUI gains its specified IIS view.
- No new third-party dependency or persistent background elevated service is introduced.
