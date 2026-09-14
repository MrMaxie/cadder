use super::*;
use crate::{paths::RuntimePaths, runtime::RuntimeTimeouts};
use cadder_ipc::{
  ActivationState, EntrypointInstanceIdentity, LogStreamIdentity, OwnerProcessIdentity, SourcePath,
};
use chrono::Utc;
use std::{ffi::OsString, fs};

struct EnvSnapshot {
  values: Vec<(&'static str, Option<OsString>)>,
}

impl EnvSnapshot {
  fn capture(keys: &[&'static str]) -> Self {
    Self {
      values: keys
        .iter()
        .copied()
        .map(|key| (key, env::var_os(key)))
        .collect(),
    }
  }
}

impl Drop for EnvSnapshot {
  fn drop(&mut self) {
    for (key, value) in &self.values {
      unsafe {
        match value {
          Some(value) => env::set_var(key, value),
          None => env::remove_var(key),
        }
      }
    }
  }
}

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
  crate::TEST_ENV_LOCK
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn registration(id: &str, hosts: &[&str]) -> EntrypointRegistration {
  let now = Utc::now();
  let identity = EntrypointInstanceIdentity {
    instance_id: id.to_string(),
    started_at_utc: now,
    shim_session_nonce: format!("{id}-nonce"),
  };
  EntrypointRegistration {
    registration_id: id.to_string(),
    entrypoint_instance: identity.clone(),
    source_working_directory: SourcePath::new(".", None),
    source_config_path: SourcePath::new(format!("{id}.Caddyfile"), None),
    registered_domains: hosts
      .iter()
      .map(|host| RegisteredDomain::active(*host))
      .collect(),
    activation_state: ActivationState::Active,
    owner_process: OwnerProcessIdentity {
      process_id: 1,
      process_start_time_utc: now,
      shim_session_nonce: identity.shim_session_nonce,
      executable_path: None,
    },
    log_stream: LogStreamIdentity::entrypoint(id),
    shim_run: None,
    created_at_utc: now,
    last_heartbeat_utc: now,
  }
}

fn write_file(path: &Path) {
  fs::write(path, "fake caddy").unwrap();
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
  }
}

fn write_real_caddy_config(path: &Path, default: &Path) {
  let escape = |value: &Path| {
    value
      .display()
      .to_string()
      .replace('\\', "\\\\")
      .replace('"', "\\\"")
  };
  fs::write(
    path,
    format!("[caddy]\nreal_path = \"{}\"\n", escape(default)),
  )
  .unwrap();
}

fn canonical(path: &Path) -> PathBuf {
  path.canonicalize().unwrap()
}

#[test]
fn host_collection_and_filtering_walk_nested_values() {
  let config = json!({
    "apps": {
      "http": {
        "servers": {
          "srv0": {
            "routes": [
              {
                "match": [{ "host": ["App.Localhost", 7, "api.localhost"] }],
                "handle": [{
                  "routes": [{
                    "match": [{ "host": ["nested.localhost"] }]
                  }]
                }]
              },
              { "match": [{ "path": ["/health"] }] }
            ]
          }
        }
      }
    }
  });

  let hosts = extract_hosts(&config);

  assert_eq!(
    hosts,
    BTreeSet::from([
      "app.localhost".to_string(),
      "api.localhost".to_string(),
      "nested.localhost".to_string(),
    ])
  );

  let mut route = json!({
    "match": [{ "host": ["App.Localhost", "disabled.localhost", 7] }],
    "handle": [{
      "routes": [{
        "match": [{ "host": ["api.localhost"] }]
      }]
    }]
  });
  let mut retained_any = false;
  filter_hosts_recursive(
    &mut route,
    &BTreeSet::from(["app.localhost".to_string()]),
    &mut retained_any,
  );

  assert!(retained_any);
  assert_eq!(
    route
      .pointer("/match/0/host")
      .and_then(Value::as_array)
      .unwrap(),
    &[json!("App.Localhost")]
  );
  assert!(
    route
      .pointer("/handle/0/routes/0/match/0/host")
      .and_then(Value::as_array)
      .unwrap()
      .is_empty()
  );
}

fn write_fake_caddy(path: &Path) {
  #[cfg(windows)]
  fs::write(
    path,
    r#"@echo off
if "%1"=="adapt" (
echo {"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["project.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}
exit /b 0
)
exit /b 1
"#,
  )
  .unwrap();

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::write(
      path,
      r#"#!/bin/sh
if [ "$1" = "adapt" ]; then
printf '%s\n' '{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["project.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}'
exit 0
fi
exit 1
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
  }
}

fn write_fake_caddy_with_adapt(path: &Path, adapt_body: &str, exit_code: i32) {
  #[cfg(windows)]
  fs::write(
    path,
    format!(
      r#"@echo off
if "%1"=="adapt" (
echo {adapt_body}
exit /b {exit_code}
)
exit /b 0
"#
    ),
  )
  .unwrap();

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::write(
      path,
      format!(
        r#"#!/bin/sh
if [ "$1" = "adapt" ]; then
printf '%s\n' '{adapt_body}'
exit {exit_code}
fi
exit 0
"#
      ),
    )
    .unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
  }
}

fn write_slow_fake_caddy(path: &Path) {
  #[cfg(windows)]
  fs::write(
    path,
    r#"@echo off
if "%1"=="adapt" (
"%SystemRoot%\System32\ping.exe" -n 60 127.0.0.1 >nul
echo {"apps":{}}
exit /b 0
)
exit /b 0
"#,
  )
  .unwrap();

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::write(
      path,
      r#"#!/bin/sh
if [ "$1" = "adapt" ]; then
/bin/sleep 60
printf '%s\n' '{"apps":{}}'
exit 0
fi
exit 0
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
  }
}

fn write_runtime_fake_caddy(path: &Path) {
  #[cfg(windows)]
  fs::write(
    path,
    r#"@echo off
if "%1"=="adapt" (
echo {"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["project.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}
exit /b 0
)
if "%1"=="reload" exit /b 0
if "%1"=="stop" (
ping -n 8 127.0.0.1 >nul
exit /b 0
)
if "%1"=="run" (
:run_loop
ping -n 2 127.0.0.1 >nul
goto run_loop
)
exit /b 1
"#,
  )
  .unwrap();

  #[cfg(not(windows))]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::write(
      path,
      r#"#!/bin/sh
case "$1" in
adapt)
  printf '%s\n' '{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["project.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}'
  exit 0
  ;;
