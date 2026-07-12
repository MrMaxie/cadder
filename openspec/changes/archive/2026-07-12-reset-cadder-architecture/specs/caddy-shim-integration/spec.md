## ADDED Requirements

### Requirement: Shim command policy is explicit
The `caddy` shim SHALL classify every supported Caddy command path as managed,
read-only inspection, explicit passthrough, or unsupported.

#### Scenario: Managed registration command
- **WHEN** a command provides a Caddyfile, config, or project definition that Cadder manages
- **THEN** the shim SHALL send the definition to `cadderd`
- **AND** it SHALL NOT start a separate real Caddy process

#### Scenario: Unsupported command
- **WHEN** a user invokes a Caddy command that has no policy entry
- **THEN** the shim SHALL return a clear unsupported-command diagnostic
- **AND** it SHALL NOT silently pass the command to real Caddy

### Requirement: Shim handles daemon unavailable states
The shim SHALL give useful diagnostics when `cadderd` is unavailable and SHALL
avoid state drift during fallback behavior.

#### Scenario: Managed command while daemon is down
- **WHEN** the shim receives a managed command and cannot contact `cadderd`
- **THEN** it SHALL explain that Cadder cannot update managed runtime state
- **AND** it SHALL suggest the supported `cadder daemon start` recovery path

#### Scenario: Read-only command while daemon is down
- **WHEN** the shim receives a read-only inspection command and cannot contact `cadderd`
- **THEN** it MAY call real Caddy only if the command cannot mutate Cadder-managed state
- **AND** it SHALL label the result as real-Caddy inspection rather than Cadder runtime state

### Requirement: Shim prevents recursive real-Caddy execution
The shim SHALL resolve real Caddy without recursively executing itself.

#### Scenario: Real Caddy command configured
- **WHEN** `CADDER_CADDY_REAL_COMMAND` is set
- **THEN** the shim SHALL use that command as the first real-Caddy candidate
- **AND** it SHALL reject candidates that resolve to the shim binary

#### Scenario: PATH contains shim before real Caddy
- **WHEN** PATH lookup finds the shim binary before real Caddy
- **THEN** the shim SHALL skip itself
- **AND** it SHALL continue searching or return a clear real-Caddy-not-found diagnostic

### Requirement: Cadder owns effective Caddy config composition
`cadderd` SHALL compose the effective Caddy configuration from Cadder runtime
state and SHALL apply it atomically to real Caddy.

#### Scenario: Multiple projects register definitions
- **WHEN** multiple projects register Caddy definitions
- **THEN** `cadderd` SHALL merge them into one effective runtime model
- **AND** it SHALL apply the resulting Caddy config as a single coherent update

#### Scenario: Caddy rejects generated config
- **WHEN** real Caddy rejects a generated config update
- **THEN** `cadderd` SHALL keep or restore the last known good runtime state
- **AND** it SHALL report the validation error to clients and logs

### Requirement: Direct Caddy Admin API drift is detectable
Cadder SHALL treat Caddy Admin API state as adapter state and SHALL detect or
repair drift when possible.

#### Scenario: Caddy config changes outside Cadder
- **WHEN** real Caddy active config no longer matches Cadder's last applied config identity
- **THEN** `cadderd` SHALL mark runtime state as drifted
- **AND** it SHALL offer a controlled reapply or reconcile action
