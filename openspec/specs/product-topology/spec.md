# Product topology

## Purpose
Define the minimal Cadder 1.0 product boundary, executable set, and separation between everyday and diagnostic surfaces.
## Requirements
### Requirement: TOP-001: Three executables form one installation
Cadder SHALL provide version-matched cadder, cadderd and caddy entrypoints from one TypeScript product in both npm and standalone distributions. The daemon SHALL be the sole runtime/state/process owner. Runtime v2 SHALL be owner-protected and installation-specific, with deliberately isolated dev/test runtimes; separate installation directories SHALL retain independent endpoints and application databases. Rust/Cargo SHALL be removed from the final product only after complete Node functionality and both release gates.

#### Scenario: Distribution channel changes
- **WHEN** the owner uses npm or standalone Cadder of the same version
- **THEN** both SHALL expose the same product contracts and owner runtime policy
- **AND** clients SHALL NOT create independent Caddy or persistent state owners

#### Scenario: Separate installations
- **WHEN** the same owner runs Cadder from two different installation directories
- **THEN** each installation SHALL use an independent endpoint and application database
- **AND** each installation's three entrypoints SHALL agree on its runtime identity

### Requirement: TOP-002: Product and diagnostic surfaces remain distinct
Cadder SHALL expose CLI and TUI workflows through one client service/model boundary, with diagnostics requested explicitly. Production state SHALL come from the daemon, not mocks or random values. The current routes-first TUI and bounded CLI diagnostic surfaces SHALL remain distinct. This migration SHALL NOT restore superseded history, IIS, autostart, machine-output, profile, export, watch or continuous-tail surfaces, or include MCP, Web UI, Tauri, Bun, custom WASM, native runtime addons or new installers.

#### Scenario: Client renders project state
- **WHEN** CLI or TUI renders a project, domain or lifecycle result
- **THEN** both SHALL use the same daemon-backed model and canonical state
- **AND** neither SHALL inspect daemon storage or Caddy administration directly
