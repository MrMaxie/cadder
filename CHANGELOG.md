# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/2.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Rebuilt the product around the `cadder`, `cadderd`, and PATH-facing `caddy` executables.
- Reduced the operator to the Ratatui-based `cadder tui` workflow.
- Replaced custom file storage with one owner-protected SQLite database.
- Replaced custom validation and release orchestration with mise and cargo-dist.

### Removed

- Removed profiles, history, autostart, machine-output, export, tail, watch, and legacy storage surfaces.

[Unreleased]: https://github.com/MrMaxie/cadder/commits/HEAD
