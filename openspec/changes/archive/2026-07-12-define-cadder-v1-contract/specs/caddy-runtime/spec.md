## ADDED Requirements

### Requirement: CAD-001: Real Caddy comes only from trusted sources
The daemon SHALL resolve one real-Caddy installation, in order, from an explicit daemon-start override, trusted per-user configuration, trusted system configuration, or a safe PATH search. It MUST use a canonical executable path without a shell, exclude the active shim by file identity, and pin the resolved path, version, modules, and file identity for the daemon lifetime. Project files, registration working directories, executable-adjacent files, and shim-process overrides MUST NOT participate in resolution.

#### Scenario: Explicit trusted override
- **WHEN** the runtime owner supplies an absolute real-Caddy path through the daemon-start interface
- **THEN** the daemon validates the executable and its containing path against owner or administrator permissions before selecting it

#### Scenario: Project attempts to select a command
- **WHEN** a project-local file, working directory, or registration argument names another Caddy command
- **THEN** the daemon rejects or ignores that selector as project input
- **AND** it executes no candidate from the project-controlled source

#### Scenario: PATH resolves to the shim
- **WHEN** a PATH candidate is the active shim, a link to it, or the same file under another name
- **THEN** the resolver excludes that candidate and continues the safe search

#### Scenario: Resolved executable changes
- **WHEN** the selected path's file identity changes before a later spawn or validation
- **THEN** the daemon refuses to execute it and reports that the trusted Caddy installation changed

### Requirement: CAD-002: Caddy compatibility is verified
Cadder 1.0 SHALL consider a real Caddy build eligible only when its semantic version is at least 2.11.3 and lower than 3.0.0, it contains every required standard module, and it passes the deterministic compatibility probe shipped with that exact Cadder release. The probe MUST verify adaptation of embedded fixtures covering every allowed Caddyfile directive, the closed adapted JSON schema, private Admin API authentication, transactional `/load`, `/config/` inspection, and bounded `/stop` against a disposable loopback-only runtime before any project traffic is served. A 2.x version alone MUST NOT establish compatibility.

Each Cadder release manifest SHALL record the exact tested minimum and current-stable Caddy versions, required module IDs, and compatibility-probe revision. The daemon MUST reject an out-of-range version, missing module, failed probe, or unverifiable build with actionable diagnostics. Probe execution MUST use Cadder-owned fixtures and isolated runtime state rather than project input.

#### Scenario: Minimum supported version
- **WHEN** Cadder starts against the tested minimum Caddy 2.11.3 build with all required standard modules
- **THEN** compatibility validation succeeds

#### Scenario: Unsupported or incomplete build
- **WHEN** the selected executable is outside the accepted range, lacks a required standard module, or fails any compatibility probe
- **THEN** startup fails before the executable serves project traffic
- **AND** the diagnostic reports the observed version, missing module, or failed probe stage

#### Scenario: Release compatibility matrix
- **WHEN** Cadder is prepared for release
- **THEN** the integration suite passes against Caddy 2.11.3 and the exact current-stable version recorded in the release manifest on every supported operating-system family
- **AND** both builds pass the same shipped compatibility probe used at runtime

### Requirement: CAD-003: Project Caddyfiles use a closed subset
Cadder SHALL accept project configuration only through the Caddyfile adapter and MUST require every site and top-level matcher alternative to contain one exact, non-wildcard host. In source Caddyfiles, Cadder SHALL support only the `path`, `method`, `header`, and `query` named matchers; the `reverse_proxy`, `respond`, `header`, `rewrite`, `encode`, and `file_server` directives; and `route` or `handle` solely as nested grouping directives. After Caddy adapts that source, Cadder MUST accept only the corresponding `path`, `method`, `header`, and `query` matcher objects and `reverse_proxy`, `static_response`, `headers`, `rewrite`, `encode`, `file_server`, and `subroute` handler objects. Nested groupings MUST remain within the proven host scope and use the same closed subset.

