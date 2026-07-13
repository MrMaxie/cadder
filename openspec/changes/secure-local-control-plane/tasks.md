## 1. Protocol core

- [x] 1.1 `[IPC-006]` `[IPC-007]` `[IPC-010]` Add validated protocol-version, capability, request-ID, handshake, envelope, and typed-error contracts (verification: `cargo test -p cadder-protocol protocol`)
- [x] 1.2 `[IPC-006]` `[IPC-010]` Add the central operation registry and reject unsupported versions or capabilities before payload decoding (verification: `cargo test -p cadder-daemon operation_registry`)
- [x] 1.3 `[IPC-010]` Add compatibility fixtures for additive responses, gated mutation fields and variants, unknown discriminators, and release-failing wire drift (verification: `cargo test -p cadder-protocol wire_compatibility`)
- [x] 1.4 `[IPC-007]` Preserve typed daemon, discovery, transport, and timeout errors through every client API (verification: `cargo test --workspace typed_error`)

## 2. Authenticated discovery and transport

- [x] 2.1 `[IPC-001]` Authenticate the OS peer immediately after accept and before protocol-frame buffering through one connection-owning, testable platform identity boundary (verification: `cargo test -p cadder-daemon peer_identity`)
- [ ] 2.2 `[IPC-001]` Enforce Unix filesystem socket ownership, `0700/0600` modes, and effective-peer UID checks (verification: Linux and macOS jobs run `cargo test -p cadder-daemon unix_ipc_security`)
- [x] 2.3 `[IPC-001]` Enforce a local-only Windows named pipe, owner SID DACL, fixed transport-authentication preface, impersonated client SID, verified revert paths, and fail-stop behavior when impersonation cannot be reverted (verification: Windows job runs `cargo test -p cadder-daemon windows_ipc_security`)
- [ ] 2.4 `[IPC-003]` Implement crash-safe atomic discovery publication and generation-aware cleanup for Unix and Windows algorithms (verification: Windows, Linux, and macOS jobs run `cargo test -p cadder-daemon discovery_publication`)
- [x] 2.5 `[IPC-003]` Require discovery validation and an instance-matching handshake for readiness and all new sessions (verification: `cargo test -p cadder-daemon discovery_handshake`)

## 3. Bounded IPC and shutdown

- [x] 3.1 `[IPC-004]` Replace every unbounded NDJSON reader and writer with the shared 1 MiB bounded codec (verification: `cargo test -p cadder-daemon ipc_codec`)
- [x] 3.2 `[IPC-004]` `[IPC-005]` Enforce the shared accept-to-first-frame deadline, one active request, pipelining rejection, the 64-connection cap, and operation-specific deadlines (verification: `cargo test -p cadder-daemon ipc_limits`)
- [x] 3.3 `[IPC-005]` Bound stream records and bytes, emit heartbeats and gap outcomes, and close stalled writers (verification: `cargo test -p cadder-daemon stream_limits`)
- [x] 3.4 `[RUN-005]` `[IPC-005]` Add cancellation/task ownership, lifecycle epochs, and per-operation commit permits that reject late mutation without invalidating unrelated work (verification: `cargo test -p cadder-daemon operation_fence`)
- [x] 3.5 `[RUN-005]` `[IPC-009]` Implement the shared accept, request/stream, and owned-runtime shutdown phases and their fixed budgets (verification: `cargo test -p cadder-daemon shutdown_coordinator`)
- [x] 3.6 `[RUN-005]` `[IPC-009]` Bound interruptible storage flush and join, retain ownership while an in-progress non-interruptible durability or rollback operation finishes under fail-stop containment, and perform generation-matched discovery and lock cleanup (verification: `cargo test -p cadder-daemon shutdown_storage`)

## 4. Trusted Caddy and ownership containment

