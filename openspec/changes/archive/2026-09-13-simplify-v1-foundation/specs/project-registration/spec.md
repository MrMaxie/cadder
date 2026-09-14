## MODIFIED Requirements

### Requirement: REG-007: Shim PATH setup preserves existing Caddy installations
Cadder SHALL distribute its compatibility shim as `caddy` inside the portable archive. Extracting or upgrading an archive MUST NOT create, replace, or remove entries in another PATH directory.

Cadder SHALL copy or link the shim into a PATH directory only through explicit `cadder setup shim`. Without `--dir`, setup SHALL use the documented per-user command directory. An explicit directory MUST already exist, be owner-writable, belong to the current user's PATH, and not be administrator-owned or shared with another user. Setup MUST verify an empty destination or Cadder-owned provenance; removal MUST delete only an entry whose recorded provenance and current target both identify Cadder's installed shim.

#### Scenario: Empty PATH destination
- **WHEN** a user explicitly requests shim setup at a writable destination with no `caddy` entry
- **THEN** Cadder creates the PATH entry and records owner-readable provenance
- **AND** the independently installed real Caddy remains unchanged and discoverable

#### Scenario: Existing command collision
- **WHEN** the selected destination already contains a real Caddy executable or an unrelated `caddy` entry
- **THEN** setup reports a conflict without overwriting, renaming, or deleting the entry

#### Scenario: Explicit destination is unsafe
- **WHEN** `--dir` names a missing, non-PATH, shared, administrator-owned, or non-owner-writable directory
- **THEN** setup rejects the destination without creating a PATH entry or provenance record

#### Scenario: Provenance mismatch during removal
- **WHEN** `cadder setup shim --remove` finds that the PATH entry target or provenance has changed
- **THEN** removal fails closed and leaves the entry unchanged

## RENAMED Requirements

- FROM: `### Requirement: REG-007: Shim alias setup preserves existing Caddy installations`
- TO: `### Requirement: REG-007: Shim PATH setup preserves existing Caddy installations`