reload)
  exit 0
  ;;
stop)
  sleep 6
  exit 0
  ;;
run)
  while true; do sleep 1; done
  ;;
esac
exit 1
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
  }
}

#[test]
fn trusted_caddy_source_explicit_override_precedes_user_and_system_configuration() {
  let dir = tempfile::tempdir().unwrap();
  let explicit = dir.path().join(exe_name_for_test("explicit-caddy"));
  let user = dir.path().join(exe_name_for_test("user-caddy"));
  let system = dir.path().join(exe_name_for_test("system-caddy"));
  for path in [&explicit, &user, &system] {
    write_file(path);
  }
  let user_config = dir.path().join("user.toml");
  let system_config = dir.path().join("system.toml");
  write_real_caddy_config(&user_config, &user);
  write_real_caddy_config(&system_config, &system);
  let resolver = RealCaddyResolver::with_test_sources(
    Some(explicit.clone()),
    TrustedConfigPaths {
      user: Some(user_config),
      system: Some(system_config),
    },
    None,
  );

  assert_eq!(resolver.resolve().unwrap(), canonical(&explicit));
}

#[test]
fn trusted_caddy_source_user_configuration_precedes_system_configuration() {
  let dir = tempfile::tempdir().unwrap();
  let user_default = dir.path().join(exe_name_for_test("user-default"));
  let system_default = dir.path().join(exe_name_for_test("system-default"));
  for path in [&user_default, &system_default] {
    write_file(path);
  }
  let user_config = dir.path().join("user.toml");
  let system_config = dir.path().join("system.toml");
  write_real_caddy_config(&user_config, &user_default);
  write_real_caddy_config(&system_config, &system_default);
  let paths = TrustedConfigPaths {
    user: Some(user_config),
    system: Some(system_config),
  };

  let default = RealCaddyResolver::with_test_sources(None, paths, None);

  assert_eq!(default.resolve().unwrap(), canonical(&user_default));
}

#[test]
fn trusted_caddy_source_uses_system_default_after_empty_user_config() {
  let dir = tempfile::tempdir().unwrap();
  let system_default = dir.path().join(exe_name_for_test("system-default"));
  write_file(&system_default);
  let user_config = dir.path().join("user.toml");
  let system_config = dir.path().join("system.toml");
  fs::write(&user_config, "[caddy]\n").unwrap();
  write_real_caddy_config(&system_config, &system_default);
  let paths = TrustedConfigPaths {
    user: Some(user_config),
    system: Some(system_config),
  };

  let default = RealCaddyResolver::with_test_sources(None, paths, None);

  assert_eq!(default.resolve().unwrap(), canonical(&system_default));
}

#[cfg(windows)]
#[test]
fn trusted_caddy_source_system_configuration_ignores_program_data_environment_override() {
  let _lock = lock_env();
  let _snapshot = EnvSnapshot::capture(&["ProgramData"]);
  let expected = system_config_path().expect("Windows exposes the ProgramData known folder");
  unsafe { env::set_var("ProgramData", r"C:\untrusted-program-data") };

  assert_eq!(system_config_path(), Some(expected));
}

#[test]
fn pinned_caddy_image_parses_semantic_version_and_module_inventory() {
  let modules = required_caddy_modules();
  let output = serde_json::to_vec(
    &modules
      .iter()
      .map(|module| json!({ "module_name": module, "module_type": "standard" }))
      .collect::<Vec<_>>(),
  )
  .unwrap();

  assert_eq!(
    parse_caddy_version(b"v2.11.3 h1:fixture\n").unwrap(),
    Version::new(2, 11, 3)
  );
  assert_eq!(parse_caddy_modules(&output).unwrap(), modules);
}

#[test]
fn trusted_caddy_source_invalid_higher_priority_config_fails_without_fallback() {
  let dir = tempfile::tempdir().unwrap();
  let system = dir.path().join(exe_name_for_test("system-caddy"));
  write_file(&system);
  let user_config = dir.path().join("user.toml");
  let system_config = dir.path().join("system.toml");
  fs::write(&user_config, "[caddy]\nreal_path = 'relative-caddy'\n").unwrap();
  write_real_caddy_config(&system_config, &system);
  let resolver = RealCaddyResolver::with_test_sources(
    None,
    TrustedConfigPaths {
      user: Some(user_config),
      system: Some(system_config),
    },
    None,
  );

  let error = resolver.resolve().unwrap_err();

  assert!(format!("{error:#}").contains("relative real-Caddy path"));
}

#[cfg(windows)]
#[test]
fn trusted_caddy_source_broken_user_config_link_fails_without_fallback() {
  use std::os::windows::fs::symlink_file;

  let dir = tempfile::tempdir().unwrap();
  let system = dir.path().join(exe_name_for_test("system-caddy"));
  write_file(&system);
  let user_config = dir.path().join("user.toml");
  let missing_target = dir.path().join("missing-user.toml");
  symlink_file(&missing_target, &user_config).expect("create broken config link fixture");
  let system_config = dir.path().join("system.toml");
  write_real_caddy_config(&system_config, &system);
  let resolver = RealCaddyResolver::with_test_sources(
    None,
    TrustedConfigPaths {
      user: Some(user_config),
      system: Some(system_config),
    },
    None,
  );

  let error = resolver.resolve().unwrap_err();

  assert!(format!("{error:#}").contains("canonicalize test configuration"));
}

