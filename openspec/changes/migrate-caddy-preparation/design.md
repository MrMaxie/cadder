## Context

The operational baseline is released Rust 1.0.5. Current Node code already has
strict RPC DTOs, authenticated runtime, installation identity and four leaf ports.
No Node Caddy implementation existed at intake; this slice now delivers preparation only. Source authority is retained config.rs,
caddy/{resolver,configuration,tests}.rs, caddy_path_trust.rs, caddy_image.rs and
process_tree.rs; test/compatibility freezes their retained behavior.

## Requirements

REG-004 governs parsing and precedence. CADDY-001/SHIM-003 govern selection,
identity and pinning. CADDY-005 governs short-lived output limits and cleanup.
RUN-003 governs daemon shutdown/server lifecycle and remains outside slice closure.
The complete JSON carrier and validation result already exist in
src/contracts/ports.ts. No normative requirement is duplicated here.

## Goals and non-goals

Deliver configuration -> trusted executable -> bounded adapt -> JSON candidate ->
bounded validate. Keep the installed upstream Caddy separate and mandatory.
Do not compose routes, apply configuration, contact Admin API or start a server.

## Ownership and trust boundaries

Configuration parsing is pure; executable selection is daemon-owned. Preserve
explicit absolute daemon override, portable cadder.toml, per-user configuration,
system configuration, then safe PATH. Trusted files select a single command name
or absolute path; no arguments, relative command paths or project/shim/environment
selectors. Existing empty higher-priority files can fall through, malformed or
unsafe selected sources cannot. Use physical installation identity for npm and
SEA, not cwd, argv wrappers or the shared Node executable as a portable anchor.

Platform defaults preserve the released OS configuration locations. Windows
known folders use the existing small PowerShell/.NET adapter rather than trusting
PROGRAMDATA/APPDATA overrides for system policy. Filesystem checks use native
regular-file/executable checks and dev/inode or equivalent identity; Unix aliases
are followed for identity, Windows final reparse/script candidates are denied.
Reject local/global npm script wrappers; native Windows Scoop descriptors may
resolve only to a checked native target. Compare all known distribution entries
and aliases before executing a candidate.

Cache selected path and immutable image evidence for the resolver lifetime.
Reverify canonical path, identity and content digest before and immediately after
spawn; retain an open image anchor and release it on explicit close. Compatibility
probes preserve baseline versions >=2.11.4 and <3.0.0 and the 19 required module IDs.
Evidence failures never silently select another image. Shared probes belong to
the resolver lifetime: caller cancellation stops that wait, not other callers'
probes, and closing the resolver cancels its pending probe. This is check-and-detect behavior, not an
atomic pathname-exec guarantee against arbitrary same-owner concurrent swaps.

## Data flow and public contracts

1. Parse strict TOML with installed smol-toml and Zod; retain snake_case selectors.
2. Resolve and pin one trusted native image, then run bounded version/modules probes.
3. Adapt using an argument array with canonical config path when supplied, raw
   path otherwise, and retained adapter metadata or caddyfile default.
4. Reject failed/partial/malformed output; carry complete adapted JSON and SHA-256.
5. Validate a JSON candidate through real Caddy's command, exposing the existing
   CaddyPort validation signature without adding an unimplemented apply method.

Errors stay typed and bounded. No child stderr or project text becomes an
unbounded authenticated error payload. No RPC catalog or daemon-ready state changes.

## Decisions

Use Node spawn with shell disabled and Node crypto/filesystem primitives. One
narrow owned-command helper is shared by compatibility probes and adaptation;
this is application process ownership, not a custom build/test framework.

A native Windows reproducer confirmed that taskkill cannot clean a descendant
retaining stdout/stderr after its leader exits. The supervisor therefore approves
one narrow PowerShell/.NET Windows Job Object adapter, matching the retained Rust
per-command job semantics. This is necessary product OS integration, not optional
hardening: attach the selected child before it can execute, keep the job handle
non-inheritable, disallow descendant breakaway and kill the owned job on completion,
failure or wrapper exit. Preserve direct native argv and binary streams, existing
image checks, limits and bounded teardown. No native Node addon, distributed
launcher binary, general process broker, external tool installation, dependency
change or host privilege/account configuration is authorized. Failure to prove
the narrow adapter must stop this slice rather than trigger another layer.

