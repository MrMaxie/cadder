# Cadder TUI Smoke Checklist

Use this checklist for manual terminal smoke verification of `cadder-tui` on supported terminal backends after lifecycle or TUI-facing changes. Automated tests cover the model and IPC contracts; this checklist verifies the real terminal loop, keyboard handling, and rendering.

## Prerequisites

- Build the workspace with `cargo build --workspace`.
- Use a disposable test runtime directory.
- Keep `cadderd` stopped for the initial no-daemon startup smoke.
- For connected-state checks, start `cadderd` with the disposable runtime directory and a fake or local real Caddy command, or launch it from inside `cadder-tui` with `s`.
- Register at least two shim sessions whose adapted Caddyfiles expose multiple domains.
- Keep one domain disabled or conflicting when diagnostics need to be visible.
- For Windows IIS handoff smoke, use a normal user shell first and a disposable IIS site or binding that can be removed and restored safely. Cadder should request elevation only for the IIS mutation batch. Do not use production IIS bindings. Follow the [Windows IIS handoff cookbook](../site/src/content/docs/cookbooks/windows/iis.mdx) for the operator flow.

## Terminal Backends

Run the checklist on each backend available for the platform under test:

- Windows Terminal on Windows.
- A VT-compatible terminal on Linux.
- Terminal.app or iTerm2 on macOS.

## Checklist

- Launch `cargo run -p cadder-tui -- --runtime-dir <runtime-dir> --no-start` while no daemon is running for that runtime and confirm the Overview view renders without layout corruption, panic, immediate exit, or broken terminal echo/cursor behavior.
- Confirm Overview shows `Daemon: Not running` or `Daemon: Connection failed` with actionable guidance to press `s`, `r`, or `q`.
- Confirm the footer advertises `s start/reconnect`.
- Press `r` and confirm the TUI remains usable if the daemon is still unavailable.
- Relaunch with `--daemon-path <path-to-cadderd>` if `cadderd` is not next to `cadder-tui` or on `PATH`, press `s`, and confirm the Overview state changes to `Starting` while input remains responsive.
- After a successful in-TUI start, confirm Overview changes to `Daemon: Connected` and normal state refresh resumes.
- If testing a start failure, pass a missing `--daemon-path`, press `s`, and confirm Overview changes to `Start failed` while retry with `s` and quit with `q` remain available.
- Confirm Overview shows runtime status, config status, entrypoint count, domain count, and active domain count.
- Change daemon state from a shim session and confirm Overview updates automatically within a few seconds; press `r` and confirm manual refresh still works.
- Press `Tab` or `Right` to open Entrypoints and confirm each active shim registration is listed with ID, state, source path, and domain count.
- Press `Tab` or `Right` to open Domains, then use `Shift+Tab` or `Left` to navigate backward and confirm view navigation wraps cleanly.
- Confirm domains remain associated with their source entrypoint in the first table column.
- Confirm the selected Domains row is visibly highlighted, active domains show `[x]`, and disabled domains show `[ ]` with a lower-contrast disabled style.
- Select a domain with arrow keys, press `Space`, and confirm the domain activation state toggles after the automatic or manual refresh.
- On Windows, press `Tab` or `Right` to open IIS Handoff and confirm the view lists IIS site bindings with site, protocol, IP address, port, host header, state, and safety details. On Linux and macOS, confirm the IIS Handoff view is absent.
- In IIS Handoff on Windows, confirm unsupported bindings are visible inline and cannot be handed off: unsupported protocols, unsupported ports, duplicate host bindings, active Cadder-domain conflicts, missing route hosts, HTTPS bindings with missing TLS certificate metadata, elevation-denied results, unsupported elevation paths, and insufficient privilege errors should show a safety reason instead of silently changing IIS.
- Before pressing `Space` on an available IIS row, confirm the status line explains why administrator approval may be requested. The daemon and TUI should still be running in the normal user context.
- For a disposable `http:80` or `https:443` IIS binding with a concrete host header, press `Space` and confirm the binding moves to `HandedOff`, Caddy remains the front door, and the status line reports the loopback IIS backend port. For HTTPS, use a binding with readable certificate metadata and confirm an application with IIS Require SSL enabled still sees an HTTPS backend request through Caddy.
- When Windows shows the elevation prompt, approve it and confirm the status line reports succeeded steps and admin-approved IIS mutation steps. Confirm there was one prompt for the handoff mutation batch, not separate prompts for Caddy config, registration state, or discovery.
- Repeat with another disposable binding and deny the elevation prompt. Confirm the row remains available or recoverable, restore metadata is not treated as complete handoff state, and non-IIS TUI actions such as navigation, refresh, logs, and daemon state remain usable. Confirm the status line reports admin-denied and a retry-elevation follow-up.
- For a disposable wildcard or empty-host IIS binding, press `/`, enter the intended route host or full URL, press `Enter`, then press `Space`; confirm the handed-off row shows the route host and the target URL is served through Caddy to IIS. For HTTPS, confirm direct loopback verification keeps the route host as SNI, for example with `curl.exe -k --connect-to "<host>:<backendPort>:127.0.0.1:<backendPort>" "https://<host>:<backendPort>/..."`. Application-level `4xx` or `5xx` responses can be valid smoke results when the target application is intentionally misconfigured; verify routing by checking that the response comes through Caddy and reaches IIS, for example `Via: 1.1 Caddy` with `Server: Microsoft-IIS/...`.
- Press `Space` on the handed-off IIS row again and confirm the original IIS binding is restored, the loopback backend binding is removed, and restore metadata is cleared after success. If other Cadder routes still need `:80` or `:443`, confirm restore is rejected rather than silently breaking those routes.
- If testing a failure path, force Cadder route apply to fail after IIS removal and confirm the UI reports whether rollback restored the IIS binding or rollback failed, including step status and follow-up action details. If rollback fails, confirm the row remains recoverable instead of losing restore metadata. Do not proceed until the disposable binding is back under the expected owner.
- Press `Enter` or `l` on a domain row and confirm Logs opens for that exact domain, not another entrypoint or domain.
- In Logs, press `p` and confirm tailing pauses and resumes.
- With the domain log stream still open, switch to Settings, use `Up` and `Down` to choose `All`, `Info and higher`, `Warnings and errors`, or `Errors only`, then press `Enter` or `Space`; return to Logs and confirm the severity label and displayed page reset without mixing entries from different filters.
- Press `Enter` in Logs and confirm manual refresh remains responsive.
- Press `Tab` to open Diagnostics and confirm config/runtime diagnostics are shown when a conflict or runtime reload failure exists; otherwise confirm the empty diagnostics message is shown.
- Confirm the footer advertises `Tab`/`Shift+Tab`/`Left`/`Right`, manual refresh, daemon start/reconnect, Settings severity selection, log pause/refresh/export, daemon shutdown, and quit controls.
- On Windows, confirm the footer advertises IIS refresh and handoff/restore controls.
- Press `d` and confirm the daemon shutdown request returns a status message and does not freeze terminal input.
- Press `q` and confirm the terminal exits cleanly with normal echo/cursor behavior restored.

## Evidence To Record

- Platform and terminal backend.
- Runtime directory used.
- Fake Caddy or real Caddy command used.
- Whether IIS handoff was skipped, tested with a disposable binding, or tested with fake-provider data.
- Any rendering issue, stuck key flow, incorrect status, or daemon error observed.