#[test]
fn trusted_caddy_source_is_pinned_after_first_resolution() {
  let _guard = lock_env();
  let _snapshot = EnvSnapshot::capture(&["PATH"]);
  let dir = tempfile::tempdir().unwrap();
  let first_dir = dir.path().join("first");
  let second_dir = dir.path().join("second");
  fs::create_dir_all(&first_dir).unwrap();
  fs::create_dir_all(&second_dir).unwrap();
  let first = first_dir.join(exe_name_for_test("caddy"));
  let second = second_dir.join(exe_name_for_test("caddy"));
  write_file(&first);
  write_file(&second);
  unsafe {
    env::set_var("PATH", &first_dir);
  }
  let resolver = RealCaddyResolver::with_test_sources(
    None,
    TrustedConfigPaths {
      user: None,
      system: None,
    },
    None,
  );

  assert_eq!(resolver.resolve().unwrap(), canonical(&first));
  unsafe {
    env::set_var("PATH", &second_dir);
  }
  assert_eq!(resolver.resolve().unwrap(), canonical(&first));
}

#[test]
fn trusted_caddy_source_uses_portable_configuration_and_ignores_project_and_environment_selectors()
{
  let _guard = lock_env();
  let _snapshot = EnvSnapshot::capture(&[
    "PATH",
    "CADDER_CADDY_REAL_COMMAND",
    "CADDER_CADDY__REAL_COMMAND",
  ]);
  let dir = tempfile::tempdir().unwrap();
  let project = dir.path().join("project");
  let bin = dir.path().join("bin");
  let path_dir = dir.path().join("path");
  fs::create_dir_all(&project).unwrap();
  fs::create_dir_all(&bin).unwrap();
  fs::create_dir_all(&path_dir).unwrap();
  let rejected = dir.path().join(exe_name_for_test("rejected"));
  let selected = path_dir.join(exe_name_for_test("caddy"));
  write_file(&rejected);
  write_file(&selected);
  write_real_caddy_config(&project.join(CONFIG_FILE_NAME), &rejected);
  write_real_caddy_config(&bin.join(CONFIG_FILE_NAME), &rejected);
  unsafe {
    env::set_var("CADDER_CADDY_REAL_COMMAND", &rejected);
    env::set_var("CADDER_CADDY__REAL_COMMAND", &rejected);
    env::set_var("PATH", env::join_paths([path_dir]).unwrap());
  }
  let resolver = RealCaddyResolver::with_test_sources(
    None,
    TrustedConfigPaths {
      user: None,
      system: None,
    },
    Some(bin.join(exe_name_for_test("cadderd"))),
  );

  assert_eq!(resolver.resolve().unwrap(), canonical(&rejected));
}

#[tokio::test]
async fn mock_adapter_prepares_domains_without_running_caddy_adapt() {
  let dir = tempfile::tempdir().unwrap();
  let config_path = dir.path().join("Caddyfile");
  fs::write(
    &config_path,
    r#"
app.localhost, http://api.localhost:8080 {
reverse_proxy [::1]:4200
}
"#,
  )
  .unwrap();
  let mut registration = registration("project", &[]);
  registration.source_config_path = SourcePath::new(config_path.display().to_string(), None);

  let prepared = MockCaddyConfigAdapter.prepare(registration).await;

  assert!(prepared.diagnostics.is_empty(), "{prepared:?}");
  assert_eq!(
    prepared
      .registration
      .registered_domains
      .iter()
      .map(|domain| domain.name.canonical.as_str())
      .collect::<Vec<_>>(),
    vec!["api.localhost", "app.localhost"]
  );
  assert_eq!(prepared.routes.len(), 2);
  assert_eq!(
    prepared
      .registration
      .registered_domains
      .iter()
      .map(|domain| domain.upstream.as_deref())
      .collect::<Vec<_>>(),
    vec![Some("[::1]:4200"), Some("[::1]:4200")]
  );
}

#[tokio::test]
async fn mock_adapter_reports_invalid_caddyfile() {
  let dir = tempfile::tempdir().unwrap();
  let config_path = dir.path().join("Caddyfile");
  fs::write(&config_path, "app.localhost {\n").unwrap();
  let mut registration = registration("project", &[]);
  registration.source_config_path = SourcePath::new(config_path.display().to_string(), None);

  let prepared = MockCaddyConfigAdapter.prepare(registration).await;

  assert!(prepared.routes.is_empty());
  assert_eq!(prepared.diagnostics.len(), 1);
  assert_eq!(prepared.diagnostics[0].code, "adapt-failed");
  assert!(
    prepared.diagnostics[0]
      .message
      .contains("parse Caddyfile for mock backend")
  );
}

#[tokio::test]
async fn mock_coordinator_applies_effective_config_without_real_caddy_process() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  let mut coordinator = CaddyConfigCoordinator::new_mock(paths.clone());
  let logs = CaddyLogStore::new(20, 20);
  let registration = registration("project", &["app.localhost"]);
  let prepared = PreparedRegistration {
    registration: registration.clone(),
    routes: vec![mock_route_for_domain("app.localhost")],
    diagnostics: Vec::new(),
  };
  coordinator.commit_prepared_registration("Caddyfile".to_string(), prepared);

  let state = coordinator.apply(&[registration], &logs).await;
  let runtime = coordinator.runtime_state().await;

  assert_eq!(state.status, ConfigApplyStatus::Applied);
  assert_eq!(runtime.status, cadder_ipc::RuntimeStatus::Running);
  assert_eq!(runtime.binary_path.as_deref(), Some("mock-caddy"));
  assert!(paths.effective_config_path().is_file());
}

#[tokio::test]
async fn adapter_uses_canonical_config_path_without_shim_metadata() {
  let dir = tempfile::tempdir().unwrap();
  let fake_caddy = dir.path().join(fake_caddy_name_for_test());
  write_fake_caddy(&fake_caddy);
  let project_cwd = dir.path().join("project");
  fs::create_dir_all(&project_cwd).unwrap();
  let config_path = project_cwd.join("Caddyfile");
  fs::write(&config_path, "project.localhost { respond ok }").unwrap();
  let mut registration = registration("project", &[]);
  registration.source_working_directory = SourcePath::new(
    project_cwd.display().to_string(),
    Some(project_cwd.canonicalize().unwrap().display().to_string()),
  );
  registration.source_config_path = SourcePath::new(
    config_path.display().to_string(),
    Some(config_path.canonicalize().unwrap().display().to_string()),
  );

  let adapter = CaddyConfigAdapter::new(RealCaddyResolver::for_test_fixture(fake_caddy));
  let prepared = adapter.prepare(registration).await;

  assert!(prepared.diagnostics.is_empty(), "{prepared:?}");
  assert!(prepared.registration.shim_run.is_none());
  assert_eq!(prepared.routes.len(), 1);
  assert_eq!(
    prepared.registration.registered_domains[0].name.canonical,
    "project.localhost"
  );
}

