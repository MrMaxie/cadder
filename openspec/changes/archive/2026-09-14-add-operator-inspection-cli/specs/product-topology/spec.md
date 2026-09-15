## MODIFIED Requirements

### Requirement: TOP-002: Product and diagnostic surfaces remain distinct
Managed `caddy run`, the read-oriented and activation-oriented `cadder` CLI, and `cadder tui` SHALL be the user-facing 1.0 surfaces, while foreground `cadderd`, bounded logs, and explicit process details SHALL remain diagnostic surfaces. MCP SHALL NOT be presented as a Cadder product surface.

#### Scenario: User opens the operator
- **WHEN** the user runs `cadder tui`
- **THEN** Cadder opens the routes workspace with daemon, Caddy, and activation state

#### Scenario: Developer inspects current state
- **WHEN** the user runs a supported status, project, domain, port, or Caddyfile command
- **THEN** Cadder reports the requested current state without exposing unrelated local diagnostics
