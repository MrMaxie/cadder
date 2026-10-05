#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShimCommandPolicyKind {
  Managed,
  ReadOnlyInspection,
  ExplicitPassthrough,
  Unsupported,
}

#[derive(Debug, Clone, Copy)]
struct ShimCommandPolicyEntry {
  command: &'static str,
  kind: ShimCommandPolicyKind,
  rationale: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassifiedShimCommand<'a> {
  pub(crate) command: &'a str,
  pub(crate) kind: ShimCommandPolicyKind,
  pub(crate) rationale: &'static str,
}

const fn policy_entry(
  command: &'static str,
  kind: ShimCommandPolicyKind,
  rationale: &'static str,
) -> ShimCommandPolicyEntry {
  ShimCommandPolicyEntry {
    command,
    kind,
    rationale,
  }
}

const SHIM_COMMAND_POLICY_TABLE: &[ShimCommandPolicyEntry] = &[
  policy_entry(
    "run",
    ShimCommandPolicyKind::Managed,
    "Registers the project definition with cadderd and keeps Cadder as runtime owner.",
  ),
  policy_entry(
    "adapt",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Reads a Caddy config and prints adapted JSON without mutating runtime state.",
  ),
  policy_entry(
    "build-info",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Reports real Caddy build metadata.",
  ),
  policy_entry(
    "environ",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Reports real Caddy environment information.",
  ),
  policy_entry(
    "help",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Displays command help.",
  ),
  policy_entry(
    "list-modules",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Reports installed real Caddy modules.",
  ),
  policy_entry(
    "validate",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Validates config input without applying it to Cadder-managed runtime state.",
  ),
  policy_entry(
    "version",
    ShimCommandPolicyKind::ReadOnlyInspection,
    "Reports real Caddy version metadata.",
  ),
  policy_entry(
    "completion",
    ShimCommandPolicyKind::ExplicitPassthrough,
    "Generates shell completion output without touching Cadder-managed state.",
  ),
  policy_entry(
    "file-server",
    ShimCommandPolicyKind::ExplicitPassthrough,
    "Starts an unmanaged one-shot real Caddy file server by explicit command.",
  ),
  policy_entry(
    "fmt",
    ShimCommandPolicyKind::ExplicitPassthrough,
    "Formats user-provided config files without touching Cadder-managed runtime state.",
  ),
  policy_entry(
    "manpage",
    ShimCommandPolicyKind::ExplicitPassthrough,
    "Generates manual page output without touching Cadder-managed state.",
  ),
  policy_entry(
    "reverse-proxy",
    ShimCommandPolicyKind::ExplicitPassthrough,
    "Starts an unmanaged one-shot real Caddy reverse proxy by explicit command.",
  ),
  policy_entry(
    "add-package",
    ShimCommandPolicyKind::Unsupported,
    "Mutates the real Caddy binary/module set outside Cadder release ownership.",
  ),
  policy_entry(
    "reload",
    ShimCommandPolicyKind::Unsupported,
    "Mutates real Caddy runtime state outside Cadder's generated config model.",
  ),
  policy_entry(
    "remove-package",
    ShimCommandPolicyKind::Unsupported,
    "Mutates the real Caddy binary/module set outside Cadder release ownership.",
  ),
  policy_entry(
    "start",
    ShimCommandPolicyKind::Unsupported,
    "Starts an unmanaged real Caddy runtime that can drift from cadderd ownership.",
  ),
  policy_entry(
    "stop",
    ShimCommandPolicyKind::Unsupported,
    "Stops real Caddy outside Cadder's runtime ownership boundary.",
  ),
  policy_entry(
    "trust",
    ShimCommandPolicyKind::Unsupported,
    "Mutates local trust stores outside the current Cadder shim contract.",
  ),
  policy_entry(
    "untrust",
    ShimCommandPolicyKind::Unsupported,
    "Mutates local trust stores outside the current Cadder shim contract.",
  ),
  policy_entry(
    "upgrade",
    ShimCommandPolicyKind::Unsupported,
    "Mutates the real Caddy binary outside Cadder release ownership.",
  ),
];

pub(crate) fn classify_caddy_command(args: &[String]) -> ClassifiedShimCommand<'_> {
  let command = normalized_caddy_command(args);
  if let Some(entry) = SHIM_COMMAND_POLICY_TABLE
    .iter()
    .find(|entry| entry.command == command)
  {
    return ClassifiedShimCommand {
      command,
      kind: entry.kind,
      rationale: entry.rationale,
    };
  }

  ClassifiedShimCommand {
    command,
    kind: ShimCommandPolicyKind::Unsupported,
    rationale: "No explicit Cadder shim policy entry exists for this Caddy command.",
  }
}

fn normalized_caddy_command(args: &[String]) -> &str {
  match args.first().map(String::as_str) {
    None | Some("--help" | "-h" | "help") => "help",
    Some("--version" | "-v" | "version") => "version",
    Some(command) => command,
  }
}

#[cfg(test)]
mod tests;
