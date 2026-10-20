## Context

The retained behavioral baseline is Rust 1.0.5. The main Node target specs have
been synchronized without archiving the migration roadmap. Current rpc.ts accepts
arbitrary methods/unknown values; response validation allows both or neither
outcome. Existing client/server and compiled fixtures already use mutual HMAC.

## Requirements

IPC-001/002 keep authentication and framing authoritative. IPC-003 owns strict
operation, payload, result and error validation at dispatch and client boundaries.

## Goals and non-goals

### Goals

Close the eight released operations and retain their DTO semantics. Provide
method-indexed TypeScript types and Zod schemas. Exercise malformed authenticated
peers and complete fixture shutdown through the catalog.

### Non-goals

No product state handlers, Caddy adapter, durable storage, presentation methods,
Rust interoperability or new capability negotiation.

## Ownership and trust boundaries

Protocol schemas import no daemon, storage or process implementations. The server
validates before invoking a supplied handler and validates its result before
sending. The client validates before sending and validates the received outcome.
Fixtures own fake state and recovery diagnostics, never production modules.

The adjacent task 1.5 contract slice adds exactly four leaf interfaces in
`src/contracts/ports.ts`:

- `CaddyPort` owns typed `validate` and `apply` configuration calls only; its
  candidate carries the complete adapted JSON body (`format: 'json'`, `body`)
  and effective hash. Existing CADDY-005 validation bounds that body to 32 MiB
  before the port is called. The port does not own executable resolution,
  process lifecycle or admin transport.
- `PlatformPort` owns current-owner lookup and fail-closed protected-path checks.
  `RuntimeOwner` is a type-only contract and is re-exported by the existing
  security adapter without making the contract depend on that implementation.
- `StoragePort` owns loading and persisting `DesiredState`, which contains only
  stable project/domain intent, including project and domain `enabled` flags, and
  no session nonce, process identity, lease or active-route fields.
- `ClientServicePort` owns typed state, bounded log, project/domain activation and
  shutdown calls. Its payloads/results are aliases of the closed eight-operation
  RPC catalog, not a generic transport proxy.

All port methods return the small discriminated `PortResult<T>` using the existing
`ProtocolError` category type. The leaf imports only protocol contracts and never
imports daemon, storage, process or platform implementation modules.

## Data flow and public contracts

The four port declarations are contract-only and do not add a method to the wire
catalog or change dispatch behavior.

An authenticated request identifies one of register-entrypoint-request,
unregister-entrypoint-request, heartbeat-entrypoint-request, query-state-request,
set-entrypoint-enabled-request, set-domain-enabled-request, query-logs-request or
shutdown-daemon-request. All DTO objects reject unknown fields. Responses contain
one outcome and correlate their envelope and nested result/error request IDs.
Business rejection remains a result with accepted:false.

## Decisions

- Retain the Node envelope names method/params to minimize transport changes;
  the eight method values map directly to released behavior. This is not Rust
  wire compatibility. Numeric protocol 3 and security policy 2 remain exact.
- Keep UUID request IDs for Node-generated requests. Malformed requests lacking
  a valid UUID use a fixed valid UUID for uncorrelated rejection, distinct from
  the original request. Clients never accept that fallback as their own response.
- Mirror optional/null DTO values and enum spellings; do not invent a string
  encoding for integers. Reject unsafe JavaScript integer representations.
- Registration identity/nonce ownership is business policy: invalid ownership
  remains accepted:false, not a schema/transport error. Schemas validate shape;
  later handlers enforce ownership semantics.
- RPC omitted/null log limit remains a handler default of 100 with clamping to
  1–200; CLI default 50 is separate. Non-negative safe integer input is allowed.
- Frame size remains the 1 MiB aggregate bound. Logs results cannot exceed 200
  entries. No speculative smaller per-field limits are added.
- Unexpected handler errors use a bounded generic internal diagnostic, not the
  original exception text. Existing runtime/security failures remain distinct.
- Recovery state travels over fixture stdout, not a product snapshot field.

## Failure and recovery

Unknown methods or malformed payloads receive one bounded typed error without
handler execution. A valid request's result/error is correlated; invalid handler
output becomes an internal protocol error. Connections close after one response.
Authentication/sequence/version violations continue to fail before RPC.

## Migration and rollback

Only Node fixture callers change. No Rust endpoint/data negotiation or migration
occurs. Existing runtime data and baseline Rust sources remain untouched.

## Test strategy

Pure schema round trips cover all eight payload/results, nullability, enums and
unknown fields. Authenticated fake peers cover malformed requests, unknown
methods, result/error exclusivity, nested/envelope IDs and invalid handler output.
Compiled child/native smoke uses query-state and shutdown catalog methods. Run
focused tests then nub run check and the Windows compiled-JS smoke. Other native
platforms and real Sandbox remain open evidence gates.

## Risks and trade-offs

- Rust u64/usize exceed JS safe integers: refuse lossy numbers, record this bound.
- Fixture migration can masquerade as product readiness: keep fake handlers only
  under test/ and keep G1/G2 open.
- Response/result nested IDs require one authoritative population point: server
  dispatch fills them from the validated request; client checks them.
- OpenSpec 1.5's strict change validator requires a delta even for the configured
  spec-free implementation schema. The current checkout has no cargo xtask alias
  or manifest for the documented custom check. Strict main-spec validation and
  artifact/schema inspection remain available. Full mise openspec-check is blocked
  by that tool/schema mismatch; do not add duplicate requirements or change
  repository validation policy in this slice.

## Open questions

None. These are implementation decisions within accepted IPC-001..003, not new
product requirements.
