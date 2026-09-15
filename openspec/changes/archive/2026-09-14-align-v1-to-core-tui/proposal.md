## Why

Cadder's accepted 1.0 contract describes a broad CLI, multiple runtime profiles, streaming and historical workflows, and installer-managed surfaces that the current product does not expose. The current parser exposes only `cadder tui` (`crates/cadder-client/src/main.rs`), while the shim already starts a missing daemon for managed run (`crates/cadder-shim/src/main.rs`). This change preserves that working `caddy run` to daemon to TUI journey and makes it the complete public 1.0 surface before more infrastructure is built for unused promises.

## What Changes

- **BREAKING** Limit the public `cadder` command to help, version, and `cadder tui`; remove the unimplemented command, machine-output, profile, history, export, watch, autostart, doctor, and shim-setup contracts.
- Define Status, Domains, and Logs as the TUI's product views, including activation controls and explicit daemon start, stop, and restart actions.
- **BREAKING** Make the PATH-facing `caddy` shim auto-start the user-owned daemon when `caddy run` cannot attach, then register, renew, and detach the project through that daemon.
- **BREAKING** Reset the pre-1.0 local protocol to one typed request/response envelope with exact version matching and only the operations required by managed run, the TUI, and bounded shutdown.
- Establish one runtime per installation directory and one local client boundary; remove profile selection, legacy envelopes, capability negotiation, paging, subscriptions, CLI-equivalence models, and speculative reusable presentation models.
- Keep portable archives containing `cadder`, `cadderd`, and the PATH-facing `caddy` shim as the only distribution journey. Treat foreground `cadderd` as an operational and diagnostic entrypoint rather than a second product workflow.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `product-topology`: Reduce the accepted product to managed `caddy run`, the operator TUI, foreground diagnostics, and portable archives.
- `operator-cli`: Replace the planned command tree with help, version, and TUI launch only.
- `operator-tui`: Define the three supported views, activation controls, bounded logs, and daemon lifecycle actions without CLI equivalence.
- `project-registration`: Align managed run with daemon auto-start, one runtime, durable activation state, and no forget or historical tombstone workflow.
- `local-control-plane`: Replace the compatibility, capability, paging, and subscription layers with one exact-version typed request/response protocol.
- `daemon-lifecycle`: Define one user-owned runtime and TUI-driven start, stop, and restart while retaining bounded shutdown and child ownership.
- `caddy-runtime`: Remove profile-dependent runtime behavior while retaining trusted real-Caddy resolution, configuration validation, and owned-process safety.
- `distribution-and-upgrades`: Make the portable three-binary archive and checksum the only 1.0 release form.
- `documentation-experience`: Document only portable installation, managed run, TUI operation, foreground diagnostics, and manual archive lifecycle for their actual audiences.
- `observability`: Reduce operator observability to redacted bounded recent-log queries without history, tailing, subscriptions, exports, or pages.
- `runtime-storage`: Define only the durable product state required by the retained journey without selecting a storage format.

## Success Criteria

- The accepted public command surface contains only help, version, `cadder tui`, supported `caddy run`, and foreground `cadderd` operation.
- One bounded snapshot and one bounded recent-log response can represent every accepted runtime state without paging or subscriptions.
- One exact-version typed protocol contains only the eight operations required by managed run, the TUI, and shutdown.
- Public documentation contains no profile, autostart, history, export, tail, alias-setup, installer, mock-backend, hidden-flag, or machine-output journey.
- Implementation tasks name the removable CLI, IPC compatibility, paging, subscription, profile, autostart, history, and presentation-model concepts and pass the complete repository gate before closeout.

## Impact

Future implementation removes unexposed CLI DTOs and handlers, autostart and historical operator surfaces, legacy wire adapters, paging and subscription machinery, profile abstractions, format-specific storage promises, and duplicate presentation models. `cadder-api` becomes the sole local client boundary and the operator and shim stop depending on daemon implementation details. The SQLite engine, schema, cleanup, and corruption behavior are intentionally handled by the dependent `replace-custom-storage-with-sqlite` change; mutation-actor and repository-tooling simplifications remain separate follow-ups.