#[tokio::test]
async fn pinned_caddy_image_adapt_prevents_or_rejects_mutation_after_pinning() {
  let dir = tempfile::tempdir().unwrap();
  let fake_caddy = dir.path().join(fake_caddy_name_for_test());
  write_fake_caddy(&fake_caddy);
  let config_path = dir.path().join("Caddyfile");
  fs::write(&config_path, "project.localhost { respond ok }").unwrap();
  let mut registration = registration("project", &[]);
  registration.source_config_path = SourcePath::new(
    config_path.display().to_string(),
    Some(config_path.canonicalize().unwrap().display().to_string()),
  );
  let adapter = CaddyConfigAdapter::new(RealCaddyResolver::for_test_fixture(fake_caddy.clone()));

  let accepted = adapter.prepare(registration.clone()).await;
  assert!(accepted.diagnostics.is_empty(), "{accepted:?}");
  match fs::write(&fake_caddy, b"modified Caddy image") {
    Ok(()) => {
      let rejected = adapter.prepare(registration).await;
      assert_eq!(rejected.diagnostics[0].code, "adapt-failed");
      assert!(rejected.diagnostics[0].message.contains("digest changed"));
    }
    Err(error) => {
      #[cfg(not(windows))]
      panic!("unexpected image mutation failure: {error}");
      #[cfg(windows)]
      let _ = error;
    }
  }
}

#[tokio::test]
async fn prepare_registration_commits_routes_on_success() {
  let dir = tempfile::tempdir().unwrap();
  let fake_caddy = dir.path().join(fake_caddy_name_for_test());
  write_fake_caddy(&fake_caddy);
  let config_path = dir.path().join("Caddyfile");
  fs::write(&config_path, "project.localhost { respond ok }").unwrap();
  let resolver = RealCaddyResolver::for_test_fixture(fake_caddy);
  let adapter = CaddyConfigAdapter::new(resolver.clone());
  let paths = RuntimePaths::resolve(Some(dir.path().join("run"))).unwrap();
  paths.ensure_dirs().unwrap();
  let runtime = ProcessRuntime::new(resolver, paths);
  let mut coordinator = CaddyConfigCoordinator::new(adapter, runtime);
  let mut registration = registration("project", &[]);
  registration.source_config_path = SourcePath::new(
    config_path.display().to_string(),
    Some(config_path.display().to_string()),
  );

  let prepared = coordinator.prepare_registration(registration).await;

  assert_eq!(
    prepared.registered_domains.len(),
    1,
    "registration diagnostics: {:?}",
    coordinator.registration_diagnostics
  );
  assert_eq!(
    prepared.registered_domains[0].name.canonical,
    "project.localhost"
  );
  assert_eq!(coordinator.routes["project"].len(), 1);
  assert!(!coordinator.registration_diagnostics.contains_key("project"));
}

#[tokio::test]
async fn adapter_reports_invalid_real_caddy_override() {
  let dir = tempfile::tempdir().unwrap();
  let mut registration = registration("project", &[]);
  registration.source_config_path = SourcePath::new(
    dir.path().join("missing.Caddyfile").display().to_string(),
    None,
  );
  let adapter = CaddyConfigAdapter::new(RealCaddyResolver::with_executable_path(
    Some("definitely-missing-caddy".to_string()),
    Some(dir.path().join(exe_name_for_test("cadderd"))),
  ));

  let prepared = adapter.prepare(registration).await;

  assert_eq!(prepared.diagnostics[0].code, "adapt-failed");
  assert!(
    prepared.diagnostics[0]
      .message
      .contains("relative real-Caddy path")
  );
  assert!(prepared.routes.is_empty());
}

#[tokio::test]
async fn adapter_reports_caddy_adapt_failure_and_invalid_json() {
  let dir = tempfile::tempdir().unwrap();
  let config_path = dir.path().join("Caddyfile");
  fs::write(&config_path, "app.localhost { respond ok }").unwrap();
  let failing_caddy = dir.path().join(fake_caddy_name_for_test());
  write_fake_caddy_with_adapt(&failing_caddy, "adapt failed", 7);
  let adapter = CaddyConfigAdapter::new(RealCaddyResolver::with_executable_path(
    Some(failing_caddy.display().to_string()),
    Some(dir.path().join(exe_name_for_test("cadderd"))),
  ));
  let mut registration = registration("project", &[]);
  registration.source_config_path = SourcePath::new(
    config_path.display().to_string(),
    Some(config_path.display().to_string()),
  );

  let failed = adapter.prepare(registration.clone()).await;
  assert_eq!(failed.diagnostics[0].code, "adapt-failed");
  assert!(failed.diagnostics[0].message.contains("adapt failed"));

  let invalid_dir = dir.path().join("invalid");
  fs::create_dir(&invalid_dir).unwrap();
  let invalid_caddy = invalid_dir.join(fake_caddy_name_for_test());
  write_fake_caddy_with_adapt(&invalid_caddy, "not-json", 0);
  let invalid_adapter = CaddyConfigAdapter::new(RealCaddyResolver::with_executable_path(
    Some(invalid_caddy.display().to_string()),
    Some(dir.path().join(exe_name_for_test("cadderd"))),
  ));
  let invalid = invalid_adapter.prepare(registration).await;
  assert_eq!(invalid.diagnostics[0].code, "adapt-failed");
  assert!(invalid.diagnostics[0].message.contains("parse adapted"));
}

