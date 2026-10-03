## 1. Local Caddy boundaries

- [x] 1.1 Add focused configuration tests and bind both owned servers to IPv4 and IPv6 loopback on the existing ports. (verification: configuration tests inspect exact listener addresses)
- [x] 1.2 Guard each registration's complete adapted route tree with its active canonical hosts. (verification: a hostless sibling cannot match outside the registration host set)
- [x] 1.3 Bound each `caddy adapt` output stream at 32 MiB and preserve process-tree cleanup. (verification: focused tests cover success at the limit and failure above it)

## 2. Diagnostics and launch trust

- [x] 2.1 Redact the credential forms required by OBS-001 before storage and IPC. (verification: focused tests cover headers, text, JSON, URL query values, and private-key blocks)
- [x] 2.2 Remove terminal control sequences from log and diagnostic messages at the CLI rendering boundary. (verification: focused unit tests cover ANSI CSI, OSC, C0, and C1 input)
- [x] 2.3 Remove automatic PATH discovery for `cadderd` while preserving sibling and explicit diagnostic launch paths. (verification: launch tests reject a PATH-only daemon and accept the sibling or explicit path)

## 3. Compatibility and complete validation

- [x] 3.1 Raise the minimum and integration fixture Caddy version to 2.11.4. (verification: version and integration fixtures target the same patch release)
- [x] 3.2 Run focused crate checks and the complete repository validation, then review the full diff and working tree. (verification: all applicable gates pass with no unrelated files)
