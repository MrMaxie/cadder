## MODIFIED Requirements

### Requirement: CLI-008: Safe daemon control and diagnostics
`daemon start`, `daemon stop`, and `daemon restart` SHALL be idempotent, SHALL operate only on the selected per-user daemon, and SHALL report the final observed state. Restart SHALL wait for the owned daemon to stop before starting and confirming readiness. `doctor` SHALL perform non-mutating checks of local endpoint readiness, IPC compatibility and permissions, storage, configuration, real-Caddy resolution, and platform integrations, and SHALL return the most specific blocking exit category when the installation is not operational.

#### Scenario: Start finds an already running daemon
- **WHEN** the selected daemon is already ready and the operator runs `cadder daemon start`
- **THEN** the command reports that no start was needed, leaves the process unchanged, and exits successfully
