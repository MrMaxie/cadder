## Context

Cadder currently resolves a per-user runtime directory and optional development profile before clients attach to the daemon. This exposes selection flags, environment overrides, profile-specific durable state, and runtime-root permission handling that are unnecessary for the portable distribution model.

## Goals / Non-Goals

**Goals:**

- Resolve one runtime root from the parent directory of the running executable, never from the current working directory.
- Remove public runtime-directory and profile options while retaining `cadder tui` and Clap-derived help.
- Keep the daemon, shim, and operator on the same runtime root when distributed together.
- Keep test-only explicit runtime construction without exposing it through product CLI behavior.

**Non-Goals:**

- Do not validate, repair, or change ownership and privacy of the portable release directory.
- Do not add profile migration, multi-user coordination, or a replacement runtime-selection interface.
- Do not remove protocol fields whose fixed `default` value preserves compatibility in this change.

## Decisions

### Resolve the runtime root from the executable parent

`RuntimePaths` derives its default root from `current_exe().parent()`. The durable store remains `<runtime-root>/data`; IPC discovery, locks, generated configuration, and runtime metadata remain directly in the runtime root. This is independent of the process working directory.

An explicit constructor remains available for tests and internal controlled handoffs. It is not represented by a public Cadder CLI option or documented environment override.

### Resolve real Caddy from the portable configuration

The `cadder.toml` distributed beside the executables is the primary runtime configuration. Its `[caddy]` table selects either one `real_command` resolved from `PATH` or one absolute `real_path`; the values are mutually exclusive. The command is a program name, never shell input or arguments. This configuration precedes the existing per-user, system, and generic `caddy` PATH sources, so a portable release remains self-contained without reintroducing runtime selection flags.

### Remove public selection but retain a fixed internal identity

The `cadder` and `cadderd` public argument models remove runtime selection. The shim removes matching hidden selection inputs once its managed invocation derives the same executable-colocated root. Existing protocol and record fields keep the fixed value `default` rather than introducing an unrelated compatibility migration.

### Do not enforce portable-root privacy

Runtime-root creation and discovery publication do not apply owner-only permission checks to the executable parent directory. Local IPC peer authentication and storage-file protections remain separate controls.

## Risks / Trade-offs

- [A portable release directory is not writable] → Cadder reports the operating-system write failure when it creates runtime files; it does not select a fallback location.
- [Executables from different directories target different roots] → Portable releases distribute `cadder`, `cadderd`, and the shim together; tests cover deterministic executable-parent resolution.
- [Existing profile data is not selected] → This breaking change starts from the executable-colocated `data` directory and does not migrate legacy profile stores.
