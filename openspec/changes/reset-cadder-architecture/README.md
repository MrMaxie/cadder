# Cadder 2.0 migration roadmap

Start with [acceptance.md](acceptance.md) for the approved-scope mapping and gate
order, then [tasks.md](tasks.md) for the current implementation task. This change
must replace the Rust product, not leave two permanent implementations. Functional
scope is the current released Rust 1.0.5 product in TypeScript, not superseded
history/IIS/autostart or machine-output workflows. Bare cadder retains help;
cadder tui retains the routes workspace and explicit --start-daemon option.

| Artifact | Responsibility |
| --- | --- |
| proposal.md | User journey, full replacement scope, affected capabilities |
| node-migration.md | Approved migration constraints and exclusions |
| design.md | Node module contracts, ownership, security, packaging and cutover |
| specs/ | Reviewed stable-ID deltas synchronized into main specs; retained provenance |
| tasks.md | Only Node implementation checklist, grouped by dependency gate |
| acceptance.md | Approved-plan coverage, evidence levels and final completion |
| release.md | Planned consumer outcomes and major release impact |
| verification.md | Executed evidence and remaining acceptance gaps |
| behavior-baseline.md | Current and pinned historical behavior provenance |
| windows-runtime-gate.md | Isolated real Windows privilege-gate procedure |

The original Rust reset is archived separately. Its earlier active task snapshot
is recoverable at `f850418:openspec/changes/reset-cadder-architecture/tasks.md`.
Historical Rust completions must not count as Node progress.

G1/G2 acceptance remains open. The user-approved sequence permits groups 3-6
implementation against delivered contract/runtime boundaries and schedules Windows
Sandbox for the completed product under 2.8/7.4. G1/G2 must still close before
G7 acceptance, Rust removal or release. The runtime foundation is not a shippable
2.0 application. Both npm
and every standalone SEA variant must pass before Rust/Cargo/legacy packaging
removal, followed by a complete Node-only recheck.

Follow the repository's Arcantry/OpenSpec contract -> reviewed delta sync ->
focused implementation slice -> evidence -> closeout workflow. Completing these
planning artifacts does not close implementation gates, update release
manifests/changelog, or authorize commit/push/tag/publication. The reviewed
spec-only synchronization is recorded separately in verification.md; this
roadmap remains active.
