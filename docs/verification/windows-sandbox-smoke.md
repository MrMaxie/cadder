# Windows Sandbox Smoke Checklist

Cadder system-facing behavior should be smoke-tested in Windows Sandbox before it is trusted on a host workstation. The sandbox pass is intentionally focused on OS effects that are hard to validate safely in normal unit or integration tests.

## Scope

Run this checklist for the v1.0 runtime topology:

- `cadderd` per-user daemon.
- `caddy` PATH-facing shim.
- `cadder` operator executable, including CLI and TUI.

Use the same v1.0 package layout as release verification: `cadderd.exe`, `cadder.exe`, `caddy.exe`, and `cadder.toml`.

## Sandbox Inputs

Prepare a disposable package that contains only:

- `cadderd.exe`
- `caddy.exe`
- `cadder.exe`
- sample `cadder.toml`
- optional mock real-Caddy fixture for deterministic shim tests
- smoke script and cleanup script

Use a clean sandbox image for each release candidate. Prefer mock backend tests first, then a real Caddy binary when validating resolver and process behavior.

## Smoke Steps

1. Install the package into a disposable directory under the sandbox user profile.
2. Put the Cadder shim directory before real Caddy on the user `PATH`.
3. Run `cadder daemon status` and confirm unavailable-daemon guidance is actionable.
4. Run `cadder daemon start` and confirm the daemon is detached from the launching shell.
5. Run `cadder daemon status` and confirm runtime, storage, and Caddy resolver state.
6. Run `caddy run` from a disposable project and confirm the shim does not recursively execute itself.
7. Open `cadder tui` and confirm it works from a normal user shell.
8. Use the TUI to inspect daemon status, entrypoints, domains, logs, diagnostics, history, autostart, and settings.
9. Enable daemon autostart through `cadder autostart set daemon`.
10. Restart the sandbox session or simulate logon where possible, then confirm the daemon autostart target is present and valid.
11. Disable autostart through `cadder autostart set disabled` and confirm the target is removed.
12. Run `cadder daemon shutdown` and confirm the daemon exits without killing unrelated Caddy processes.
13. Run uninstall or cleanup and confirm binaries, runtime state, and autostart entries are removed.

## Pass Criteria

- All shipped binaries run from the sandbox package without requiring repository sources.
- The package contains exactly the v1.0 runtime files listed above.
- The daemon owns only the real Caddy process it starts.
- The shim never resolves itself as real Caddy.
- Autostart enable and disable are reversible.
- Cleanup leaves no Cadder PATH, autostart, or runtime residue in the sandbox.

## Evidence To Capture

Capture the following for release review:

- `cadder daemon status` before and after daemon startup.
- `cadder diagnostics` after shim registration.
- `cadder logs --limit 50`.
- `cadder history --limit 50`.
- screenshot of `cadder tui` on the connected state.
- autostart query output after enable and after disable.
- cleanup script output.
