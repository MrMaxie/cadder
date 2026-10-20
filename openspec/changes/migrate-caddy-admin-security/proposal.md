## Why

Preserve the project Caddyfile -> managed shim -> daemon -> operator journey.
The next capability gives the existing transaction queue an authenticated Caddy
administration channel without restoring plaintext localhost administration.
Caddy remains the required runtime engine; Cadder remains TypeScript-only.

## Requirement IDs

- CADDY-006: Protected loopback mutual TLS, validated server identity, authorized
  client material, no system trust installation and no project policy override.

CADDY-003 supplies independent active-state verification and ambiguity fencing;
CADDY-002/004 supply owned process and local route boundaries. This slice does
not close their whole-product acceptance.

## Scope

Implement roadmap 3.6–3.7 incrementally: protected CA/client material first,
then secure configuration assembly and bounded authenticated administration.
Reuse existing runtime-security, CaddyConfig, preparation, composition and owned
command boundaries. No dependency additions or shared RPC/port changes.

The approved engineering correction assigns root/intermediate CA and authorized
client generation to Node WebCrypto/@peculiar/x509. Caddy's standard internal
issuer obtains and renews the admin server certificate. Its default identity
storage must be redirected into the protected runtime boundary, not injected
with a separately generated leaf.

## Non-goals

No TLS proxy, custom issuer/plugin, CertMagic storage-layout injection, plaintext
fallback, OS trust installation, native runtime addon, CA installer or dependency
change. No daemon handlers, application SQLite worker, CLI/TUI, distribution,
full owned-server lifecycle (3.8), final completed-product privilege acceptance
or publication. The user permits disposable Sandbox testing of this increment;
it does not close G7. Test-only fake TLS servers are not production adapters.

## Success criteria

- Complete valid CA/client material is created or validated inside an explicitly
  supplied owner-protected directory. Unsafe, partial, malformed, mismatched or
  expired existing material is refused, not repaired or silently regenerated.
- Secure assembly owns admin/PKI policy and isolates Caddy identity/cache storage;
  project input cannot widen trust, listeners or plaintext administration.
- Node HTTPS validates the CA chain and server identity and presents only the
  authorized client certificate. Requests and responses are bounded; failures
  never trigger HTTP fallback or cached-receipt observation.
- Native Caddy acceptance proves authorized access, missing/foreign-client
  refusal, wrong server trust/identity refusal and protected storage. Source
  inspection and fixture success alone do not close task 3.7.
- Focused/full Node checks cover changed boundaries above 85% own lines, with
  independent MiMo review and explicit native/platform gaps.

## Impact

Additive Caddy leaf sources and dedicated tests; this focused planning/evidence
and roadmap progress only when supported by executed checks. Main requirements
are unchanged. Rust, earlier dirty work and final acceptance gates remain intact.
