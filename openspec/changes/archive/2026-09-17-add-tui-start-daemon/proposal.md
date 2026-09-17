## Why

The homepage currently recommends `npx cadder tui`, but opening the TUI alone does not start an offline Cadder daemon. A new user should be able to follow the hero without discovering an additional keyboard action after the command has started.

The change preserves the existing operator-first journey and delivers one small end-to-end capability: explicitly request daemon startup while opening the TUI.

## What Changes

- Add `cadder tui --start-daemon` as an explicit, idempotent startup option.
- Start or attach to `cadderd` before the full-screen operator opens when the option is present.
- Keep plain `cadder tui` read-first and unchanged.
- Update the homepage hero and operator documentation so the prerequisite for separately installed upstream Caddy and the startup command are visible together.
- Do not add OS login autostart, another daemon lifecycle path, or an alias named `autostart`.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `operator-cli`: the TUI command gains an explicit option that reuses the existing attach-first daemon launch contract.
- `documentation-experience`: the hero presents a complete first-run path instead of an incomplete command.

## Impact

- `crates/cadder-client`: CLI parsing, TUI startup sequencing, and focused tests.
- `openspec/specs/operator-cli` and `openspec/specs/documentation-experience`: public behavior and onboarding requirements.
- `docs/site`: homepage hero and operator guidance.
- No new dependency, protocol, daemon behavior, system startup integration, or platform-specific script is introduced.
