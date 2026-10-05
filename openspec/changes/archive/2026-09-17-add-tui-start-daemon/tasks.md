## 1. Add explicit TUI startup

- [x] 1.1 Add the `tui --start-daemon` parser contract while preserving plain `tui` behavior, with focused parser and help verification.
- [x] 1.2 Reuse the existing operator context and attach-first daemon launch path before opening the TUI, with focused client tests that do not require real Caddy.

## 2. Make the first-run path complete

- [x] 2.1 Update the hero and operator guidance to show upstream Caddy as a separate prerequisite and `npx cadder tui --start-daemon` as the copyable first Cadder command.
- [x] 2.2 Verify OpenSpec, focused Rust behavior, documentation automation, and the rendered desktop and mobile hero.
