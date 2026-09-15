# Windows Sandbox Smoke Test

Use a disposable Windows Sandbox to verify the released Windows x64 archives without relying on repository-only settings.

1. Verify the three archive SHA-256 files.
2. Extract `cadder.exe`, `cadderd.exe`, and `caddy.exe` into one user-owned directory.
3. Place a valid `cadder.toml` beside the executables.
4. Add the directory to the sandbox user's PATH.
5. Run `caddy run` in two projects with distinct domains and confirm one daemon serves both registrations.
6. Open `cadder tui` and inspect the separate `cadderd` and Caddy states and the routes workspace.
7. Run `cadder projects list`, `cadder domains list`, `cadder port inspect <upstream-port>`, and `cadder caddyfile inspect <path>` and confirm both inspection directions describe the same route.
8. Confirm Stop and Restart require confirmation and terminate only the Cadder-owned Caddy child.
9. Replace one executable with a different version and confirm the exact handshake rejects it without mutating state.
10. Restore the matched executable set and confirm the runtime reconnects.

Record archive names, checksums, executable versions, the configured real-Caddy source, and the observed TUI state. Do not include personal paths or secrets in published evidence.