#[tokio::test]
async fn adapter_reports_adapt_timeout() {
  let dir = tempfile::tempdir().unwrap();
  let config_path = dir.path().join("Caddyfile");
  fs::write(&config_path, "app.localhost { respond ok }").unwrap();
  let slow_caddy = dir.path().join(fake_caddy_name_for_test());
  write_slow_fake_caddy(&slow_caddy);
  let adapter = CaddyConfigAdapter::with_command_timeout(
    RealCaddyResolver::with_executable_path(
      Some(slow_caddy.display().to_string()),
      Some(dir.path().join(exe_name_for_test("cadderd"))),
    ),
    Duration::from_millis(250),
  );
  let mut registration = registration("project", &[]);
  registration.source_config_path = SourcePath::new(
    config_path.display().to_string(),
    Some(config_path.display().to_string()),
  );

  let prepared = adapter.prepare(registration).await;

  assert_eq!(prepared.diagnostics[0].code, "adapt-failed");
  assert!(
    prepared.diagnostics[0]
      .message
      .contains("timed out after 250 ms")
  );
}

#[test]
fn trusted_caddy_source_resolution_help_names_only_explicit_sources() {
  let resolver = RealCaddyResolver::with_test_sources(
    Some(PathBuf::from("relative-caddy")),
    TrustedConfigPaths {
      user: None,
      system: None,
    },
    None,
  );
  let error = resolver.resolve().unwrap_err();
  let help = RealCaddyResolver::resolution_help(&error);

  assert!(help.contains("absolute --real-caddy daemon-start override"));
  assert!(help.contains("Project files"));
  assert!(!help.contains("CADDER_CADDY_REAL_COMMAND"));
}

#[test]
fn trusted_caddy_source_reports_missing_path_without_implicit_alias() {
  let _guard = lock_env();
  let _snapshot = EnvSnapshot::capture(&["PATH"]);
  unsafe {
    env::remove_var("PATH");
  }
  let resolver = RealCaddyResolver::with_executable_path(None, None);

  let error = resolver.resolve().unwrap_err();

  assert!(format!("{error:#}").contains("PATH is not set"));
}

#[test]
fn trusted_caddy_source_rejects_explicit_shim_by_file_identity() {
  let dir = tempfile::tempdir().unwrap();
  let shim = dir.path().join(exe_name_for_test("caddy"));
  write_file(&shim);
  let resolver = RealCaddyResolver::with_test_sources(
    Some(shim.clone()),
    TrustedConfigPaths {
      user: None,
      system: None,
    },
    Some(shim),
  );

  let error = resolver.resolve().unwrap_err();

  assert!(error.to_string().contains("Cadder Caddy shim"));
}

#[test]
fn trusted_caddy_source_path_does_not_guess_the_caddy_real_executable_name() {
  let _guard = lock_env();
  let _snapshot = EnvSnapshot::capture(&["PATH"]);
  let dir = tempfile::tempdir().unwrap();
  let caddy_real = dir.path().join(exe_name_for_test("caddy-real"));
  write_file(&caddy_real);
  unsafe {
    env::set_var("PATH", dir.path());
  }
  let resolver = RealCaddyResolver::with_executable_path(None, None);

  let error = resolver.resolve().unwrap_err();

  assert!(format!("{error:#}").contains("trusted executable `caddy` not found"));
}

#[test]
fn trusted_caddy_source_rejects_configured_command_arguments() {
  let error = resolve_command_on_path("caddy-real --version", &[], CaddyTrustPolicy::TestFixture)
    .unwrap_err();

  assert!(error.to_string().contains("single program name"));
}

#[test]
fn trusted_caddy_source_portable_command_selects_the_named_path_executable() {
  let _guard = lock_env();
  let _snapshot = EnvSnapshot::capture(&["PATH"]);
  let dir = tempfile::tempdir().unwrap();
  let portable_dir = dir.path().join("portable");
  let path_dir = dir.path().join("path");
  fs::create_dir_all(&portable_dir).unwrap();
  fs::create_dir_all(&path_dir).unwrap();
  let caddy_real = path_dir.join(exe_name_for_test("caddy-real"));
  write_file(&caddy_real);
  fs::write(
    portable_dir.join(CONFIG_FILE_NAME),
    "[caddy]\nreal_command = \"caddy-real\"\n",
  )
  .unwrap();
  unsafe {
    env::set_var("PATH", &path_dir);
  }
  let resolver = RealCaddyResolver::with_test_sources(
    None,
    TrustedConfigPaths {
      user: None,
      system: None,
    },
    Some(portable_dir.join(exe_name_for_test("cadderd"))),
  );

  assert_eq!(resolver.resolve().unwrap(), canonical(&caddy_real));
}

#[test]
fn trusted_caddy_source_path_skips_shim_identity_and_uses_next_candidate() {
  let _guard = lock_env();
  let _snapshot = EnvSnapshot::capture(&["PATH"]);
  let dir = tempfile::tempdir().unwrap();
  let shim_dir = dir.path().join("shim");
  let real_dir = dir.path().join("real");
  fs::create_dir_all(&shim_dir).unwrap();
  fs::create_dir_all(&real_dir).unwrap();
  let shim = shim_dir.join(exe_name_for_test("caddy"));
  let real = real_dir.join(exe_name_for_test("caddy"));
  write_file(&shim);
  write_file(&real);
  let path_var = env::join_paths([shim_dir.as_path(), real_dir.as_path()]).unwrap();
  unsafe {
    env::set_var("PATH", path_var);
  }
  let resolver = RealCaddyResolver::with_test_sources(
    None,
    TrustedConfigPaths {
      user: None,
      system: None,
    },
    Some(shim),
  );

  let resolved = resolver.resolve().unwrap();

  assert_eq!(resolved, canonical(&real));
}

#[cfg(windows)]
#[test]
fn trusted_caddy_source_path_rejects_scoop_wrapper_for_the_cadder_shim() {
  let _guard = lock_env();
  let _snapshot = EnvSnapshot::capture(&["PATH"]);
  let dir = tempfile::tempdir().unwrap();
  let shim_dir = dir.path().join("shim");
  let wrapper_dir = dir.path().join("wrapper");
  fs::create_dir_all(&shim_dir).unwrap();
  fs::create_dir_all(&wrapper_dir).unwrap();
  let shim = shim_dir.join(exe_name_for_test("caddy"));
  let wrapper = wrapper_dir.join(exe_name_for_test("caddy"));
  write_file(&shim);
  write_file(&wrapper);
  fs::write(
    wrapper.with_extension("shim"),
    format!("path = \"{}\"\n", shim.display()),
  )
  .unwrap();
  unsafe {
    env::set_var("PATH", wrapper_dir);
  }
  let resolver = RealCaddyResolver::with_test_sources(
    None,
    TrustedConfigPaths {
      user: None,
      system: None,
    },
    Some(shim),
  );

  let error = resolver.resolve().unwrap_err();

  assert!(format!("{error:#}").contains("trusted executable `caddy` not found"));
}

