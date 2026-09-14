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
fn resolve_uses_the_current_executable_parent() {
  let paths = RuntimePaths::resolve(None).unwrap();
  let executable = std::env::current_exe().unwrap();

  assert_eq!(paths.runtime_dir(), executable.parent().unwrap());
}

#[test]
fn ensure_dirs_creates_runtime_directory() {
  let dir = tempfile::tempdir().unwrap();
  let runtime_dir = dir.path().join("nested").join("runtime");
  let paths = RuntimePaths::resolve(Some(runtime_dir.clone())).unwrap();

  paths.ensure_dirs().unwrap();

  assert!(runtime_dir.is_dir());
}