Implementation is serial: config/resolver plus their required bounded probe
helper, then adapter/validation using that verified handoff. One writer owns
each stage; fresh MiMo reviews cannot write source. Production has no test
bypass, fake backend, ambient fixture switch or arbitrary runner injection.
Synthetic fake-Caddy behavior remains in test/fixtures and owned temporary files.

## Failure and recovery

Adapt/validate defaults are 30 seconds and at most 32 MiB separately on stdout
and stderr; metadata probes retain the smaller 1 MiB bound. Reject output as soon
as a bound is exceeded rather than accepting its prefix. Always observe stream,
spawn and exit errors, settle pending reads, and close stdin explicitly.

Cleanup targets only the owned child/process group/tree, with a bounded teardown
and primary-error preservation. Unix can use an owned detached process group;
Windows must use a narrow owned-tree mechanism and prove its actual guarantees.
An orphan/pipe-inheritance gap cannot be recorded as full CADDY-005 completion:
report a concrete blocker before adding broad process enumeration, native addons
or another orchestration layer. Unrelated processes must survive.

After native success, the Windows helper waits up to four seconds for the Job's
active process count to reach zero. After leader exit, unsettled pipes cause a
bounded five-second stream error, not an operation timeout. Unix descendants
that deliberately escape the group cannot be reclaimed by this narrow mechanism;
that native acceptance limitation remains open, without process enumeration.

Validation staging belongs under exclusively created system-temp storage and is
removed after all child finalizers, using three built-in removal retries with
100 ms linear backoff for transient filesystem locks. No generated JS, bundles or captures enter
checkout. Candidate rejection does not publish state or modify existing config.

## Migration and rollback

No product cutover, old-data access or persisted schema change. Retain Rust and
its packaging unchanged. Reverting this slice removes only the new Node modules
and tests, not runtime or compatibility work. Sandbox is mandatory final-product
acceptance under 2.8/7.4; no intermediate test is falsely marked passed.

## Test strategy

Use frozen TOML inputs and retained Rust precedence/adapter cases as authority.
Test explicit/portable/user/system/PATH sources, invalid-priority denial, command
injection, relative PATH entries, installation entries, aliases/hardlinks,
wrappers, pin mutations and missing/incompatible probes.

Execute disposable fake Caddy processes for real argv/cwd, adaptation/validation,
invalid UTF-8/JSON, exits, both stream limits, spawn errors, timeout, abort,
parent/grandchild cleanup and unrelated-child survival. Fake executables must
pass the same production path policy; no test-only production trust switch.
Use current build dependencies only for outside-checkout fixture emission.
Native Windows proof and modeled Unix cases are labeled separately; later real
Caddy/npm/SEA acceptance is not supplied by these tests.

Run focused Vitest tests after each stage, nub run check after integration,
strict main/roadmap OpenSpec checks and git diff --check. Inspect LSP results
without treating an inconclusive push-only probe as clean. Retain Rust tests as
baseline checks when test-only fixtures are affected.

## Risks and trade-offs

- Path-based spawning cannot claim atomic image identity -> pre/post verification,
  owned cleanup and explicit race residual, with mutation regressions.
- Windows inherited descendants/pipes require concrete teardown proof -> do not
  substitute a live-parent taskkill case for orphan cleanup evidence.
- npm/SEA artifacts are not built yet -> realistic layout fixtures, no distribution
  acceptance claim until G6.
- Existing OpenSpec custom implementation validation mismatch -> schema/status
  and main-spec validation plus honest residual; no fake deltas or validator layer.

## Open questions

No product behavior or sequencing question is open. If a required native cleanup
or identity guarantee cannot be delivered with the approved small Node/platform
boundary, stop with the reproducer and request a parent decision before expanding
implementation. Such a blocker leaves the affected task incomplete.