- [x] 4.1 `[CAD-001]` Move real-Caddy selection to explicit daemon override, trusted per-user/system selectors, and safe PATH; validate containing-directory ownership/write permissions; remove project, environment, executable-adjacent, and shim selectors (verification: `cargo test -p cadder-daemon trusted_caddy_source`)
- [ ] 4.2 `[CAD-001]` Pin and reverify Caddy handle identity, digest, semantic version, modules, and probe revision at every spawn seam within the runtime-owner threat boundary (verification: `cargo test -p cadder-daemon pinned_caddy_image`)
- [ ] 4.3 `[RUN-004]` `[RUN-009]` Implement the authenticated runtime-guard protocol, generation lock, containment record, and replacement proof (verification: `cargo test -p cadderd --test cadderd_binary runtime_guard`)
- [ ] 4.4 `[RUN-004]` `[RUN-009]` Prove forced owner loss terminates only the owned child and grandchild within ten seconds on each supported OS family (verification: Windows, Linux, and macOS jobs run `cargo test -p cadderd --test cadderd_binary containment`)

## 5. Safe shim and registration lease

- [ ] 5.1 `[REG-005]` `[REG-006]` Close the shim command table, remove implicit daemon start and executable overrides, and preserve delegated read-only stdout and exit status (verification: `cargo test -p cadder-shim command_policy`)
- [ ] 5.2 `[REG-005]` `[REG-006]` Validate managed-run arguments, canonical project boundaries, regular config files, adapter choice, and symlink or junction escape (verification: `cargo test -p cadder-shim managed_run_input`)
- [ ] 5.3 `[REG-002]` Derive the stable entrypoint key, persist one public registration ID, and enforce principal, daemon-instance, and shim-session ownership for live leases (verification: `cargo test -p cadder-daemon registration_identity`)
- [ ] 5.4 `[REG-002]` `[REG-003]` Implement the five-second heartbeat, 30-second reconnect window, fresh cross-instance lease, bounded backoff, joined supervisor, and distinct clean-exit path (verification: `cargo test -p cadder-shim lease_supervisor`)
- [ ] 5.5 `[REG-003]` Preserve routes during reconnect and expire them through the existing configuration transaction without transferring ownership (verification: `cargo test -p cadder-daemon --test ipc_lifecycle lease_expiry`)

## 6. Client migration and terminal experience

- [ ] 6.1 `[IPC-003]` `[IPC-004]` `[IPC-006]` `[IPC-007]` Migrate daemon readiness and the reusable client/session foundation to discovery, handshake, bounded codec, and typed errors (verification: `cargo test -p cadder-daemon ipc_client`)
- [ ] 6.2 `[REG-002]` `[REG-003]` `[REG-005]` `[IPC-003]` `[IPC-007]` Migrate the shim session and reconnect state machine to the shared client without changing delegated output (verification: `cargo test -p cadder-shim --test shim_binary ipc`)
- [ ] 6.3 `[IPC-003]` `[IPC-004]` `[IPC-006]` `[IPC-007]` Migrate operator requests and state subscriptions to the shared client foundation (verification: `cargo test -p cadder-operator ipc`)
- [ ] 6.4 `[REG-002]` `[REG-003]` `[REG-005]` `[IPC-007]` Add terminal golden and pseudo-terminal tests for human unavailable, reconnect, restored, conflict, timeout, shutdown and Ctrl+C outcomes, exact delegated stdout/exit status, the stderr delegation notice, no ANSI, non-TTY, and `TERM=dumb` (verification: `cargo test --workspace terminal_ux`)
- [ ] 6.5 `[REG-005]` `[REG-006]` `[IPC-003]` Update architecture, real-Caddy selection, shim recovery, protocol, and security documentation for their intended audiences (verification: `cargo xtask docs check`)

## 7. Final verification

- [ ] 7.1 `[RUN-004]` `[RUN-005]` `[RUN-009]` `[REG-002]` `[REG-003]` `[REG-005]` `[REG-006]` `[IPC-001]` `[IPC-003]` `[IPC-004]` `[IPC-005]` `[IPC-006]` `[IPC-007]` `[IPC-009]` `[IPC-010]` `[CAD-001]` Run the complete local slice gate without a waiver (verification: `cargo xtask check`)
- [ ] 7.2 `[RUN-004]` `[RUN-009]` `[IPC-001]` `[IPC-003]` Complete the OS evidence matrix before recording `verification.md` (verification: Windows x64, Linux x64, macOS x64, and macOS arm64 jobs each pass the full gate plus their platform security and containment tests)
