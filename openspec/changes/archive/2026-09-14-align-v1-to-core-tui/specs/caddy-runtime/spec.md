## MODIFIED Requirements

### Requirement: CAD-001: Real Caddy comes only from explicit runtime sources
The daemon SHALL resolve one real-Caddy installation, in order, from an explicit foreground-daemon override, an owner-protected `cadder.toml` beside the Cadder executables, standard owner configuration, standard system configuration, or safe native PATH discovery. It MUST use a canonical absolute path to a regular native executable without a shell, exclude the active shim by file identity, and pin the resolved path, version, modules, file identity, and digest for the daemon lifetime. Project files, registration working directories, and shim-process environment overrides MUST NOT select real Caddy.

#### Scenario: Portable configuration selects real Caddy
- **WHEN** the installation configuration names a valid real-Caddy command or absolute path
- **THEN** the daemon verifies and pins a native executable distinct from the active shim

#### Scenario: Project attempts to select a command
- **WHEN** a project file, working directory, or registration argument names another Caddy command
- **THEN** Cadder rejects or ignores that selector and executes no project-controlled candidate

#### Scenario: PATH resolves to the shim
- **WHEN** a PATH candidate is the active shim or the same file under another name
- **THEN** the resolver excludes it and continues safe discovery

### Requirement: CAD-006: Cadder owns the composed runtime
The daemon SHALL compose validated project routes with Cadder-owned listeners, TLS, administration, storage, and logging. HTTP and HTTPS SHALL bind loopback addresses on the fixed user-level ports 8080 and 8443. Project input MUST NOT change or widen listeners. If either listener is unavailable, startup MUST fail with recovery guidance instead of elevating, binding a wildcard address, or choosing an undocumented fallback.

#### Scenario: Project route is composed
- **WHEN** a validated route enters the effective candidate
- **THEN** Cadder emits it only under its exact canonical host on the owned loopback listeners

#### Scenario: Default user-level listeners
- **WHEN** the runtime becomes ready
- **THEN** it serves HTTP on loopback port 8080 and HTTPS on loopback port 8443

#### Scenario: Listener is unavailable
- **WHEN** either fixed listener cannot be bound
- **THEN** startup fails without elevation or an alternate port

### Requirement: CAD-010: Active configuration drift is repaired once
The daemon SHALL compare a canonical hash of Caddy's active configuration with the last verified Cadder configuration at startup, after apply, after child recovery, and during health checks. It MUST make at most one restoration attempt for one observed drift event. A failed or recurring restoration MUST place the Caddy subsystem in degraded read-only state.

#### Scenario: Drift restoration succeeds
- **WHEN** one reload restores the expected canonical hash
- **THEN** Cadder records the outcome and allows normal mutations

#### Scenario: Drift restoration fails
- **WHEN** restoration fails or the hash still differs
- **THEN** Status and Logs remain available while configuration mutations are rejected

### Requirement: CAD-012: Local host and TLS behavior is explicit
Cadder SHALL serve exact `.localhost` hosts on loopback HTTP port 8080 and HTTPS port 8443. It MAY accept another exact host only when that host resolves exclusively to loopback during registration. HTTP SHALL remain available, and HTTPS SHALL use a Cadder-owned internal Caddy PKI. Cadder MUST NOT redirect HTTP to HTTPS by default, install a trust root silently, or elevate to change operating-system trust.

The daemon SHALL keep internal CA keys in owner-only runtime storage. TUI Status or diagnostic detail SHALL report reachable URLs, certificate validity, trust state when inspectable, the public CA certificate location, and an explicit recovery action. Project input MUST NOT select certificates, issuers, trust stores, redirects, or TLS policy.

#### Scenario: Default localhost route
- **WHEN** a project registers `app.localhost`
- **THEN** Cadder reports `http://app.localhost:8080` and `https://app.localhost:8443`

#### Scenario: Internal CA is not trusted
- **WHEN** HTTPS is configured but the operating system does not trust the Cadder CA
- **THEN** the daemon continues without elevation and diagnostic detail presents explicit remediation

#### Scenario: Project attempts to control TLS
- **WHEN** project configuration selects a certificate, issuer, trust action, redirect, or TLS policy
- **THEN** Cadder rejects the complete candidate
