<!-- arcantry:changelog:start -->
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/2.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

<!-- Arcantry release baseline: 0.8.0 (2026-06-11). Earlier history is not reconstructed. -->

## [1.0.0] - 2026-09-15

### Added

<!-- openspec: add-npm-distribution -->
#### Install Cadder through npm

Run `npx cadder` for one-off operator access or install `cadder` globally to place the version-matched `cadder`, `cadderd`, and Cadder `caddy` shim commands on PATH. npm packages use the same verified native binaries as the portable GitHub archives; the upstream Caddy server remains a separate installation.

<!-- openspec: add-operator-inspection-cli -->
#### Inspect and control the local Cadder runtime

Use focused terminal commands to inspect projects, domains, Caddyfiles, upstream ports, daemon state, diagnostics, and bounded redacted logs. Cadder can also stop a known local port owner after the process identity is explicitly supplied and revalidated. The TUI remains the overview for interactive work.

<!-- openspec: align-v1-to-core-tui -->
#### Run project Caddyfiles together

Keep a Caddyfile in each repository and use the normal `caddy run` command. Cadder's PATH shim sends each project's routes to `cadderd`, which checks ownership and conflicts, combines the active configuration, and applies it to one separately installed Caddy server. Projects can start and stop independently without competing for the same local HTTP and HTTPS ports.

<!-- openspec: align-v1-to-core-tui -->
#### See every route in the TUI

`cadder tui` shows registered projects, domains, upstreams, activation state, daemon state, and Caddy state in one workspace. It can enable or disable one project or route without changing the others.

### Changed

<!-- openspec: replace-custom-storage-with-sqlite -->
#### Keep Cadder state in one local database

Cadder stores project registration, activation state, and bounded recent logs in one owner-protected SQLite database. The portable binaries include SQLite and do not require a system SQLite installation.

<!-- openspec: simplify-v1-foundation -->
#### Ship one portable Cadder bundle per platform

Cadder 1.0 ships as portable archives for Windows x64, Linux x64, Intel macOS, and Apple Silicon macOS. Each archive contains the version-matched `cadder`, `cadderd`, and `caddy` executables, plus the README, changelog, license, sample configuration, and a SHA-256 checksum. The bundled `caddy` executable is Cadder's PATH shim, not the upstream Caddy server. Install Caddy separately and point Cadder to that trusted executable.

### Removed

<!-- openspec: align-v1-to-core-tui -->
#### Keep the 1.0 surface focused

Profiles, autostart, history, export, continuous log tailing and watching, machine-readable output, and MCP are outside Cadder 1.0.

<!-- openspec: simplify-v1-foundation -->
#### Keep 1.0 distribution portable

Cadder 1.0 does not ship installers, Homebrew or Scoop packages, crates.io packages, Authenticode signatures, or macOS notarization.

[Unreleased]: https://github.com/MrMaxie/cadder/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/MrMaxie/cadder/compare/v0.8.0...v1.0.0
<!-- arcantry:changelog:end -->