#[cfg(windows)]
#[test]
fn trusted_caddy_source_portable_command_resolves_scoop_target() {
  let _guard = lock_env();
  let _snapshot = EnvSnapshot::capture(&["PATH"]);
  let dir = tempfile::tempdir().unwrap();
  let wrapper_dir = dir.path().join("wrapper");
  let native_dir = dir.path().join("native");
  fs::create_dir_all(&wrapper_dir).unwrap();
  fs::create_dir_all(&native_dir).unwrap();
  let wrapper = wrapper_dir.join(exe_name_for_test("caddy-real"));
  let native = native_dir.join(exe_name_for_test("caddy"));
  write_file(&wrapper);
  write_file(&native);
  fs::write(
    wrapper.with_extension("shim"),
    format!("path = \"{}\"\n", native.display()),
  )
  .unwrap();
  unsafe {
    env::set_var("PATH", wrapper_dir);
  }
  let portable_dir = dir.path().join("portable");
  fs::create_dir_all(&portable_dir).unwrap();
  fs::write(
    portable_dir.join(CONFIG_FILE_NAME),
    "[caddy]\nreal_command = \"caddy-real\"\n",
  )
  .unwrap();
  let resolver = RealCaddyResolver::with_test_sources(
    None,
    TrustedConfigPaths {
      user: None,
      system: None,
    },
    Some(portable_dir.join(exe_name_for_test("cadderd"))),
  );

  assert_eq!(resolver.resolve().unwrap(), canonical(&native));
}

#[test]
fn trusted_caddy_source_ignores_legacy_shim_path_environment_override() {
  let _guard = lock_env();
  let _snapshot = EnvSnapshot::capture(&["PATH", "CADDER_CADDY_SHIM_PATH"]);
  let dir = tempfile::tempdir().unwrap();
  let real = dir.path().join(exe_name_for_test("caddy"));
  write_file(&real);
  unsafe {
    env::set_var("CADDER_CADDY_SHIM_PATH", &real);
    env::set_var("PATH", dir.path());
  }
  let resolver = RealCaddyResolver::with_executable_path(None, None);

  let resolved = resolver.resolve().unwrap();

  assert_eq!(resolved, canonical(&real));
}

#[cfg(windows)]
fn exe_name_for_test(name: &str) -> String {
  format!("{name}.exe")
}

#[cfg(not(windows))]
fn exe_name_for_test(name: &str) -> String {
  name.to_string()
}

#[cfg(windows)]
fn fake_caddy_name_for_test() -> &'static str {
  "fake-caddy.cmd"
}

#[cfg(not(windows))]
fn fake_caddy_name_for_test() -> &'static str {
  "fake-caddy"
}

struct CoordinatorFixture {
  coordinator: CaddyConfigCoordinator,
  _temp: tempfile::TempDir,
}

impl std::ops::Deref for CoordinatorFixture {
  type Target = CaddyConfigCoordinator;

  fn deref(&self) -> &Self::Target {
    &self.coordinator
  }
}

impl std::ops::DerefMut for CoordinatorFixture {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.coordinator
  }
}

fn coordinator_for_test() -> CoordinatorFixture {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("run"))).unwrap();
  paths.ensure_dirs().unwrap();
  let resolver = RealCaddyResolver::with_executable_path(
    Some("missing-caddy".to_string()),
    Some(temp.path().join(exe_name_for_test("cadderd"))),
  );
  let adapter = CaddyConfigAdapter::new(resolver.clone());
  let runtime = ProcessRuntime::new(resolver, paths);
  CoordinatorFixture {
    coordinator: CaddyConfigCoordinator::new(adapter, runtime),
    _temp: temp,
  }
}

fn diagnostic(code: &str) -> ConfigDiagnostic {
  ConfigDiagnostic {
    code: code.to_string(),
    message: format!("{code} message"),
    domain_key: None,
    source_config_paths: Vec::new(),
  }
}

#[test]
fn commit_prepared_registration_records_diagnostics_and_removes_routes() {
  let mut coordinator = coordinator_for_test();
  coordinator.routes.insert(
    "shim".to_string(),
    vec![json!({ "match": [{ "host": ["app.localhost"] }] })],
  );
  let prepared = PreparedRegistration {
    registration: registration("shim", &["app.localhost"]),
    routes: vec![json!({ "match": [{ "host": ["app.localhost"] }] })],
    diagnostics: vec![diagnostic("adapt-failed")],
  };

  coordinator.commit_prepared_registration("shim.Caddyfile".to_string(), prepared);

  assert!(!coordinator.routes.contains_key("shim"));
  let diagnostics = &coordinator.registration_diagnostics["shim"];
  assert_eq!(diagnostics[0].source_config_paths, ["shim.Caddyfile"]);
  assert_eq!(coordinator.current.status, ConfigApplyStatus::Failed);
}

#[test]
fn begin_apply_returns_current_failed_state_for_enabled_diagnostics() {
  let mut coordinator = coordinator_for_test();
  coordinator.registration_diagnostics.insert(
    "shim".to_string(),
    vec![ConfigDiagnostic {
      source_config_paths: vec!["shim.Caddyfile".to_string()],
      ..diagnostic("adapt-failed")
    }],
  );

  let action = coordinator.begin_apply(&[registration("shim", &["app.localhost"])]);

  let CaddyApplyAction::Current(state) = action else {
    panic!("expected current failed state");
  };
  assert_eq!(state.status, ConfigApplyStatus::Failed);
  assert_eq!(state.diagnostics[0].code, "adapt-failed");
}

