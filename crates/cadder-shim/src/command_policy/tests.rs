use super::*;

#[test]
fn table_classifies_core_caddy_command_paths() {
  let run = vec!["run".to_string()];
  let adapt = vec!["adapt".to_string()];
  let fmt = vec!["fmt".to_string()];
  let start = vec!["start".to_string()];
  let version = vec!["--version".to_string()];
  let unknown = vec!["frobnicate".to_string()];

  assert_eq!(
    classify_caddy_command(&run).kind,
    ShimCommandPolicyKind::Managed
  );
  assert_eq!(
    classify_caddy_command(&adapt).kind,
    ShimCommandPolicyKind::ReadOnlyInspection
  );
  assert_eq!(
    classify_caddy_command(&fmt).kind,
    ShimCommandPolicyKind::ExplicitPassthrough
  );
  assert_eq!(
    classify_caddy_command(&start).kind,
    ShimCommandPolicyKind::Unsupported
  );
  assert_eq!(classify_caddy_command(&version).command, "version");
  assert_eq!(
    classify_caddy_command(&unknown),
    ClassifiedShimCommand {
      command: "frobnicate",
      kind: ShimCommandPolicyKind::Unsupported,
      rationale: "No explicit Cadder shim policy entry exists for this Caddy command.",
    }
  );
}
