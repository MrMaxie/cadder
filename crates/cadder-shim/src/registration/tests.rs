use super::*;
use cadder_ipc::ActivationState;

#[test]
fn parses_long_config_and_adapter_flags() {
  let cwd = PathBuf::from("/project");
  let args = vec![
    "run".to_string(),
    "--config".to_string(),
    "Proxy.Caddyfile".to_string(),
    "--adapter".to_string(),
    "caddyfile".to_string(),
  ];

  let (config, adapter) = parse_run_args(&args, &cwd);

  assert_eq!(config, PathBuf::from("Proxy.Caddyfile"));
  assert_eq!(adapter.as_deref(), Some("caddyfile"));
}

#[test]
fn parses_short_config_and_adapter_flags() {
  let cwd = PathBuf::from("/project");
  let args = vec![
    "run".to_string(),
    "-c".to_string(),
    "Caddy.alt".to_string(),
    "-a".to_string(),
    "json".to_string(),
  ];

  let (config, adapter) = parse_run_args(&args, &cwd);

  assert_eq!(config, PathBuf::from("Caddy.alt"));
  assert_eq!(adapter.as_deref(), Some("json"));
}

#[test]
fn defaults_to_caddyfile_in_working_directory() {
  let cwd = PathBuf::from("/project/site");
  let args = vec!["run".to_string()];

  let (config, adapter) = parse_run_args(&args, &cwd);

  assert_eq!(config, cwd.join("Caddyfile"));
  assert_eq!(adapter, None);
}

#[test]
fn registration_captures_owner_and_run_metadata() {
  let args = vec![
    "run".to_string(),
    "--config".to_string(),
    "Proxy.Caddyfile".to_string(),
    "--adapter".to_string(),
    "caddyfile".to_string(),
  ];

  let registration = build_registration(&args).unwrap();

  assert_eq!(registration.activation_state, ActivationState::Active);
  assert_eq!(
    registration.registration_id,
    registration.entrypoint_instance.instance_id
  );
  assert_eq!(
    registration.owner_process.shim_session_nonce,
    registration.entrypoint_instance.shim_session_nonce
  );
  assert_eq!(
    registration.source_config_path.raw,
    PathBuf::from("Proxy.Caddyfile").display().to_string()
  );
  let run = registration.shim_run.unwrap();
  assert_eq!(run.adapter.as_deref(), Some("caddyfile"));
  assert_eq!(run.raw_arguments, args);
  assert_eq!(
    run.command_line,
    "run --config Proxy.Caddyfile --adapter caddyfile"
  );
}