Cadder MUST decode each adapted matcher and handler through a Cadder-owned typed allowlist, reject every unknown field or module selector, and re-emit only validated fields. `encode` SHALL allow only the standard `gzip` and `zstd` encoders. `file_server` SHALL allow a validated project root without browse mode. `reverse_proxy` SHALL use only validated local upstreams and the standard HTTP transport with certificate verification enabled. Other source directives or adapted options MUST fail as unsupported configuration.

The 1.0 allowlist SHALL contain only these semantics:

| Caddyfile form → adapted component | Allowed options |
| --- | --- |
| `path` → `path` matcher | Absolute URI path literals and glob patterns; no placeholders or regular expressions. |
| `method` → `method` matcher | One or more valid uppercase HTTP method tokens. |
| `header` or `query` matcher → same matcher | Field names with literal UTF-8 values or presence tests; no placeholders, regular expressions, or credential-bearing values. |
| `reverse_proxy` → `reverse_proxy` | One or more upstreams allowed by `CAD-004`, optional verified upstream TLS, and an optional literal TLS server name; no header mutation, health-check requests, dynamic discovery, retries beyond Cadder defaults, or custom transport fields. |
| `respond` → `static_response` | A status from 100 through 599, a UTF-8 body of at most 64 KiB, and static non-sensitive response headers. |
| `header` → `headers` | Static set, add, or delete operations for non-sensitive request or response headers; no placeholders, regular-expression replacement, deferred mutation, or `Authorization`, `Proxy-Authorization`, `Cookie`, or `Set-Cookie`. |
| `rewrite` → `rewrite` | One literal absolute URI path and optional literal query string; no placeholders or regular expressions. |
| `encode` → `encode` | `gzip` or `zstd` without custom module fields. |
| `file_server` → `file_server` | One canonical root within the project and literal index basenames without separators, `.` or `..`; every joined canonical path must remain under the root. No browse, pass-through, precompressed variants, hide rules, or custom filesystem module. |
| `route` or `handle` → `subroute` | Nested routes that satisfy this same table and retain the parent's exact host scope. |

#### Scenario: Supported project route
- **WHEN** a Caddyfile defines an exact host and uses only supported source matchers, directives, and grouping forms
- **THEN** Cadder adapts it and validates every resulting matcher and handler object as a typed project-route candidate for that host

#### Scenario: Catch-all or wildcard host
- **WHEN** any top-level matcher alternative omits a host or uses a wildcard host
- **THEN** Cadder rejects the complete candidate and identifies the offending route

#### Scenario: Configuration escapes the subset
- **WHEN** a project requests another app, adapter, handler, matcher, custom module, global option, import, listener, admin setting, TLS policy, storage setting, or logging setting
- **THEN** Cadder rejects the complete candidate with a specific unsupported-feature diagnostic
- **AND** the active configuration remains unchanged

#### Scenario: Adapted handler contains an unknown option
- **WHEN** an adapted handler contains a field, transport, encoder, plugin selector, or nested option outside its typed allowlist
- **THEN** Cadder rejects the complete candidate before validation or application
- **AND** it identifies the unsupported field path

#### Scenario: Header option contains credentials
- **WHEN** a matcher or handler supplies an authorization, proxy-authorization, cookie, or set-cookie value
- **THEN** Cadder rejects the candidate instead of persisting the credential in effective or last-known-good configuration

### Requirement: CAD-004: Project handlers cannot widen resource access
`reverse_proxy` upstreams MUST use a numeric IPv4 or IPv6 loopback literal. On Linux and macOS, Cadder MAY also accept a Unix-domain filesystem socket whose canonical path remains within the canonical project root, whose file type is a socket, and whose owner and containing-directory permissions exclude access or replacement by other users. Windows named pipes and filesystem-socket upstreams are outside 1.0. If the platform cannot inspect the socket type, canonical path, owner, or permissions, Cadder MUST reject that upstream. Hostname upstreams, including `localhost`, MUST be rejected so DNS cannot change the validated destination. Upstream TLS MUST retain certificate verification. A `file_server` root MUST resolve within the canonical project root. Projects MUST NOT enable directory browsing, insecure TLS verification, arbitrary transports, or upstreams that reach non-loopback networks.

