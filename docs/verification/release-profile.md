# Release Verification

cargo-dist owns Cadder's release plan, four-target build matrix, archives, SHA-256 checksums, source tarball, and generated GitHub workflow. The workspace `dist` profile inherits the optimized release profile.

Run the non-publishing checks from the repository root:

```sh
mise run dist-plan
mise run dist-build
```

`dist-plan` also runs `dist generate --check`, so workflow drift fails before packaging. Inspect the local platform archive for the `cadder`, `cadderd`, and `caddy` executables plus the README, changelog, Apache-2.0 license, and sample configuration. Confirm the adjacent checksum matches the archive and all three executables report workspace version 1.0.0.

The generated release workflow builds Windows x64, Linux x64, Intel macOS, and Apple Silicon macOS artifacts. Local Windows validation proves only the Windows target; the other targets are verified by their generated CI matrix jobs.
