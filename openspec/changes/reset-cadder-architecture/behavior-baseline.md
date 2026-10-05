# Behavior baseline and migration evidence

Rust task completion is not Node implementation evidence. The source paths and
test names below refer to the historical Rust snapshot at `a10c63a`, accessible
with `git show a10c63a:<path>`, not necessarily the current checkout. Node tests
are listed separately. After integrating `master`, the released Rust 1.0.5
workspace is retained unchanged as the operational baseline, including its
security fixes and release tooling. Its superseding reset is archived under
`openspec/changes/archive/`; this active change retains the approved 2.0 scope,
including IIS, autostart, history and both distribution channels.

| Contract | Rust source/test evidence | Node evidence |
| --- | --- | --- |
| Runtime override and default/dev profile aliases | `crates/cadder-daemon/src/paths.rs`, `resolve_override_derives_stable_socket_and_runtime_paths`, `runtime_profile_parser_rejects_unknown_values` | `test/paths.test.ts`; v2 isolation is an intentional change |
| Exclusive ownership despite stale/misleading PID metadata | `crates/cadder-daemon/src/runtime_lock.rs`, `raw_lock_rejects_second_owner`, `runtime_lock_recovers_stale_owner_metadata` | `test/runtime-lock.test.ts`, `test/runtime-system.test.ts`; SQLite replaces fs4 |
| Same-owner account can contact elevated endpoint | `crates/cadder-daemon/src/ipc_security.rs`, `IpcSecurityPolicy::evaluate` | Owner ACL plus HMAC replaces native peer inspection; elevated system acceptance pending |
| Config file/env/CLI precedence | `crates/cadder-daemon/src/config.rs`, `crates/cadder-daemon/src/caddy.rs`, `RealCaddyResolver::selected_command` | Pending Node port |
| Managed shim starts missing daemon, never unmanaged fallback | `crates/cadder-shim/src/main.rs`, `run_managed_does_not_delegate_to_real_caddy_when_backend_is_missing`, `open_managed_run_target_starts_missing_daemon_when_fallback_is_skipped` | Pending Node port |
| Registration heartbeat and unregister on shutdown | `crates/cadder-shim/src/main.rs`, `run_managed_registers_heartbeats_and_unregisters_on_shutdown` | Pending Node port |
| Multiple-project composition and last-known-good | `crates/cadder-daemon/src/caddy.rs`, `crates/cadder-daemon/src/state/config_apply.rs` | Serial transaction and ambiguity reconciliation are new acceptance requirements; pending |
| Operator views use real daemon state | `crates/cadder-operator/src/`; current `crates/cadder` TUI contains mocks | Node CLI/TUI must use the shared client service; pending |
| IIS preview, explicit handoff and restore | `crates/cadder-daemon/src/iis.rs` and daemon IIS state modules | Pending Node provider and Sandbox acceptance |

`test/fixtures/runtime-child.ts` is a real child-process fixture, not a production
daemon. Its status/shutdown handlers exist solely to prove runtime exclusion,
crash recovery and authenticated OS transport before porting product handlers.