#### Scenario: Local development upstream
- **WHEN** a route proxies to a numeric loopback TCP endpoint or, on Linux or macOS, a fully verified owner-protected Unix-domain socket within the project root
- **THEN** Cadder accepts the upstream after canonical validation

#### Scenario: Socket ownership cannot be proven
- **WHEN** a socket upstream is requested on Windows or its type, canonical path, owner, or containing-directory permissions cannot be verified
- **THEN** Cadder rejects the upstream before applying configuration
- **AND** the diagnostic recommends a numeric loopback endpoint

#### Scenario: Hostname upstream
- **WHEN** a project supplies a hostname such as `localhost` as a reverse-proxy upstream
- **THEN** Cadder rejects it and requests a numeric loopback literal or validated local socket

#### Scenario: Network or TLS escape
- **WHEN** a project targets a non-loopback address, requests an arbitrary transport, or disables upstream certificate verification
- **THEN** Cadder rejects the candidate before applying it

#### Scenario: File root escapes the project
- **WHEN** a file-server root resolves outside the canonical project root
- **THEN** Cadder rejects the candidate and reports the resolved boundary violation

### Requirement: CAD-005: Adaptation is bounded and secret-isolated
Cadder MUST limit a source Caddyfile to 131,072 bytes, adapted output to 8 MiB, and each adapt or validate process to 30 seconds. The daemon SHALL run these processes with a minimal documented environment that excludes daemon secrets and MUST reject environment placeholders or file imports that could read undeclared process or filesystem data. The complete registration envelope, including worst-case JSON escaping, MUST remain below the `IPC-004` frame limit.

#### Scenario: Source reaches its limit
- **WHEN** a valid 131,072-byte Caddyfile is encoded in a registration envelope
- **THEN** the complete NDJSON frame remains within the control-plane limit
- **AND** the daemon can validate the candidate normally

#### Scenario: Source uses an environment placeholder
- **WHEN** a project Caddyfile references a daemon-process environment variable
- **THEN** Cadder rejects the placeholder without revealing whether the variable exists or logging its value

#### Scenario: Adapter output exceeds its limit
- **WHEN** adaptation emits more than 8 MiB across its bounded result channels
- **THEN** Cadder terminates the owned adapter process and returns a configuration-too-large error

#### Scenario: Adapter times out
- **WHEN** adaptation or validation has no terminal result within 30 seconds
- **THEN** Cadder terminates and joins the owned process
- **AND** the last effective and last-known-good configurations remain unchanged

### Requirement: CAD-006: Cadder owns the composed runtime
The daemon SHALL compose validated project routes with Cadder-owned listeners, TLS, administration, storage, and logging. Default HTTP and HTTPS listeners SHALL bind loopback addresses on unprivileged ports 8080 and 8443. Cadder SHALL allow trusted profile configuration to select different fixed loopback endpoints, including ports 80 and 443 when the operating system already permits the current user to bind them; project inputs MUST NOT change or widen listeners. If the operating system denies a selected listener or another profile owns it, startup MUST fail with provisioning guidance instead of elevating, binding a wildcard address, or choosing an undocumented fallback port.

#### Scenario: Project route is composed
- **WHEN** a validated route enters the effective candidate
- **THEN** Cadder emits it only under its exact canonical host on Cadder-owned loopback listeners

#### Scenario: Default user-level listeners
- **WHEN** a profile has no trusted listener override
- **THEN** Cadder serves HTTP on loopback port 8080 and HTTPS on loopback port 8443
- **AND** snapshots and operator output include the port in each reachable URL

#### Scenario: Privileged port is unavailable
- **WHEN** trusted profile configuration selects port 80 or 443 and the current user cannot bind it
- **THEN** runtime startup fails as a listener precondition
- **AND** neither the daemon nor Caddy requests elevation

