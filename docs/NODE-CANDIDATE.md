# Cadder 2.0 release candidate

This is an unpublished 2.0.0-rc.1 candidate. Use the supplied npm tarball or the
standalone archive for your platform. Both provide `cadder`, `cadderd`, and the
`caddy` shim. Upstream Caddy is installed separately.

The npm package requires Node 24 LTS, version 24.18.0 or later in the 24.x series:

```sh
npm install --global ./cadder-2.0.0-rc.1.tgz
```

Standalone archives need no installed Node or npm. Verify the extracted files
against `SHA256SUMS`, keep the three executables and accompanying files together,
and add their directory to PATH. `BUILD.json` identifies the source and platform.

## Configure Caddy

Set one trusted upstream source in a per-user `cadder.toml`:

- Windows: `%APPDATA%\Cadder\Cadder\config\cadder.toml`.
- macOS: `~/Library/Application Support/dev.Cadder.Cadder/cadder.toml`.
- Linux: `$XDG_CONFIG_HOME/cadder/cadder.toml`, or `~/.config/cadder/cadder.toml` when unset.

Use the supplied `cadder.toml.example` as a starting point. For example, if
`caddy-real` is the name of your separately installed native Caddy executable:

```toml
[caddy]
real_command = "caddy-real"
```

Alternatively, set `real_path` to its absolute path. Set only one selector.

## Use Cadder

Run `caddy run` from each project containing a Caddyfile. In another terminal,
use `cadder status`, `cadder projects list`, or `cadder tui`.

In the TUI, Up/Down select a project or domain, Enter/Space toggle it, `s` asks
to stop the daemon, `r` restarts it, and `q` quits the TUI. Enter starts an
offline daemon. Run `cadder --help` for the current command surface.

## Switch versions

Stop the old daemon using its own `cadder daemon stop` before changing PATH.
Install a matching set of all three commands, then re-run `caddy run` in each
project. The candidate keeps separate runtime storage and does not import or
delete the Rust database. Keep the previous release for rollback; stop the
candidate before switching back and registering the projects again.
