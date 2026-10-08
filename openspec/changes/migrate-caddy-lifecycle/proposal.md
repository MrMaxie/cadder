## Why

Preserve the project Caddyfile -> managed shim -> daemon -> operator journey.
The next capability runs and stops the daemon's own Caddy with independently
observed configuration, so the delivered transaction queue can leave and restore
idle. Existing resolution, validation, route composition and secure administration
are implemented; none currently owns a long-lived Caddy lifecycle.

## Requirement IDs

- `CADDY-001`: retain the pinned trusted executable.
- `CADDY-002`: one owned child serves composed routes.
- `CADDY-003`: independent readback and real idle transitions for the queue.
- `CADDY-006`: retain verified loopback mTLS for lifecycle administration.
- `RUN-003`: bounded graceful/forced teardown of owned work only.
- `RUN-004`: retain startup identity/configuration until restart.

## Scope

Implement a small Caddy lifecycle adapter over the existing pinned-image,
owned-command and HTTPS boundaries. Cover initial start, reload, independent
inspection, unexpected exit, idle rollback, bounded stop and admission close.
Extend existing owned execution only where long-lived execution needs it;
reuse its Windows Job/Unix group instead of another process framework.

## Non-goals

No registration handlers, SQLite application worker, shim, CLI/TUI, packaging,
auto-restart loop, process adoption/enumeration, trust installation, ACL repair,
new dependencies or runtime framework. No Rust removal or release action.
Native platform and full product acceptance remain separate gates.

## Success criteria

- Start and reload use the pinned image and immutable secure policy.
- Active hashes come from mTLS readback, never submitted bytes or receipts.
- Idle is reported only when absence/owned settlement is positively established;
  unexpected exit or failed observation cannot masquerade as a committed state.
- Graceful stop has a deadline and then forces only the owned Job/group.
- Startup failure, cancellation and close settle acquired work before releasing
  staging; unrelated processes survive fixture success and failure scenarios.
- Focused tests, Node checks and independent review provide executed evidence;
  unexecuted native/product gates stay open.

## Impact

Caddy lifecycle/admin modules, the existing owned-command seam and direct tests.
The internal ConfigurationRuntime is implemented without changing public RPC or
CaddyPort. Conceptually this removes the missing idle/lifecycle seam; it does not
replace the existing resolver, queue or native process ownership implementation.
