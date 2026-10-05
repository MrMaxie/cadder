## Context

`cadder tui` currently opens the operator and renders the daemon as offline when `cadderd` is unavailable. The user can then press Enter to invoke the existing attach-first startup path. The homepage command does not explain this second interaction, so it is not a complete copy-and-run first step.

The operator already exposes `cadder daemon start`, and `OperatorContext::ensure_daemon_running` already owns the idempotent start-or-attach behavior. The change can reuse that path without adding another process manager or platform-specific implementation.

## Goals / Non-Goals

**Goals:**

- Let a user explicitly start or attach to `cadderd` before the TUI opens.
- Preserve plain `cadder tui` as a read-first command with its existing offline state and Enter action.
- Give the homepage a complete first-run sequence that also names upstream Caddy as a separate prerequisite.
- Keep startup errors on the operator's existing structured exit-code and guidance path.

**Non-Goals:**

- Starting Cadder automatically on login or persisting an autostart preference.
- Starting an unrelated system Caddy service.
- Hiding a missing or invalid upstream Caddy configuration.
- Adding shell scripts, OS-specific launchers, or another background-process abstraction.

## Decisions

The public option is named `--start-daemon`, not `--autostart-daemon`. "Autostart" commonly means login or boot persistence and is already an intentionally unsupported Cadder surface. The imperative name describes this invocation only.

Clap will model the TUI command as `Tui { start_daemon: bool }`. Command execution will create one `OperatorContext`. When the option is set, it will call `ensure_daemon_running("tui")` and wait for the existing bounded readiness check before handing the same context to the TUI. Without the option, execution will hand the context directly to the TUI exactly as it does today.

Startup remains backgrounded by the existing daemon launch contract. The new option does not spawn or detach a process itself. This keeps Windows, Linux, and macOS behavior behind the current shared runtime abstraction.

Focused parser tests will cover the default and opt-in forms. Existing API and daemon tests remain the seam for attach-first startup behavior; the client test will not launch a real daemon or require a real Caddy installation.

The hero will present upstream Caddy as a visible prerequisite and `npx cadder tui --start-daemon` as the first Cadder command. Supporting copy will state that the command starts Cadder when needed and then opens the operator.

## Risks / Trade-offs

- The longer command is less visually compact, but it is honest and actionable.
- Startup can fail before terminal raw mode begins. This is preferable because the existing CLI error report remains readable and the terminal does not require recovery.
- A user may interpret daemon startup as also installing upstream Caddy. The hero and getting-started guidance must keep that prerequisite explicit.

## Migration Plan

1. Add and test the optional CLI flag without changing the default TUI behavior.
2. Update the hero and operator documentation to use the explicit startup form for first-run guidance.
3. Validate OpenSpec, Rust, and documentation, then inspect the rendered desktop and mobile hero.
4. Roll back by removing the optional flag and reverting the first-run command; no persisted state or data migration is involved.

## Open Questions

None.