#[test]
fn begin_apply_returns_stop_when_no_routes_are_active() {
  let mut coordinator = coordinator_for_test();

  let action = coordinator.begin_apply(&[registration("shim", &[])]);

  assert!(matches!(action, CaddyApplyAction::Stop { .. }));
}

#[test]
fn begin_apply_builds_rendered_config_and_prunes_stale_state() {
  let mut coordinator = coordinator_for_test();
  coordinator.routes.insert(
    "stale".to_string(),
    vec![json!({ "match": [{ "host": ["stale.localhost"] }] })],
  );
  coordinator
    .registration_diagnostics
    .insert("stale".to_string(), vec![diagnostic("adapt-failed")]);

  let action = coordinator.begin_apply(&[registration("shim", &["app.localhost"])]);

  let CaddyApplyAction::Apply {
    rendered,
    hash,
    source_config_paths,
    ..
  } = action
  else {
    panic!("expected apply action");
  };
  let config: Value = serde_json::from_slice(&rendered).unwrap();
  assert!(!hash.is_empty());
  assert_eq!(source_config_paths, ["shim.Caddyfile"]);
  assert_eq!(
    config
      .pointer("/apps/http/servers/cadder_https/routes/0/match/0/host/0")
      .and_then(Value::as_str),
    Some("app.localhost")
  );
  assert!(coordinator.routes.is_empty());
  assert!(coordinator.registration_diagnostics.is_empty());
}

#[test]
fn compose_config_uses_placeholder_route_when_adapted_routes_are_missing() {
  let registrations = vec![registration("shim", &["app.localhost"])];
  let config = compose_config(&registrations, &BTreeMap::new());

  assert_eq!(
    config
      .pointer("/apps/http/servers/cadder_https/routes/0/handle/0/body")
      .and_then(Value::as_str),
    Some("Cadder route placeholder")
  );
  assert_eq!(
    config
      .pointer("/apps/tls/automation/policies/0/subjects/0")
      .and_then(Value::as_str),
    Some("app.localhost")
  );
}

#[test]
fn compose_config_drops_routes_for_disabled_domains() {
  let mut registration = registration("shim", &["app.localhost", "api.localhost"]);
  registration.registered_domains[1].activation_state = ActivationState::Inactive;
  let routes_by_registration = BTreeMap::from([(
    "shim".to_string(),
    vec![json!({
        "match": [{ "host": ["app.localhost", "api.localhost"] }],
        "handle": [{ "handler": "static_response", "body": "mixed" }],
        "terminal": true
    })],
  )]);

  let config = compose_config(&[registration], &routes_by_registration);
  let hosts = config
    .pointer("/apps/http/servers/cadder_https/routes/0/match/0/host")
    .and_then(Value::as_array)
    .unwrap();
  let tls_subjects = config
    .pointer("/apps/tls/automation/policies/0/subjects")
    .and_then(Value::as_array)
    .unwrap();

  assert_eq!(hosts, &[json!("app.localhost")]);
  assert_eq!(tls_subjects, &[json!("app.localhost")]);
}

#[test]
fn finish_apply_state_updates_success_failure_and_idle() {
  let mut coordinator = coordinator_for_test();
  let attempted = Utc::now();

  let applied = coordinator.finish_runtime_apply(attempted, "hash-1".to_string(), vec![], Ok(()));
  assert_eq!(applied.status, ConfigApplyStatus::Applied);
  assert_eq!(applied.effective_config_hash.as_deref(), Some("hash-1"));

  let failed = coordinator.finish_runtime_apply(
    attempted,
    "hash-2".to_string(),
    vec!["shim.Caddyfile".to_string()],
    Err(anyhow::anyhow!("runtime exploded")),
  );
  assert_eq!(failed.status, ConfigApplyStatus::Failed);
  assert_eq!(failed.effective_config_hash.as_deref(), Some("hash-1"));
  assert_eq!(failed.diagnostics[0].code, "runtime-apply-failed");
  assert_eq!(
    failed.diagnostics[0].source_config_paths,
    ["shim.Caddyfile"]
  );

  let idle = coordinator.finish_idle(attempted);
  assert_eq!(idle.status, ConfigApplyStatus::Idle);
  assert_eq!(idle.effective_config_hash, None);
  assert!(idle.diagnostics.is_empty());
}

#[tokio::test]
async fn public_control_types_keep_clone_debug_and_accessor_contracts() {
  let resolver = RealCaddyResolver::from_trusted_sources();
  let adapter = CaddyConfigAdapter::new(resolver.clone());
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().to_path_buf())).unwrap();
  let runtime = ProcessRuntime::new(resolver.clone(), paths);
  let coordinator = CaddyConfigCoordinator::new(adapter.clone(), runtime.clone());
  let attempted = Utc::now();
  let apply_action = CaddyApplyAction::Apply {
    attempted,
    rendered: br#"{"apps":{}}"#.to_vec(),
    hash: "hash".to_string(),
    source_config_paths: vec!["Caddyfile".to_string()],
  };
  let stop_action = CaddyApplyAction::Stop { attempted };
  let current_action = CaddyApplyAction::Current(ConfigState::idle());
  let prepared = PreparedRegistration {
    registration: registration("shim", &["app.localhost"]),
    routes: vec![json!({ "match": [{ "host": ["app.localhost"] }] })],
    diagnostics: Vec::new(),
  };

  assert!(format!("{:?}", resolver.clone()).contains("RealCaddyResolver"));
  assert!(format!("{:?}", adapter.clone()).contains("CaddyConfigAdapter"));
  assert!(format!("{:?}", prepared.clone()).contains("PreparedRegistration"));
  assert!(format!("{:?}", apply_action).contains("Apply"));
  assert!(format!("{:?}", stop_action).contains("Stop"));
  assert!(format!("{:?}", current_action).contains("Current"));
  assert_eq!(coordinator.current_state().status, ConfigApplyStatus::Idle);
  assert_eq!(
    coordinator.runtime_state().await.status,
    cadder_ipc::RuntimeStatus::Idle
  );
  let _ = coordinator.adapter();
  let _ = coordinator.runtime();
}

