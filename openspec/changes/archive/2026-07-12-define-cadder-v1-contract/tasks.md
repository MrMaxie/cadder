## 1. OpenSpec workflow foundation

- [x] 1.1 Preserve `reset-cadder-architecture` as a superseded archive without syncing its delta specs or changing its task history. (verification: archive exists, no active `reset-cadder-architecture` change remains, and main specs stay empty before this contract is archived)
- [x] 1.2 Correct repository artifact rules and prove that OpenSpec includes every configured proposal, design, specs, and tasks rule without warnings. (verification: resolved artifact instructions contain the configured string rules and doctor reports a healthy root)
- [x] 1.3 Document the Cadder target-contract model and add the project-local `implementation` schema with proposal, design, tasks, and verification artifacts. (verification: schema validation and artifact discovery pass under OpenSpec 1.5.0)
- [x] 1.4 Add a schema-aware `cargo xtask openspec-check` command that validates main specs, contract changes, implementation evidence, and local-data boundaries without new parser dependencies. (verification: focused xtask unit and CLI tests cover valid and invalid contracts, schema classification, evidence ordering, and OpenSpec version enforcement)

## 2. Cadder 1.0 contract

- [x] 2.1 Define product topology, daemon lifecycle, and project registration requirements with stable `TOP`, `RUN`, and `REG` identifiers. (verification: each capability passes strict structural validation and matches the accepted process boundaries)
- [x] 2.2 Define local control-plane, Caddy runtime, and durable storage requirements with stable `IPC`, `CAD`, and `STO` identifiers. (verification: each trust, compatibility, transaction, and recovery boundary has an observable scenario)
- [x] 2.3 Define CLI, TUI, observability, and IIS requirements with stable `CLI`, `TUI`, `OBS`, and `IIS` identifiers. (verification: commands, machine output, exit codes, state models, elevation, and equivalent operator workflows are internally consistent)
- [x] 2.4 Define distribution and documentation requirements with stable `DST` and `DOC` identifiers. (verification: platform artifacts, Apache-2.0 identity, immutable publication, target-state prose, audience boundaries, and accessibility gates are explicit)

## 3. Contract review

- [x] 3.1 Check all twelve capabilities for unique identifiers, correct prefixes, normative language, scenarios, and proposal-to-spec coverage. (verification: the structural contract audit reports no duplicate, missing, malformed, or dangling capability or requirement)
- [x] 3.2 Reconcile independent security, operator, delivery, and cross-capability reviews without adding implementation status to product requirements. (verification: every actionable P0/P1 finding is fixed or recorded as a resolved design decision)
- [x] 3.3 Check cross-capability public contracts for one command tree, one exit taxonomy, one platform matrix, one privilege model, and one publication gate. (verification: repeated behavior is identical or replaced by an explicit cross-reference)

## 4. Final validation

- [x] 4.1 Run repository whitespace and local-data checks over the OpenSpec foundation and contract artifacts. (verification: no malformed Markdown, workstation path, private operational detail, or unresolved template placeholder remains)
- [x] 4.2 Run the OpenSpec root, schema, and strict contract validation set. (verification: doctor, implementation-schema validation, and strict validation all pass without warnings)
- [x] 4.3 Review the archive operation as twelve new capabilities with no modification or removal of existing main requirements. (verification: the archive plan contains only the accepted ADDED requirements and is ready to populate main specs)
