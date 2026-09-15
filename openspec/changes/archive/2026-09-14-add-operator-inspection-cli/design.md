## Context

The daemon snapshot already owns current Cadder registrations, source Caddyfile paths, project and domain activation, Caddy runtime state, and upstream targets. The operator client can query and mutate that state through `OperatorContext`, but the executable currently exposes only `cadder tui`. Local socket ownership is intentionally outside the daemon protocol and is not represented in the snapshot.

This change crosses the public CLI, local operating-system inspection, process control, documentation, and TUI copy. The primary audience is a developer answering operational questions such as "which registered project points at this port?" and "is this Caddyfile currently contributing a live route?" Deep identifiers remain available only where they disambiguate an operator action.

## Goals / Non-Goals

**Goals:**

- Provide one consistent CLI vocabulary for daemon lifecycle and status, projects, domains, ports, Caddyfiles, diagnostics, and bounded logs.
- Correlate local socket owners with Cadder registrations in both port-first and Caddyfile-first flows.
- Reuse the daemon snapshot as the source of truth for Cadder-owned state.
- Make process termination explicit, revalidated, and bounded to the expected PID.
- Keep the TUI focused on real daemon and Caddy state.

**Non-Goals:**

- Adding MCP, machine-readable output, continuous log tailing or watching, history, profiles, or autostart.
- Moving general process discovery or termination into `cadderd`.
- Claiming that a registered upstream is listening when local socket inspection cannot establish an owner.
- Resolving arbitrary hostnames or treating remote upstream ports as local process ownership.

## Decisions

### Organize the CLI around developer entry points

The public commands are `status`, `daemon`, `projects`, `domains`, `port`, `caddyfile`, `diagnostics`, `logs`, and `tui`. Resource commands use explicit lifecycle, `list`, `inspect`, `enable`, `disable`, and `kill` verbs where applicable. This is more discoverable than exposing the internal `entrypoint` term and keeps destructive behavior visibly separate from inspection.

Project and domain activation commands reuse `OperatorContext`. Port and Caddyfile inspection query one current daemon snapshot when available and correlate it with a separately captured local socket snapshot. Port inspection remains useful when `cadderd` is offline; it reports that registration data is unavailable instead of starting the daemon implicitly.

Daemon lifecycle commands reuse the existing attach-first start, bounded stop, and ordered restart operations. Diagnostics render the runtime and configuration diagnostics already present in a snapshot. Log commands resolve runtime, project, or domain streams through the existing API and retain the daemon-enforced limit of 200 entries; they do not introduce a polling or tail abstraction.

### Keep one correlation model behind both directions

A client-side inspection module converts daemon registrations into project, domain, Caddyfile, and local-upstream relationships. Local upstreams accept loopback aliases (`localhost`, `127.0.0.1`, and `::1`) and wildcard bind addresses while preserving the original target for output. Remote or dynamic upstreams are displayed but are not attributed to a local port owner.

Caddyfile matching compares normalized absolute paths and uses the canonical source path supplied by the daemon when available. Output exposes distinct facts rather than one overloaded "active" flag: registered state, project activation, domain activation, Caddy runtime state, configuration apply state, and local listener ownership.

### Use focused libraries for operating-system data

`netstat2` provides cross-platform socket enumeration and associated process IDs. `sysinfo`, with only its system feature enabled, provides process name, executable path, and process signaling. The standard library cannot enumerate another process's sockets or terminate a process by PID. Calling `netstat`, `lsof`, or platform task tools would require custom parsing, locale handling, quoting, and platform branches. A complete port-kill CLI framework would add presentation and policy that Cadder would need to bypass; the two focused libraries keep Cadder's correlation and safety policy explicit.

Socket discovery is isolated behind a small trait so deterministic fixtures can verify correlation without depending on live machine state. The production adapter performs blocking system enumeration before or outside asynchronous daemon work rather than introducing an async abstraction around synchronous OS APIs.

### Guard process termination with current ownership

`cadder port kill <port> --pid <pid>` requires an expected PID. Immediately before signaling, Cadder captures sockets again and refuses unless the same PID still owns the requested local port. It then loads that process from `sysinfo` and sends the platform's supported termination signal. The command never chooses a process implicitly, never kills every owner, and never makes the daemon responsible for an unrelated process.

Permissions remain those of the invoking user. Missing process visibility is reported as unavailable evidence, and permission failures return the existing permission-oriented exit category. A successful signal request is not described as graceful shutdown.

### Separate routine and diagnostic information

`projects list` and `domains list` show names, paths, activation, and targets needed for daily work. `inspect` commands add process IDs, executable paths, socket protocol/address, and runtime/config state because those values support a concrete diagnosis or follow-up command. `diagnostics` may expose Caddy process and binary details, while `logs` shows only the daemon's bounded redacted entries. The TUI header displays only `cadderd` and Caddy because MCP is not a Cadder product surface.

## Risks / Trade-offs

- [Socket ownership can change between inspection and signaling] -> Re-enumerate immediately before kill and require the caller's expected PID.
- [A platform can hide process ownership without sufficient permissions] -> Report unknown ownership without guessing and keep inspection results partial.
- [One port can have multiple sockets or owners] -> Preserve every socket-owner pair and require a specific PID for termination.
- [An upstream string can be remote or dynamic] -> Correlate only syntactically local targets and display other targets without local-owner claims.
- [CLI output can become noisy] -> Keep lists compact and reserve identifiers and process details for inspect commands.
- [New system crates increase binary size and maintenance] -> Use exact versions, disable unnecessary `sysinfo` features, and isolate both crates behind one module.

## Migration Plan

1. Add the new command definitions and inspection modules without changing the daemon protocol.
2. Replace stale recovery guidance with the delivered `projects` and `domains` commands.
3. Update operator documentation and remove the unbacked MCP TUI label.
4. Rollback is a client-only revert; stored state and daemon compatibility are unchanged.

## Open Questions

None. Machine-readable output and broader lifecycle management require separate product decisions.
