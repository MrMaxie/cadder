use super::*;

#[test]
fn resolve_override_derives_stable_socket_and_runtime_paths() {
  let dir = tempfile::tempdir().unwrap();

  let first = RuntimePaths::resolve(Some(dir.path().to_path_buf())).unwrap();
  let second = RuntimePaths::resolve(Some(dir.path().to_path_buf())).unwrap();

  assert_eq!(first.runtime_dir(), dir.path());
  assert_eq!(first.storage_paths().profile_dir(), dir.path().join("data"));
  assert_eq!(
    first.storage_paths().lock_path(),
    dir.path().join("data").join("storage.lock")
  );
  assert_eq!(
    first.storage_paths().manifest_path(),
    dir.path().join("data").join("manifest.json")
  );
  assert_eq!(
    first.storage_paths().generations_dir(),
    dir.path().join("data").join("generations")
  );
  assert_eq!(
    first.storage_paths().plans_dir(),
    dir.path().join("data").join("plans")
  );
  assert_eq!(
    first.storage_paths().secrets_dir(),
    dir.path().join("data").join("secrets")
  );
  assert_eq!(
    first.storage_paths().recovery_dir(),
    dir.path().join("data").join("recovery")
  );
  assert_eq!(first.instance_key(), second.instance_key());
  assert_eq!(first.socket_name(), second.socket_name());
  assert!(first.socket_name().starts_with("cadder-"));
  assert_eq!(
    first.effective_config_path(),
    dir.path().join("effective-caddy.json")
  );
}

#[test]
fn explicit_runtime_directory_precedes_environment_directory() {
  let explicit = PathBuf::from("explicit-runtime");
  let environment = PathBuf::from("environment-runtime");

  assert_eq!(
    resolve_runtime_dir(Some(explicit.clone()), Some(environment)).unwrap(),
    explicit
  );
}

#[test]
fn environment_runtime_directory_is_used_without_an_explicit_override() {
  let environment = PathBuf::from("environment-runtime");

  assert_eq!(
    resolve_runtime_dir(None, Some(environment.clone())).unwrap(),
    environment
  );
}

#[test]
fn for_executable_uses_its_parent_as_the_runtime_directory() {
  let dir = tempfile::tempdir().unwrap();
  let executable = dir.path().join("bin").join("cadder.exe");
  let paths = RuntimePaths::for_executable(&executable).unwrap();

  assert_eq!(paths.runtime_dir(), dir.path().join("bin"));
  assert_eq!(
    paths.storage_paths().profile_dir(),
    dir.path().join("bin/data")
  );
}

#[test]
fn runtime_resolution_falls_back_to_the_current_executable_parent() {
  let runtime_dir = resolve_runtime_dir(None, None).unwrap();
  let executable = std::env::current_exe().unwrap();

  assert_eq!(runtime_dir, executable.parent().unwrap());
}

#[test]
fn ensure_dirs_creates_runtime_directory() {
  let dir = tempfile::tempdir().unwrap();
  let runtime_dir = dir.path().join("nested").join("runtime");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();

  paths.ensure_dirs().unwrap();

  assert!(runtime_dir.is_dir());
}