#### Scenario: Another profile owns the listener
- **WHEN** a second profile selects an HTTP or HTTPS endpoint already owned by a live Cadder profile
- **THEN** the second profile fails with a typed listener conflict
- **AND** it leaves the first profile and its Caddy child unchanged

#### Scenario: Project supplies no routes
- **WHEN** no accepted registration or durable Cadder-owned route exists
- **THEN** Cadder composes an explicit empty runtime configuration without a catch-all placeholder

### Requirement: CAD-007: Caddy administration is private and least-privileged
On Linux and macOS, Cadder SHALL administer its Caddy child through a per-runtime Unix socket owned by the runtime user with mode `0600`. On Windows, Cadder SHALL use a unique loopback endpoint with per-runtime mutual TLS, exact client-certificate pinning, owner-only key material, and no local plaintext Admin API. Admin mTLS credentials MUST remain distinct from the site-serving PKI defined by `CAD-012`. The administration policy MUST authorize only `POST /load`, `GET /config/`, and `POST /stop`; `localhost:2019` and broad configuration mutation MUST remain disabled.

#### Scenario: Unix administration
- **WHEN** the daemon starts Caddy on Linux or macOS
- **THEN** Caddy exposes its local Admin API only through the owner-permissioned runtime socket

#### Scenario: Windows administration
- **WHEN** the daemon starts Caddy on Windows
- **THEN** the configured Admin API requires the pinned Cadder client certificate and a valid server certificate on its unique loopback endpoint
- **AND** unauthenticated, wrong-certificate, wrong-method, and wrong-path requests fail

#### Scenario: Proxy environment is present
- **WHEN** the daemon process has HTTP proxy environment variables
- **THEN** Admin API requests still connect directly to the verified private endpoint

#### Scenario: Caddy persists configuration
- **WHEN** Caddy accepts a configuration
- **THEN** Caddy's own autosave persistence remains disabled and Cadder's last-known-good generation remains the durable authority

### Requirement: CAD-008: Configuration application is transactional
For every candidate, the daemon SHALL adapt and validate with the pinned real-Caddy installation, submit the final Cadder-owned JSON to `POST /load`, read `GET /config/` to verify the active canonical hash, and only then atomically publish matching effective and last-known-good generations. A failed stage MUST leave both the previously active Caddy configuration and the previously published files unchanged.

#### Scenario: Candidate succeeds
- **WHEN** validation and `POST /load` succeed and the active hash matches the candidate
- **THEN** Cadder atomically publishes the new effective and last-known-good generation
- **AND** registration state reports the generation as active

#### Scenario: Caddy rejects the load
- **WHEN** `POST /load` rejects a candidate
- **THEN** Caddy continues serving its prior configuration
- **AND** Cadder retains the previous effective and last-known-good generation

#### Scenario: Persistence fails after load
- **WHEN** Caddy loads the candidate but Cadder cannot publish its durable generation
- **THEN** Cadder restores the previous last-known-good configuration through the private API
- **AND** it enters degraded read-only state if that restoration cannot be verified

### Requirement: CAD-009: Recovery never resurrects stale project ownership
The daemon SHALL use the last-known-good configuration to recover its owned Caddy child only from current accepted runtime state. A daemon restart MUST rebuild project routes from live registrations and durable desired state bound to a valid `EntrypointKey`; it MUST NOT serve a route until the entrypoint establishes a new instance-bound lease under `REG-009`.

#### Scenario: Caddy child exits with live registrations
- **WHEN** the owned Caddy child exits while accepted registrations remain current
- **THEN** the daemon restarts the pinned executable and restores the verified last-known-good generation for that same current state

#### Scenario: Daemon restarts without registrations
- **WHEN** a new daemon instance starts before project shims reconnect
- **THEN** it marks durable entrypoints as reconnecting and excludes their project routes
- **AND** each reconnecting shim must establish a new lease before an enabled route becomes active

