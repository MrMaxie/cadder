---
impact: major
components:
  - distribution-and-upgrades
  - daemon-lifecycle
  - local-control-plane
audiences:
  - developers
  - operators
observable_impact: user-felt
changelog: include
---

## Changed

### Ship one portable Cadder bundle per platform

Cadder 1.0 ships as portable archives for Windows x64, Linux x64, Intel macOS, and Apple Silicon macOS. Each archive contains the version-matched `cadder`, `cadderd`, and `caddy` executables, plus the README, changelog, license, sample configuration, and a SHA-256 checksum. The bundled `caddy` executable is Cadder's PATH shim, not the upstream Caddy server. Install Caddy separately and point Cadder to that trusted executable.

## Removed

### Keep 1.0 distribution portable

Cadder 1.0 does not ship installers, Homebrew or Scoop packages, crates.io packages, Authenticode signatures, or macOS notarization.
