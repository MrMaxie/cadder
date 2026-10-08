## Native Caddy boundary

Pinned Caddy 2.11.4 admin identity accepts identifiers and issuer modules, not
static server certificate/key files. Its standard internal issuer can use an
imported root/intermediate CA. Remote administration requires client certificates;
access_control.public_keys contains base64 DER certificates, not raw SPKI.
Configure the authorized client leaf, not the issuing CA, to avoid authorizing
other clients issued by that CA.

Admin disabled applies to the local plaintext endpoint in the pinned source;
remote administration has a separate startup path. Set disabled:true, explicit
loopback remote.listen, private identity identifiers and the internal issuer.
Never rely on remote's default :2021 or the default ACME identity issuers. These
source findings still require native startup and denial evidence.

Identity uses Caddy DefaultStorage rather than arbitrary configured storage.
Explicit child environment must direct data/config/autosave paths into protected
runtime storage on each supported OS. Set install_trust:false for every CA that
can be provisioned, including local. No host trust-store or host Caddy operation
is permitted during development or acceptance.

## Material ownership

Use installed @peculiar/x509 2.1.0 with Node WebCrypto, not handwritten ASN.1 or
an OpenSSL/native dependency. Generate separate ECDSA P-256 root, intermediate
and client keys. Root/intermediate have CA/key-signing constraints; the client
is a non-CA digital-signature identity with clientAuth EKU. Caddy obtains and
renews its server leaf from the intermediate. Do not mint unused production
server credentials merely to satisfy a checklist.

One small Caddy leaf module creates or loads a fixed complete PEM bundle in an
explicit directory for an explicit RuntimeOwner. The caller must already own
the runtime lifetime lock; this module adds no lock/lease/registry framework.
Reuse prepareRuntime, createProtectedFile and assertProtected. Never repair
permissions or replace existing credentials implicitly.

Read existing files with bounded sizes; validate the complete bundle, validity
intervals, CA constraints, signing/client usages, issuer/signature chain and key
pair correspondence before returning usable material. Refuse partial, malformed,
expired, wrong-owner/ACL or linked material. Creation failure cleans only files
exclusively created by this call; it cannot remove pre-existing files. Required
credential bytes may be returned in memory to the internal TLS adapter, but
private keys and arbitrary parser errors must not enter diagnostics or logs.

Restart loading reuses the same complete identity. Expired or incomplete material
is a typed failure, not permission to rotate a CA or overwrite client credentials.
Server leaf renewal is Caddy-owned; CA/client rotation is not an automatic new
product workflow in this slice.

## Generated runtime files

The user approved accepting safe inherited permissions rather than rewriting
native files. Keep assertProtected strict for Cadder-created runtime directories,
CA/client PEM files, IPC material and primary databases. On Windows these paths
still require the expected owner and an explicitly protected owner-only DACL.

Caddy-generated storage files and SQLite journals may inherit owner-only access
from a verified protected runtime directory. Validate only the requested path
and its directory chain within that root: exact expected owner, owner-only access,
correct file/directory type and no links or junctions. On Windows every effective
access rule must grant only the runtime user, with usable owner access; the DACL
protection bit is not required on these descendants. Unix ownership and 0700/0600
requirements remain unchanged. The root itself must pass strict validation.

A different owner is unsafe even when the DACL lists only the runtime user:
Windows owners can change the DACL independently of its access entries. Existing
unsafe descendants are refused, not adopted, repaired or silently regenerated.
This is a read-only path check, not a tree monitor, ACL sealing pass or new storage
framework. Check a known SQLite journal before SQLite can consume it and after
creation while runtime exclusion remains held. Caddy startup integration uses
this boundary; full owned-process lifecycle remains roadmap 3.8.

## Assembly and HTTPS increment

Consume the existing guarded route plan rather than project admin/PKI fields.
Assemble a complete secure CaddyConfig before validation; no admin-policy merge
with arbitrary project settings. Preserve existing loopback route/host guards.
The HTTPS client uses explicit trusted CA, authorized client key/certificate and
normal Node server-name verification. No rejectUnauthorized:false, custom identity
bypass, HTTP retry, ambient trust substitution, redirects or arbitrary admin URLs.

Independent active-state readback must compare the observed effective config,
not a submitted candidate or cached receipt. Define and test deterministic JSON
normalization at this backend boundary before exposing comparable hashes to
ConfigurationTransactions; unknown/unavailable observation is failure, never
idle/null. Idle lifecycle integration remains 3.8, not an invented success stub.
Apply transport failures without an explicit no-transition guarantee stay
ambiguous. Existing transaction fencing owns recovery ordering.

## Verification and boundaries

Deliver material generation/loading and fault tests before dependent admin
assembly/client integration. Then test TLS denial with disposable fixtures and
real pinned Caddy in isolated private staging, using existing owned command cleanup.
Native proof must preserve all host state and include generated server-key
ownership, not just Node-created PEM files. Do not contact unrelated localhost:2019
or infer plaintext absence from an arbitrary host port probe.

The initial native feasibility attempt stopped before script creation because
Arcantry required external-file confirmation unavailable in a headless child.
No Caddy process, certificate or listener was created. That denied external write
must not be retried through another mechanism. The user subsequently authorized
isolated private repository-local staging for temporary probes and permits Windows
Sandbox testing now. Shared source, validation and packaging must not depend on
that private staging. A new tool or permission denial still requires a stop and
supported approval, not an execution-mode workaround. Native evidence remains
open until actual results support it.

Parent integrates independent MiMo reviews and runs proportional focused/full
Node checks. Existing implementation-schema missing-delta validation failures
remain explicit; no duplicate requirements or validator changes are added.
No production mocks, source-side generated JS, staging/commit/release or host
privilege/account/trust changes are authorized by this slice. Disposable Sandbox
execution is now user-authorized; partial-product results do not satisfy the final
completed-product privilege or G7 acceptance gate.
