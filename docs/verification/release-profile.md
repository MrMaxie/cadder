# Release Profile Verification

Cadder release artifacts are built with an explicit workspace release profile. The policy is defined in the root `Cargo.toml` and enforced by `xtask` before release layouts, portable archives, and runtime installers are produced.

## Policy

```toml
[profile.release]
opt-level = "s"
lto = "thin"
codegen-units = 1
debug = false
strip = "symbols"
panic = "unwind"
```

Use the profiling profile when inspecting optimized code with debug symbols:

```sh
cargo build --profile profiling -p cadder-daemon -p cadder-client -p cadder-shim
```

The package `cadder-client` builds the release binary named `cadder`.

## Verification Commands

```sh
cargo xtask verify-release-profile
cargo xtask verify-release-identity
cargo xtask verify-assets
cargo xtask dist --out target/cadder-dist
cargo xtask verify-dist --dir target/cadder-dist
cargo xtask package --out target/cadder-packages --version 1.0.0 --platform windows-x64 --target x86_64-pc-windows-msvc
cargo xtask runtime-installer --out target/cadder-runtime-installers --version 1.0.0 --platform windows-x64 --target x86_64-pc-windows-msvc
cargo xtask verify-runtime-installer-dist --dir target/cadder-runtime-installers --version 1.0.0 --platform windows-x64
cargo xtask verify-release-assets --dir target/release-assets --version 1.0.0 --mode dry-run
```

`verify-dist` checks `cadderd`, `cadder`, `caddy`, and `cadder.toml`. It runs `--help` and `--version` for every included binary, verifies `caddy --cadder-shim-info`, and prints file sizes.

Runtime installer verification checks that each required runtime installer exists for the selected platform, verifies installer and manifest `.sha256` files, and verifies the manifest installs only `cadderd`, `cadder`, `caddy`, and `cadder.toml`.

`verify-release-assets` checks portable archive contents, runtime installer manifests, checksums, and the complete platform matrix.

## Measurement Protocol

Before accepting a profile change:

1. Record host metadata:

   ```sh
   rustc -Vv
   cargo -V
   ```

2. Build and inspect a native portable layout:

   ```sh
   cargo xtask dist --out target/release-profile-dist
   cargo xtask verify-dist --dir target/release-profile-dist
   ```

3. Build and inspect the platform package:

   ```sh
   cargo xtask package --out target/release-profile-package --version 1.0.0 --platform windows-x64 --target x86_64-pc-windows-msvc
   ```

4. Build and inspect the daemon-first native runtime installer when local tooling supports it:

   ```sh
   cargo xtask runtime-installer --out target/release-profile-runtime-installer --version 1.0.0 --platform windows-x64 --target x86_64-pc-windows-msvc
   ```

5. Place the portable archives, runtime installers, and runtime installer manifests in one directory and run `verify-release-assets` in dry-run mode.

Do not commit generated layouts, archives, bundles, checksums, or ad hoc measurement files.

## CI Evidence

The release workflow runs `verify-release-profile`, `verify-assets`, and `verify-release-identity` before portable packaging and runtime installer builds on each release matrix target. `verify-release-assets` downloads all workflow artifacts, rechecks release contracts, checks the complete runtime matrix, and only then allows the public GitHub Release upload job to run.