#[tokio::test]
async fn apply_wrapper_handles_current_stop_and_runtime_failure_actions() {
  let logs = CaddyLogStore::default();
  let mut current = coordinator_for_test();
  current.registration_diagnostics.insert(
    "shim".to_string(),
    vec![ConfigDiagnostic {
      source_config_paths: vec!["shim.Caddyfile".to_string()],
      ..diagnostic("adapt-failed")
    }],
  );

  let current_state = current
    .apply(&[registration("shim", &["app.localhost"])], &logs)
    .await;
  let mut idle = coordinator_for_test();
  let idle_state = idle.apply(&[registration("shim", &[])], &logs).await;
  let mut failed = coordinator_for_test();
  let failed_state = failed
    .apply(&[registration("shim", &["app.localhost"])], &logs)
    .await;
  let shutdown = failed.shutdown().await;

  assert_eq!(current_state.status, ConfigApplyStatus::Failed);
  assert_eq!(idle_state.status, ConfigApplyStatus::Idle);
  assert_eq!(failed_state.status, ConfigApplyStatus::Failed);
  assert_eq!(failed_state.diagnostics[0].code, "runtime-apply-failed");
  assert!(shutdown.is_ok());
}

#[tokio::test]
async fn apply_wrapper_logs_stop_error_when_idling_running_runtime() {
  let dir = tempfile::tempdir().unwrap();
  let fake_caddy = dir.path().join(fake_caddy_name_for_test());
  write_runtime_fake_caddy(&fake_caddy);
  let resolver = RealCaddyResolver::with_executable_path(
    Some(fake_caddy.display().to_string()),
    Some(dir.path().join(exe_name_for_test("cadderd"))),
  );
  let adapter = CaddyConfigAdapter::new(resolver.clone());
  let paths = RuntimePaths::resolve(Some(dir.path().join("run"))).unwrap();
  paths.ensure_dirs().unwrap();
  let runtime = ProcessRuntime::with_timeouts(
    resolver,
    paths,
    RuntimeTimeouts {
      start_check: Duration::from_millis(150),
      reload: Duration::from_secs(3),
      graceful_stop: Duration::from_secs(1),
      stop_wait: Duration::from_secs(1),
      kill_wait: Duration::from_secs(2),
    },
  );
  let logs = CaddyLogStore::default();
  let mut coordinator = CaddyConfigCoordinator::new(adapter, runtime);
  let active = registration("shim", &["project.localhost"]);
  let started = coordinator
    .apply(std::slice::from_ref(&active), &logs)
    .await;
  let idle = coordinator.apply(&[registration("shim", &[])], &logs).await;
  let runtime_logs = logs
    .query(
      crate::logs::LogQuery {
        stream: cadder_ipc::LogStreamIdentity::runtime_control(),
        limit: 20,
      },
      true,
    )
    .await;

  assert_eq!(started.status, ConfigApplyStatus::Applied);
  assert_eq!(idle.status, ConfigApplyStatus::Idle);
  assert!(
    runtime_logs.entries.iter().any(|entry| {
      entry.operation.as_deref() == Some("idle-stop")
        && entry.raw_message.contains("caddy stop timed out")
    }),
    "{runtime_logs:#?}"
  );
}

#[test]
fn extracts_hosts_from_adapted_json() {
  let adapted = json!({
      "apps": {
          "http": {
              "servers": {
                  "srv0": {
                      "routes": [
                          { "match": [{ "host": ["App.Localhost", "api.localhost"] }] }
                      ]
                  }
              }
          }
      }
  });

  let hosts = extract_hosts(&adapted);
  assert!(hosts.contains("app.localhost"));
  assert!(hosts.contains("api.localhost"));
}

#[test]
fn extracts_reverse_proxy_upstream_for_registered_domain() {
  let adapted = json!({
      "apps": {
          "http": {
              "servers": {
                  "srv0": {
                      "routes": [{
                          "match": [{ "host": ["App.Localhost"] }],
                          "handle": [{
                              "handler": "reverse_proxy",
                              "upstreams": [{ "dial": "127.0.0.1:19087" }]
                          }],
                          "terminal": true
                      }]
                  }
              }
          }
      }
  });

  let domains = extract_registered_domains(&adapted);

  assert_eq!(domains.len(), 1);
  assert_eq!(domains[0].name.canonical, "app.localhost");
  assert_eq!(domains[0].upstream.as_deref(), Some("127.0.0.1:19087"));
}

#[test]
fn extraction_and_filtering_helpers_handle_empty_shapes() {
  assert!(extract_http_routes(&json!({ "apps": { "tls": {} } })).is_empty());
  assert!(filter_route_hosts(json!({ "handle": [] }), &BTreeSet::new()).is_none());
}

#[test]
fn detects_active_domain_conflicts() {
  let left = registration("left", &["app.localhost"]);
  let right = registration("right", &["APP.localhost."]);

  let diagnostics = detect_conflicts(&[left, right]);

  assert_eq!(diagnostics.len(), 1);
  assert_eq!(diagnostics[0].domain_key.as_deref(), Some("app.localhost"));
}

#[test]
fn conflict_detection_ignores_inactive_registrations_and_domains() {
  let active = registration("active", &["app.localhost"]);
  let mut inactive_registration = registration("inactive-registration", &["app.localhost"]);
  inactive_registration.activation_state = ActivationState::Inactive;
  let mut inactive_domain = registration("inactive-domain", &["app.localhost"]);
  inactive_domain.registered_domains[0].activation_state = ActivationState::Inactive;

  let diagnostics = detect_conflicts(&[active, inactive_registration, inactive_domain]);

  assert!(diagnostics.is_empty());
}

#[test]
fn filters_routes_to_enabled_hosts() {
  let route = json!({
      "match": [{ "host": ["app.localhost", "api.localhost"] }],
      "handle": [{ "handler": "reverse_proxy" }]
  });
  let hosts = BTreeSet::from(["api.localhost".to_string()]);

  let filtered = filter_route_hosts(route, &hosts).unwrap();

  assert_eq!(
    filtered.pointer("/match/0/host/0").and_then(Value::as_str),
    Some("api.localhost")
  );
  assert_eq!(
    filtered
      .pointer("/match/0/host")
      .and_then(Value::as_array)
      .unwrap()
      .len(),
    1
  );
}
