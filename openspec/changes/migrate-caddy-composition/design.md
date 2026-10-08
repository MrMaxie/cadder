## Evidence and dependency order

The retained Rust behavior is in crates/cadder-daemon/src/caddy/configuration.rs:
active_domains, detect_conflicts, extract_http_routes, filter_route_hosts,
guard_registration_routes, compose_config and namespace_config_ids. Its tests
cover disabled domains, conflicts, loopback listeners and nested hostless siblings.
crates/cadder-ipc/src/state.rs defines enabled activation states and canonical
trim/trailing-dot/IDNA/ASCII-case behavior. Existing Node DTOs and preparation
carriers are the input contracts; this slice does not reopen those contracts.

## Boundary and data flow

Complete adapted JSON + registration activation/domain intent -> extracted HTTP
route trees -> canonical ownership check -> domain-filtered, outer-guarded trees
-> typed route plan containing owned HTTP/HTTPS servers and TLS subjects.

Use a small pure API in new src/caddy modules. Parse only complete bounded
adapted carriers using existing preparation integrity checks. Preserve arbitrary
route/handler JSON; do not parse Caddyfile syntax or reinterpret handler settings.
Only route host matchers participate in domain extraction/filtering. Unrelated
handler properties named host are not registration ownership declarations.

Registration activation and domain activation are both respected. Canonicalize
names at the business boundary rather than trusting caller case/spelling.
Node domainToASCII normally parses URL hosts, unlike Rust's literal IDNA input.
Use a fixed nonnumeric suffix during whole-domain mapping to prevent numeric IP
reinterpretation; keep the original trimmed domain with ASCII-only lowercasing
on failure or URL-delimiter input. Strip ASCII trailing dots before mapping.
Fixtures cover the observed differences, not exhaustive Node/Rust IDNA parity.
Deduplicate domains within one registration; competing registration IDs produce
bounded diagnostics with source paths and no successful plan. Reject ambiguous
duplicate registration input rather than making iteration order authoritative.
Keep route order within a project and a documented deterministic project order.

## Route guards and failure behavior

Preserve retained branches and hostless siblings behind an outer host matcher
containing only that registration's active canonical hosts. An emptied host
matcher must not become a catch-all. If pruning removes every subroute handler
from a retained terminal parent, preserve its matchers, empty handle array and
terminal position; deleting that parent would let a later route handle the request.
Nonterminal empty parents may be dropped. Proxy-shaped data inside an opaque
handler payload does not contribute upstream metadata.
A disabled-only project or a registration with no enabled hosts contributes no
routes. Never fabricate placeholder routes
when prepared input is missing or invalid. Return failure without mutating inputs
or a previously produced plan; committing/rejecting runtime changes belongs to
roadmap 3.4-3.5.

Retain exact dual-stack loopback ports 80/443 and HTTP-copy ID namespacing from
the Rust behavior. Ignore project server listeners and root admin/TLS policy;
only extracted routes enter the plan. The plan is explicitly not CaddyConfig:
it supplies route servers and TLS subjects for later daemon-owned secure config
assembly. Do not emit localhost:2019, an admin fallback or a TLS-policy stub.

## Verification

Pure JSON fixtures exercise multiple projects, all activation states, IDN/case/
trailing dots, duplicate ownership, malformed/hash-mismatched prepared input,
nested mixed enabled/disabled branches, hostless siblings, arbitrary handler
payloads, stable ordering, ID namespacing and input immutability. No installed
Caddy, network listener, process, filesystem permissions or Sandbox is needed.

MiMo Flash independently checks retained behavior and boundary assumptions;
MiMo Pro reviews the actual implementation and test evidence. The parent resolves
concrete findings, performs simplification/fresh-eyes review, runs the full Node
gate and checks main specs/roadmap/schema plus the diff. Known global archive and
native/release blockers remain explicit, not claimed as passed.

## Alternatives and migration

Do not copy the Rust plaintext admin config or mock/placeholder branches into
the new production path. Do not build a generic JSON rule engine, new parser or
queue to deliver this slice. Additive pure modules leave the Rust baseline and
existing Node runtime/preparation unchanged and independently reversible. Later
queue and protected Admin API work consumes the plan before any real apply.
