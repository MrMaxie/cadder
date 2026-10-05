use super::*;

#[tokio::test]
async fn dropped_candidate_does_not_change_effective_config() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  restore_effective_config(&paths, Some(b"previous"))
    .await
    .unwrap();

  let staged = StagedRuntimeConfig::stage(&paths, b"candidate")
    .await
    .unwrap();
  let candidate_path = staged.path().to_path_buf();

  assert_eq!(
    fs::read(paths.effective_config_path()).unwrap(),
    b"previous"
  );
  assert_eq!(fs::read(&candidate_path).unwrap(), b"candidate");
  drop(staged);
  assert!(!candidate_path.exists());
  assert_eq!(
    fs::read(paths.effective_config_path()).unwrap(),
    b"previous"
  );
}

#[tokio::test]
async fn promotion_atomically_replaces_effective_config() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  restore_effective_config(&paths, Some(b"previous"))
    .await
    .unwrap();

  let mut staged = StagedRuntimeConfig::stage(&paths, b"candidate")
    .await
    .unwrap();
  let candidate_path = staged.path().to_path_buf();
  staged.promote().unwrap();

  assert!(!candidate_path.exists());
  assert_eq!(
    fs::read(paths.effective_config_path()).unwrap(),
    b"candidate"
  );

  #[cfg(windows)]
  crate::ipc_windows_security::validate_owner_only_runtime_file(&paths.effective_config_path())
    .unwrap();
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
      fs::metadata(paths.effective_config_path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777,
      0o600
    );
  }
}

#[test]
fn cleanup_removes_only_validated_exact_candidate_files() {
  let temp = tempfile::tempdir().unwrap();
  let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
  paths.ensure_dirs().unwrap();
  let stale = paths
    .runtime_dir()
    .join(".effective-caddy.00112233445566778899aabbccddeeff.tmp");
  let unrelated = paths
    .runtime_dir()
    .join(".effective-caddy.not-a-generation.tmp");
  let stale_file = create_owner_only_runtime_file(&paths, &stale).unwrap();
  drop(stale_file);
  fs::write(&unrelated, b"keep").unwrap();

  cleanup_stale_config_candidates(&paths).unwrap();

  assert!(!stale.exists());
  assert!(unrelated.exists());
}
