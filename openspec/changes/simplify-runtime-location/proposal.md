## Why

Cadder exposes runtime-selection concepts that add CLI and operational complexity without helping the end user at the current product stage. A single runtime colocated with the installed executables makes the product model easier to understand and operate.

## What Changes

- **BREAKING** Remove runtime profiles and their profile-specific storage isolation from the Cadder product contract.
- **BREAKING** Remove public `--runtime-dir` and profile-selection options from Cadder executables.
- Make the parent directory of the running Cadder executable the single runtime root, independent of the process working directory.
- Load the portable `cadder.toml` from that same directory before per-user, system, or generic PATH Caddy resolution.
- Preserve the `cadder` CLI, its generated Clap help/version behavior, and the explicit `cadder tui` command.
- Keep durable Cadder state under the runtime root's `data` directory.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `operator-cli`: remove public runtime-selection options while retaining the `tui` command.
- `product-topology`: replace profile-scoped runtime ownership with one executable-colocated runtime and resolve configured real Caddy from the portable release directory.
- `runtime-storage`: replace per-profile storage roots with one executable-colocated storage root.
- `local-control-plane`: bind discovery, locks, and local IPC to the executable-colocated runtime root.

## Impact

Affected code includes `RuntimePaths`, daemon launch contracts, the operator CLI, daemon and shim argument parsing, runtime/storage tests, and documentation. This removes profile and runtime-directory environment overrides from the public product behavior.