#### Scenario: Last-known-good data is invalid
- **WHEN** recovery cannot authenticate, parse, or verify the last-known-good generation
- **THEN** the daemon preserves it for diagnostics and enters degraded read-only state without loading it

### Requirement: CAD-010: Active configuration drift is repaired once
The daemon SHALL compare a canonical hash of `GET /config/` with the active Cadder generation at startup, after every apply, after child recovery, and during periodic health checks. On a mismatch, it MUST make one restoration attempt for that drift event. A failed or immediately recurring restoration MUST place the Caddy subsystem in degraded read-only state.

#### Scenario: Drift restoration succeeds
- **WHEN** the active Caddy hash differs and one last-known-good reload restores the expected hash
- **THEN** the daemon records the drift and recovery outcome
- **AND** normal mutations resume

#### Scenario: Drift restoration fails
- **WHEN** the restoration request fails or the verified hash still differs
- **THEN** the daemon stops automatic restore attempts for that event
- **AND** status, doctor, logs, history, and export remain available while configuration mutations are rejected

#### Scenario: Equivalent JSON ordering
- **WHEN** active and expected configurations differ only in object-key ordering
- **THEN** canonical hashing treats them as equal and does not report drift

### Requirement: CAD-011: Caddy lifecycle targets only the owned child
In accordance with `RUN-004`, the daemon MUST start, inspect, reload, and stop only the pinned Caddy child associated with its runtime generation. It SHALL use the private Admin API for configuration and graceful stop, then target only the recorded child handle if bounded termination escalation is required. It MUST NOT invoke a PATH-resolved `caddy reload` or `caddy stop`, enumerate system Caddy processes, or terminate a process based only on its name or PID.

#### Scenario: Graceful stop succeeds
- **WHEN** the owned child accepts `POST /stop` and exits within the shutdown deadline
- **THEN** Cadder joins that child and reports the runtime stopped

#### Scenario: Graceful stop times out
- **WHEN** the owned child does not exit after the private stop request
- **THEN** Cadder escalates only through the retained operating-system handle for that child

#### Scenario: Recorded identity is ambiguous
- **WHEN** Cadder cannot prove that a running process is the child it started
- **THEN** it reports lost ownership and leaves the process untouched

### Requirement: CAD-012: Local host and TLS behavior is explicit
Cadder SHALL serve exact `.localhost` hosts on the selected loopback HTTP and HTTPS listeners. It MAY accept another exact host only when that host resolves exclusively to the selected loopback addresses during registration; doctor MUST report later resolution drift. HTTP SHALL remain available on its configured port, and HTTPS SHALL use a Cadder-owned internal Caddy PKI on its configured port. Cadder MUST NOT redirect HTTP to HTTPS by default, install a trust root silently, or elevate to change operating-system trust.

The daemon SHALL keep internal CA keys in owner-only runtime storage. `cadder doctor` MUST report the HTTP and HTTPS URLs, certificate validity, trust status when the platform exposes it, the public CA certificate location, and platform-appropriate explicit trust guidance. Project input MUST NOT select certificates, issuers, trust stores, redirects, or TLS policy.

#### Scenario: Default localhost route
- **WHEN** a project registers `app.localhost` with default profile listeners
- **THEN** Cadder reports `http://app.localhost:8080` and `https://app.localhost:8443`
- **AND** both listeners remain bound only to loopback addresses

#### Scenario: Internal CA is not trusted
- **WHEN** HTTPS is configured but the operating system does not trust the Cadder internal CA
- **THEN** the daemon continues without elevation
- **AND** doctor reports the untrusted state, public CA certificate path, and explicit remediation

#### Scenario: External host resolves away from loopback
- **WHEN** an exact non-`.localhost` host resolves to any non-loopback address during registration
- **THEN** Cadder rejects the candidate before applying it

#### Scenario: Project attempts to control TLS
- **WHEN** project configuration selects a certificate, issuer, trust action, HTTP redirect, or TLS policy
- **THEN** Cadder rejects the complete candidate as unsupported project configuration
