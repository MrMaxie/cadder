## Context

Cadder is a local developer tool with three release processes and several
cross-cutting contracts. The daemon owns state and the real Caddy process. The
shim presents a narrow compatibility surface. The operator exposes the same
state through CLI and TUI workflows. Security, storage, observability, IIS,
distribution, and documentation affect more than one process.

The repository previously described these concerns in one broad change. Some
tasks were implemented, others remained planned, and no main specs existed. The
result preserved useful evidence but could not distinguish accepted 1.0
behavior from current delivery status.

The contract serves end users, operators, contributors, release maintainers,
and future local-client authors. It must remain stable enough for incremental
implementation while staying observable and testable.

## Goals and non-goals

### Goals

- Define one accepted Cadder 1.0 product contract in focused capabilities.
- Give every requirement a stable ID used by changes, tests, and evidence.
- Keep public behavior separate from crate layout and implementation progress.
- Make security, compatibility, failure, and recovery behavior explicit.
- Provide a deterministic path from target contract to verified release.

### Non-goals

- Selecting line-level implementation details for every requirement.
- Preserving pre-1.0 command, package, or storage compatibility when it conflicts
  with the accepted 1.0 contract.
- Expanding the 1.0 product beyond daemon, shim, CLI, and TUI.
- Marking any current implementation slice complete.

## Decisions

### Main specs describe the accepted 1.0 target

Main specs use present-tense normative language for the accepted release. They
do not encode implementation progress. An implementation change and its
`verification.md` record whether code satisfies a requirement.

This differs from treating main specs as a snapshot of current code. A current
snapshot would make every incremental implementation change rewrite the target
and would force public documentation to mix finished behavior with internal
status. The repository README states the chosen model so contributors do not
assume the default OpenSpec convention.

### Contract changes and implementation changes use different schemas

Contract changes use `spec-driven` and update main specs when archived.
Implementation changes use the project-local `implementation` schema, reference
accepted IDs, produce final evidence, and archive with `--skip-specs`.

This keeps requirement changes reviewable and prevents an implementation task
from silently changing accepted behavior. The custom schema remains small and
versioned because OpenSpec schema support is experimental.

### Capabilities follow product responsibilities

The contract uses twelve capabilities. Each capability owns one coherent set of
observable responsibilities and one identifier prefix. Crate names do not
define capability boundaries; multiple crates may implement one capability,
and one crate may support several capabilities.

The alternative was to preserve the seven broad capabilities from the archived
change. That grouping combined unrelated lifecycle, security, storage, operator,
and delivery concerns, which made requirements and implementation slices too
large to verify independently.

### Stable IDs survive editorial changes

Requirement headings use `PREFIX-NNN: Name`. The prefix belongs to the
capability, and the number is never reused. A wording or heading change keeps
the ID. Removing a requirement records its ID and migration in a contract
change; a new behavior receives a new ID.

Scenarios have descriptive names but no separate numeric identity. Tests and
verification reference the requirement ID plus the scenario name when needed.

### Specs define observable policy, not library selection

The specs define process ownership, inputs, outputs, security properties,
failure states, compatibility, and recovery. They name a protocol shape or
platform mechanism only when a user, client, or security boundary depends on
it. Crates, helper types, and internal module layouts belong in implementation
designs.

Tool choices such as cargo-dist and Astro are part of the contract only where
they determine supported artifacts or documentation behavior. Their internal
configuration remains implementation detail.

### Trust boundaries fail closed

The target contract follows five cross-capability invariants:

1. The normal daemon runs as the current user.
2. The daemon owns every real Caddy process and configuration it starts.
3. Project-controlled files cannot select executables or privileged operations.
4. Local transports authenticate or constrain the operating-system principal
   before dispatching a state-changing request.
5. Elevation is limited to an authenticated one-shot IIS plan.

These invariants appear in the capability where users observe them and are
cross-referenced rather than duplicated with different wording.

### Release readiness is evidence, not prose

The repository may build target documentation throughout implementation, but
deployment, version `1.0.0`, tags, and release artifacts remain blocked until
every required implementation change is archived and the final release change
records passing evidence.

User documentation describes only the accepted product and uses present tense.
OpenSpec changes carry delivery status; public pages do not expose workstation
paths, local operational files, or mock-only workflows.

## Verification strategy

- Strict OpenSpec validation checks normative structure and scenarios.
- The repository-specific checker enforces unique IDs, capability prefixes,
  requirement references, evidence coverage, and documentation boundaries.
- Each implementation change maps accepted IDs to focused automated tests and a
  final repository gate.
- Contract fixtures cover protocol and machine-output compatibility.
- Cross-platform integration suites cover OS-specific transport, installation,
  and IIS behavior without requiring those systems for ordinary unit tests.

## Risks and trade-offs

- [Target specs can appear ahead of code] -> Active implementation changes and
  verification are the only status source; publishing remains gated.
- [A broad initial contract can hide ambiguity] -> Capabilities and stable IDs
  stay small, and every requirement has observable scenarios.
- [Implementation discoveries may invalidate a requirement] -> Use a focused
  spec-driven change before changing code; never edit main specs implicitly.
- [Custom schema behavior may change upstream] -> Pin the OpenSpec version,
  validate the schema in CI, and keep its artifact model simple.
- [Detailed policy may constrain future clients] -> Specify behavior and wire
  compatibility, while leaving presentation and internal module choices open.

## Migration plan

1. Preserve the previous broad change in an archive without syncing its deltas.
2. Validate and archive this contract change to populate main specs.
3. Keep `spec-driven` as the OpenSpec 1.5 default, select `implementation`
   explicitly for implementation slices, and enforce both schemas through the
   repository gate. OpenSpec 1.5 otherwise treats the project-specific schema's
   spec-free changes as invalid deltas and ignores the configured `specs` rules.
4. Implement one dependency-ordered slice at a time, checking tasks only after
   their evidence passes.
5. Use a new spec-driven change for any accepted contract revision discovered
   during implementation.

Before production release, this contract can be rolled back by reverting its
archive and main specs together. After implementation depends on an ID, changes
use normal ADDED, MODIFIED, REMOVED, or RENAMED deltas so history stays intact.

## Open questions

None. Product scope, documentation tense, platform matrix, license, elevation
model, distribution baseline, and publication gate are accepted decisions.
