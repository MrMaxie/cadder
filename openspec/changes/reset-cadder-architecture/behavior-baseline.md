# Behavior baseline and migration evidence

Rust task completion is not Node implementation evidence. The source paths and
test names in the historical table below refer to Rust snapshot `a10c63a`,
accessible with `git show a10c63a:<path>`, not necessarily the current checkout.
That snapshot is reference evidence only, not authority for functional scope.
The released Rust 1.0.5 workspace and current main specs define retained behavior,
including security fixes and release tooling. The superseded reset is archived
under `openspec/changes/archive/`; IIS, autostart, history and machine output
from older intent must not be restored by this migration.

## Current product authority

- `openspec/specs/operator-cli/spec.md` and
  `crates/cadder-client/src/cli/mod.rs`: current grammar, bare help, human output,
  explicit `tui --start-daemon` and removed-command rejection.
- `openspec/specs/operator-tui/spec.md`: routes-first tree table, no activity/log
  view, quiet daemon/Caddy header, explicit lifecycle and keyboard behavior.
- `openspec/specs/local-control-plane/spec.md`: eight operations, not an expanded
  history/platform RPC catalog.
- `openspec/specs/observability/spec.md`: bounded canonical-stream diagnostic logs,
  without history, export, subscriptions or new all-log/source/severity filters.
- `openspec/specs/product-topology/spec.md`: independent installation runtimes.

Task 1.3 must freeze fixtures from these current sources and the released Rust
implementation. Historical evidence below is usable only for retained behavior.

## Historical evidence (not current scope)

| Contract | Rust source/test evidence | Node evidence |
| --- | --- | --- |
| Runtime override and default/dev profile aliases | `crates/cadder-daemon/src/paths.rs`, `resolve_override_derives_stable_socket_and_runtime_paths`, `runtime_profile_parser_rejects_unknown_values` | `test/paths.test.ts`; v2 isolation is an intentional change |
| Exclusive ownership despite stale/misleading PID metadata | `crates/cadder-daemon/src/runtime_lock.rs`, `raw_lock_rejects_second_owner`, `runtime_lock_recovers_stale_owner_metadata` | `test/runtime-lock.test.ts`, `test/runtime-system.test.ts`; SQLite replaces fs4 |
| Same-owner account can contact elevated endpoint | `crates/cadder-daemon/src/ipc_security.rs`, `IpcSecurityPolicy::evaluate` | Owner ACL plus HMAC replaces native peer inspection; elevated system acceptance pending |
| Config file/env/CLI precedence | `crates/cadder-daemon/src/config.rs`, `crates/cadder-daemon/src/caddy.rs`, `RealCaddyResolver::selected_command` | Pending Node port |
| Managed shim starts missing daemon, never unmanaged fallback | `crates/cadder-shim/src/main.rs`, `run_managed_does_not_delegate_to_real_caddy_when_backend_is_missing`, `open_managed_run_target_starts_missing_daemon_when_fallback_is_skipped` | Pending Node port |
| Registration heartbeat and unregister on shutdown | `crates/cadder-shim/src/main.rs`, `run_managed_registers_heartbeats_and_unregisters_on_shutdown` | Pending Node port |
| Multiple-project composition and last-known-good | `crates/cadder-daemon/src/caddy.rs`, `crates/cadder-daemon/src/state/config_apply.rs` | Serial transaction and ambiguity reconciliation are new acceptance requirements; pending |
| Operator views use real daemon state | `crates/cadder-operator/src/`; snapshot `crates/cadder` TUI contains mocks | Node CLI/TUI must use the shared client service and current routes-only scope; pending |
| Historical IIS preview, handoff and restore (superseded) | `crates/cadder-daemon/src/iis.rs` and daemon IIS state modules | Excluded; not a Node parity task |

`test/fixtures/runtime-child.ts` is a real child-process fixture, not a production
daemon. Its status/shutdown handlers exist solely to prove runtime exclusion,
crash recovery and authenticated OS transport before porting product handlers.
