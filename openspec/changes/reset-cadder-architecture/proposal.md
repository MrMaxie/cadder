## Why

Cadder needs a smaller and more explicit target architecture before more code is
added. The current direction must make ownership obvious: one runtime service
owns external state, while compatibility shims and operator clients attach to it
through stable local contracts.

The target should be familiar to users of daemon-backed developer tools. A
durable daemon manages the runtime. Thin clients inspect it, configure it, and
recover from offline states without becoming independent sources of truth.

## What Changes

- Define the product topology around:
  - a daemon that owns runtime state and external process coordination;
  - a Caddy-compatible shim that routes managed Caddy workflows into the daemon;
  - an operator executable that exposes CLI and TUI workflows.
- Define the local control plane, runtime identity, privilege boundaries, and
  daemon-unavailable behavior as product contracts.
- Define Caddy integration as an adapter boundary: Cadder owns the generated
  runtime model, and real Caddy is the execution target.
- Define optional platform integrations behind narrow, testable providers.
- Treat logs as a first-class operator workflow across runtime, project, domain,
  source, and severity dimensions.
- Require future operator surfaces to reuse the same daemon protocol and view
  models instead of creating another state model.
- Replace ad hoc planning with OpenSpec requirements, design notes, and
  implementation tasks.
- Reconcile workspace crates and tooling around small, independently testable
  modules and mature external tools where they fit.

## Capabilities

### New Capabilities

- `runtime-topology`: product surfaces, package boundaries, runtime ownership,
  future-surface constraints, and production/dev runtime identity.
- `daemon-control-plane`: daemon lifecycle, local IPC, state ownership,
  protocol compatibility, privilege boundaries, and offline handling.
- `caddy-shim-integration`: shim command policy, registration lifecycle, config
  composition, real Caddy resolution, fallback policy, and drift detection.
- `windows-iis-handoff`: optional IIS discovery, handoff, restore, privilege
  boundaries, provider abstraction, and system smoke coverage.
- `observability-logs`: structured log capture, retention, redaction, and query
  semantics by runtime, project, domain, source, and severity.
- `operator-clients`: CLI/TUI contracts, offline UX, daemon start/setup flows,
  reusable view models, and future client boundaries.
- `quality-tooling`: project workflow, code organization, dependency selection,
  test strategy, coverage, CI, release, and documentation expectations.

### Modified Capabilities

- None. This repository has no existing OpenSpec main specs yet.

## Impact

- Planning:
  - `openspec/` becomes the canonical place for requirements, design decisions,
    and implementation plans.
- Runtime architecture:
  - The daemon owns Cadder-managed runtime state.
  - Shims and operator clients are protocol clients.
  - Platform-specific behavior is explicit and isolated behind provider seams.
- Rust workspace:
  - Product crates, shared protocol/API code, test support, and tooling must be
    classified and simplified against the target topology.
- Testing:
  - Daemon logic, shim policy, protocol contracts, operator rendering, platform
    providers, log filtering, and system smoke behavior must be independently
    testable.
- Tooling:
  - Large custom orchestration should shrink or move to mature tools when those
    tools cover the job clearly.
