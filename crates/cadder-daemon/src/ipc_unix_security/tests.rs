use super::*;
use interprocess::local_socket::tokio::Listener;
use std::os::unix::fs::symlink;

fn runtime_paths(root: &Path) -> RuntimePaths {
  RuntimePaths::resolve(Some(root.join("runtime"))).unwrap()
}

fn mode(path: &Path) -> u32 {
  fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn unix_ipc_security_runtime_paths_are_owner_only() {
  let root = tempfile::tempdir().unwrap();
  let paths = runtime_paths(root.path());

  secure_runtime_paths(&paths).unwrap();
  fs::set_permissions(paths.runtime_dir(), fs::Permissions::from_mode(0o777)).unwrap();
  secure_runtime_paths(&paths).unwrap();

  assert_eq!(mode(paths.runtime_dir()), OWNER_DIRECTORY_MODE);
  assert_eq!(
    fs::symlink_metadata(paths.runtime_dir()).unwrap().uid(),
    current_euid()
  );
}

#[test]
fn unix_ipc_security_rejects_a_symlinked_runtime_directory() {
  let root = tempfile::tempdir().unwrap();
  let target = root.path().join("target");
  fs::create_dir(&target).unwrap();
  let paths = runtime_paths(root.path());
  symlink(&target, paths.runtime_dir()).unwrap();

  let error = secure_runtime_paths(&paths).unwrap_err();

  assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[tokio::test]
async fn unix_ipc_security_socket_is_owner_only_and_reports_peer_euid() {
  let root = tempfile::tempdir().unwrap();
  let paths = runtime_paths(root.path());
  secure_runtime_paths(&paths).unwrap();
  let name = unix_listener_name(&paths).unwrap();
  assert!(name.is_path());
  let options = secure_listener_options(ListenerOptions::new().name(name.clone())).unwrap();
  let listener: Listener = options.create_tokio().unwrap();
  secure_bound_socket(&paths).unwrap();

  let client = Stream::connect(name).await.unwrap();
  let server = listener.accept().await.unwrap();

  assert_eq!(peer_euid(&server).unwrap(), current_euid());
  assert_eq!(mode(&unix_socket_directory()), OWNER_DIRECTORY_MODE);
  assert_eq!(mode(&unix_socket_path(&paths)), OWNER_FILE_MODE);
  drop((client, server, listener));
  assert!(!unix_socket_path(&paths).exists());
}

#[tokio::test]
async fn unix_ipc_security_long_runtime_path_uses_a_portable_socket_address() {
  let root = tempfile::tempdir().unwrap();
  let long_runtime = root.path().join("x".repeat(200));
  let paths = runtime_paths(&long_runtime);

  let name = unix_listener_name(&paths).unwrap();
  let socket_path = unix_socket_path(&paths);
  let options = secure_listener_options(ListenerOptions::new().name(name.clone())).unwrap();
  let listener: Listener = options.create_tokio().unwrap();
  secure_bound_socket(&paths).unwrap();
  let client = Stream::connect(name).await.unwrap();
  let server = listener.accept().await.unwrap();

  assert!(socket_path.as_os_str().as_bytes().len() <= PORTABLE_SOCKET_PATH_MAX_BYTES);
  assert!(!socket_path.starts_with(paths.runtime_dir()));
  assert_eq!(peer_euid(&server).unwrap(), current_euid());
  assert_eq!(socket_path, unix_socket_path(&runtime_paths(&long_runtime)));
  drop((client, server, listener));
  assert!(!socket_path.exists());
  assert_eq!(mode(&unix_socket_directory()), OWNER_DIRECTORY_MODE);
}

#[test]
fn unix_ipc_security_rejects_a_writable_socket_root_without_sticky_bit() {
  let root = tempfile::tempdir().unwrap();
  fs::set_permissions(root.path(), fs::Permissions::from_mode(0o777)).unwrap();

  let error = validate_socket_root(root.path()).unwrap_err();

  assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn unix_ipc_security_accepts_an_owner_only_socket_root() {
  let root = tempfile::tempdir().unwrap();
  fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();

  validate_socket_root(root.path()).unwrap();
}

#[test]
fn unix_ipc_security_socket_claim_guard_holds_the_recovery_lock() {
  let root = tempfile::tempdir().unwrap();
  let paths = runtime_paths(root.path());
  let guard = SocketClaimGuard::acquire(&paths).unwrap();
  let lock_path = unix_socket_path(&paths).with_extension("reclaim");
  let contender = OpenOptions::new()
    .read(true)
    .write(true)
    .open(lock_path)
    .unwrap();

  assert!(matches!(
    FileExt::try_lock(&contender),
    Err(fs4::TryLockError::WouldBlock)
  ));

  drop(guard);
  FileExt::try_lock(&contender).unwrap();
}
