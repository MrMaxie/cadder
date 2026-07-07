## Context

Cadder coordinates Caddy-backed local development runtimes. The reset defines a
target architecture before implementation continues, so each part has one clear
reason to exist and one clear owner.

The desired shape is a daemon-backed tool:

- the daemon owns runtime state and external process coordination;
- compatibility shims translate familiar workflows into daemon requests;
- operator clients inspect and control the daemon through a stable local
  protocol;
- optional platform integrations sit behind narrow providers.

OpenSpec captures the product contract. Code, documentation, and release
artifacts should follow the accepted specs rather than becoming competing
sources of truth.

## Goals / Non-Goals

**Goals:**

- Define a small runtime topology with explicit ownership.
- Keep runtime, integration, and presentation boundaries independently testable.
- Make daemon-offline behavior part of the normal operator contract.
- Keep platform-specific behavior isolated behind testable abstractions.
- Prefer mature libraries and tools over custom orchestration when they fit.
- Make future client surfaces reuse existing protocol and view-model contracts.

**Non-Goals:**

- This change does not implement the architecture.
- This change does not pick every crate/module name.
- This change does not make every future user interface part of the initial
  product surface.
- This change does not require Cadder to manage external processes it did not
  start.

## Decisions

### Runtime Ownership

The daemon is the only Cadder component that owns durable runtime state,
generated Caddy configuration, Caddy process coordination, logs, and platform
integration state. Other components are clients or adapters.

This keeps process ownership, recovery, and diagnostics in one place. It avoids
state drift between shims, operator clients, and real Caddy.

### Runtime Identity

Production mode uses one stable runtime identity per user or explicit system
installation. Multiple runtimes are a dev/debug feature and require isolated
runtime directories, visible labels, and separate locks.

This gives normal users predictable behavior while preserving a deliberate path
for debugging and integration tests.

### Local Control Plane

Cadder uses a local daemon protocol with typed envelopes, requests, responses,
events, and errors. The protocol should tolerate additive compatible changes and
report unsupported capabilities explicitly.

Transport is an edge concern. Core protocol types stay transport-neutral.
Platform-specific transport security belongs behind a narrow policy layer.

### Privilege Boundaries

Normal operation should run at user privilege. Operations that require higher
privilege must be explicit, scoped, and observable from user-level clients.

The implementation must document how user-level clients discover and contact a
more privileged daemon or helper. The security policy must be testable without
requiring real OS mutation in every test.

### Shim Boundary

The Caddy-compatible shim is a routing layer, not a second daemon and not a
full Caddy clone. It must classify commands as managed, read-only inspection,
explicit passthrough, or unsupported.

Managed commands go to the daemon. Passthrough is allowed only when it cannot
mutate Cadder-managed state silently. Unknown commands fail with useful
diagnostics.

### Caddy Adapter

Cadder owns the generated runtime model. Real Caddy is the execution target.
Caddy's Admin API and process commands are adapter mechanisms, not public
Cadder state.

The adapter must prevent recursive shim execution, apply generated config
atomically where possible, and report drift when real Caddy no longer matches
Cadder's last applied model.

### Platform Integrations

Platform integrations, including IIS handoff, live behind provider traits or
equivalent boundaries. The daemon depends on provider contracts, not directly on
platform command scripts or OS APIs.

This keeps policy decisions testable with fakes and reserves real OS smoke tests
for behavior that genuinely requires the platform.

### Operator Clients

The operator executable provides non-interactive CLI workflows and interactive
TUI workflows. Both render from daemon protocol data and shared view models.

Clients must remain useful when the daemon is offline. Read-only flows can show
setup information, diagnostics, and last-known state where available.
State-changing flows must explain that the daemon is unavailable and offer a
start/recovery path.

### Logs

The daemon owns structured log capture, query, retention, and redaction
semantics. Clients request logs by dimensions such as runtime, project, domain,
source, and severity; presentation differences must not change query semantics.

Per-domain logs must be real domain-filtered records, not a UI label over a
broader Caddy stream.

### Tooling

Custom tooling should remain small. Use mature tools for testing, coverage,
release, documentation, and command orchestration when they fit the project.
Custom Rust tooling is reserved for Cadder-specific checks that external tools
do not cover clearly.

## Risks / Trade-offs

- Local IPC security differs by platform -> keep transport policy explicit and
  test policy separately from OS smoke tests.
- Future client surfaces can drift -> require shared protocol and view-model
  contracts before adding them.
- Direct real-Caddy changes can create drift -> keep generated config ownership
  in the daemon and surface drift as state.
- Domain-level logs may require extra labeling or mapping -> define that mapping
  in daemon-owned log semantics.
- Tooling cleanup can break CI or releases -> replace custom steps only when an
  equivalent verification path exists.

## Migration Plan

1. Adopt OpenSpec as the source for requirements, design decisions, and tasks.
2. Reconcile workspace crates against the accepted topology.
3. Split protocol, daemon, shim, operator, platform, and log code into testable
   seams.
4. Rebuild behavior in independently verifiable slices.
5. Replace or shrink custom tooling after equivalent validation exists.
6. Update docs and release metadata after the runtime topology is implemented.

## Open Questions

- What is the exact CLI/TUI invocation model for the operator executable?
- What local IPC security model should be used for user-level clients talking to
  a privileged daemon or helper?
- Should privileged platform operations use a narrow helper executable or a
  privileged daemon mode?
- Which exact Caddy command paths belong in each shim policy category?
- What file-size/cohesion threshold should validation enforce, and which
  generated files are exempt?
- Should the existing operator support crate remain separate or become part of
  the final operator crate boundary?
