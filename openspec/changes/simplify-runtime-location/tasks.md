## 1. Portable runtime resolution

- [x] 1.1 Resolve the default runtime root from the running executable's parent and retain an explicit test seam.
- [x] 1.2 Remove profile-based and environment-selected runtime resolution while preserving a fixed internal default identity where protocol compatibility requires it.
- [x] 1.3 Stop applying runtime-root ownership and privacy validation to the executable parent directory.

## 2. Public command surfaces

- [x] 2.1 Remove runtime-directory and profile options from `cadder` while retaining Clap and the `tui` subcommand.
- [x] 2.2 Remove public runtime-directory and profile options from `cadderd` and align daemon launch with the portable root.
- [x] 2.3 Remove matching shim runtime-selection inputs and align managed invocations with the portable root.

## 3. Verification and documentation

- [x] 3.1 Update focused unit and binary tests for executable-parent resolution and rejected public runtime-selection options.
- [x] 3.2 Update runtime configuration and architecture documentation for the portable model.
- [x] 3.3 Run formatting, linting, workspace tests, OpenSpec validation, and the repository check suite.
