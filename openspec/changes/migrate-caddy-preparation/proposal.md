## Why

Preserve the released project Caddyfile -> managed shim -> daemon -> operator
journey. The next executable capability takes an existing project configuration,
selects trusted real Caddy, adapts it to JSON and validates a candidate without
starting a server or changing active routes. Caddy remains separately installed
and mandatory; this migrates Cadder's integration, not Caddy itself.

The user approved deferring Windows Sandbox to completed-product acceptance.
Open G1/G2 evidence remains mandatory before G7, Rust removal or release.

## Requirement IDs

- `REG-004`: Preserve existing cadder.toml/Caddyfile formats and precedence.
- `CADDY-001`: Trusted real-Caddy selection, lifetime pinning and recursion denial.
- `CADDY-005`: Bounded adaptation output and owned adapter cleanup.
- `SHIM-003`: Reject npm/SEA wrappers and equivalent alias/file identities.

RUN-003 daemon shutdown/server lifecycle is a later dependency (roadmap 3.8),
not an in-scope completion claim. Short-lived cleanup belongs to CADDY-005.

## Scope

Implement roadmap 3.1-3.2 in new Node Caddy modules and narrow platform helpers:
strict smol-toml parsing; explicit/portable/user/system/safe-PATH precedence;
executable identity/digest pinning and compatibility probes; direct bounded
adapt/validate subprocesses; typed results compatible with the validation part
of the existing Caddy port. Test real disposable fake executables, not installed
real Caddy or a production mock backend.

The delivered close-node-rpc-catalog slice owns protocol/port contracts and has
complete implementation artifacts but remains unarchived due the known custom
schema validation limitation. This Caddy slice consumes those contracts without
modifying protocol behavior; its dependency and separate source ownership are
explicit. Only one writer uses the checkout at a time.

## Non-goals

No route composition, registration handler, mutation queue, apply/rollback,
Admin API contact, certificate generation, persistent state worker, real Caddy
server lifecycle, CLI/TUI, release packaging, Sandbox/UAC/account changes or
Rust production changes. Do not add an apply stub or a mock production backend.
Existing roadmap tasks 3.3-3.9 and all final acceptance gaps stay open.

## Success criteria

- Existing TOML selectors parse with unknown fields and conflicting selectors
  rejected; an invalid higher-priority source never silently falls back.
- Only trusted source locations or safe absolute PATH entries select a native
  executable; project/cwd/environment selectors cannot redirect it.
- npm wrappers, Cadder SEA entries, symlink/hardlink aliases and pinned-image
  mutations are refused without recursive execution or silent reselection.
- The retained Caddy compatibility policy and 32 MiB-per-stream/30-second command
  defaults apply; malformed JSON, nonzero exit, spawn failure, timeout, output
  overflow and cancellation never yield usable partial configuration.
- Owned-child cleanup is bounded and never terminates unrelated processes.
- Focused tests and the relevant full Node gate pass with >=85% own-code line
  coverage; unsupported native proofs remain identified, not inferred.

## Impact

New src/caddy modules, narrow platform integration and test fixtures; roadmap
sequencing and evidence documentation. Reuse current smol-toml/Zod and Node APIs.
No dependency change without a demonstrated need and parent decision. No version,
CI, publication-policy, host security or existing Rust release-path changes.
