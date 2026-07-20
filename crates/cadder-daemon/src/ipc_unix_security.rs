//! Unix ownership and peer-authentication primitives for local IPC.

use crate::RuntimePaths;
use fs4::FileExt;
use interprocess::local_socket::{
  GenericFilePath, ListenerOptions, Name, ToFsName,
  tokio::{Stream, prelude::*},
};
use std::{
  fs::{self, DirBuilder, File, OpenOptions},
  io,
  os::unix::ffi::OsStrExt,
  os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
  path::{Path, PathBuf},
};

#[cfg(any(
  target_os = "android",
  target_os = "freebsd",
  target_os = "linux",
  target_os = "openbsd"
))]
use interprocess::os::unix::local_socket::ListenerOptionsExt;

const OWNER_DIRECTORY_MODE: u32 = 0o700;
const OWNER_FILE_MODE: u32 = 0o600;
const PORTABLE_SOCKET_PATH_MAX_BYTES: usize = 103;

/// Serializes only socket recovery while a daemon is claiming its endpoint.
///
/// The advisory lock is rooted next to the Unix socket, never in the runtime
/// directory, and is released by the kernel if its owner exits. It therefore
/// cannot become persistent runtime ownership or block a later daemon.
#[derive(Debug)]
pub(crate) struct SocketClaimGuard(File);

impl SocketClaimGuard {
  pub(crate) fn acquire(paths: &RuntimePaths) -> io::Result<Self> {
    secure_socket_directory()?;
    let path = unix_socket_path(paths).with_extension("reclaim");
    let file = OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .mode(OWNER_FILE_MODE)
      .open(path)?;
    file.lock_exclusive()?;
    Ok(Self(file))
  }
}

/// Creates the runtime and socket transport directories for the effective user.
///
/// Both final paths must be real directories owned by the current effective user. Symlinks and
/// directories owned by another user are rejected before their permissions are changed.
///
/// # Errors
///
/// Returns an error when either directory cannot be created, is a symlink, has an unexpected type
/// or owner, or cannot be restricted to mode `0700`.
pub(crate) fn secure_runtime_paths(paths: &RuntimePaths) -> io::Result<()> {
  let mut builder = DirBuilder::new();
  builder.recursive(true).mode(OWNER_DIRECTORY_MODE);
  builder.create(paths.runtime_dir())?;

  secure_owned_path(
    paths.runtime_dir(),
    ExpectedFileType::Directory,
    OWNER_DIRECTORY_MODE,
  )?;
  secure_socket_directory()
}

/// Creates a new owner-only regular file without following symbolic links.
pub(crate) fn create_owner_only_runtime_file(
  paths: &RuntimePaths,
  path: &Path,
) -> io::Result<File> {
  secure_runtime_paths(paths)?;
  create_owner_only_file(path)
}

/// Opens or creates a persistent owner-only file suitable for advisory locking.
pub(crate) fn open_owner_only_lock_file(paths: &RuntimePaths, path: &Path) -> io::Result<File> {
  secure_runtime_paths(paths)?;

  match create_owner_only_lock_file(path) {
    Ok(file) => Ok(file),
    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
      validate_owned_path(path, ExpectedFileType::RegularFile)?;
      let file = open_existing_lock_file_without_following_symlinks(path)?;
      secure_open_file(&file, path)?;
      Ok(file)
    }
    Err(error) => Err(error),
  }
}

/// Verifies that a runtime file is regular, owner-controlled, and mode `0600`.
pub(crate) fn validate_owner_only_runtime_file(path: &Path) -> io::Result<()> {
  validate_owned_path(path, ExpectedFileType::RegularFile)?;
  validate_mode(&fs::symlink_metadata(path)?, path, OWNER_FILE_MODE)
}

/// Restricts an existing owner-controlled regular runtime file to mode `0600`.
pub(crate) fn secure_owner_only_runtime_file(path: &Path) -> io::Result<()> {
  let file = open_existing_lock_file_without_following_symlinks(path)?;
  secure_open_file(&file, path)
}

/// Opens an existing owner-controlled regular file without following symbolic links.
pub(crate) fn open_owner_only_runtime_file(path: &Path) -> io::Result<File> {
  validate_owned_path(path, ExpectedFileType::RegularFile)?;
  let file = open_existing_lock_file_without_following_symlinks(path)?;
  secure_open_file(&file, path)?;
  Ok(file)
}

/// Restricts an existing owner-controlled directory to mode `0700`.
pub(crate) fn secure_owner_only_directory(path: &Path) -> io::Result<()> {
  secure_owned_path(path, ExpectedFileType::Directory, OWNER_DIRECTORY_MODE)
}

/// Persists a directory entry update on Unix filesystems.
pub(crate) fn sync_parent_directory(path: &Path) -> io::Result<()> {
  let parent = path.parent().ok_or_else(|| {
    io::Error::new(
      io::ErrorKind::InvalidInput,
      "the IPC discovery path does not have a parent directory",
    )
  })?;
  File::open(parent)?.sync_all()
}

/// Returns the owner-only filesystem path used for the Unix domain socket.
///
/// Unix-domain socket addresses are much shorter than normal filesystem paths. The fixed `/tmp`
/// transport root keeps the address below the smallest `sun_path` capacity of the supported Unix
/// platforms even when the data-bearing runtime directory is long. The effective user ID protects
/// profiles owned by different users, while the runtime key keeps profiles distinct.
pub(crate) fn unix_socket_path(paths: &RuntimePaths) -> PathBuf {
  unix_socket_directory().join(paths.socket_name())
}

/// Returns an owned filesystem local-socket name rooted in an owner-only transport directory.
///
/// Using a filesystem name is deliberate: `GenericNamespaced` maps to the Linux abstract namespace
/// and directly into a shared temporary directory on other Unix systems. Cadder instead creates a
/// `0700` per-user directory before returning the endpoint.
///
/// # Errors
///
/// Returns an error when the transport directory is insecure, the generated path exceeds the
/// portable Unix-domain socket limit, or the path is not a valid local-socket name.
pub(crate) fn unix_listener_name(paths: &RuntimePaths) -> io::Result<Name<'static>> {
  secure_socket_directory()?;
  let path = unix_socket_path(paths);
  if path.as_os_str().as_bytes().len() > PORTABLE_SOCKET_PATH_MAX_BYTES {
    return Err(io::Error::new(
      io::ErrorKind::InvalidInput,
      format!(
        "local IPC socket path exceeds the portable {PORTABLE_SOCKET_PATH_MAX_BYTES}-byte limit: {}",
        path.display()
      ),
    ));
  }
  path.to_fs_name::<GenericFilePath>().map(Name::into_owned)
}

/// Applies an atomic pre-bind socket mode on platforms where `interprocess` supports it.
///
/// Call [`secure_bound_socket`] after listener creation on every Unix platform. That verification
/// also supplies the owner-only socket mode on systems where pre-bind `fchmod` is unsupported.
///
/// # Errors
///
/// The error channel is reserved for platform-specific listener hardening failures. Current Unix
/// implementations report mode-setting failures when the listener is created.
pub(crate) fn secure_listener_options(
  options: ListenerOptions<'_>,
) -> io::Result<ListenerOptions<'_>> {
  #[cfg(any(
    target_os = "android",
    target_os = "freebsd",
    target_os = "linux",
    target_os = "openbsd"
  ))]
  let options = options.mode(OWNER_FILE_MODE as libc::mode_t);

  Ok(options)
}

/// Restricts and verifies the bound Unix domain socket as owner-only.
///
/// The socket must be created from [`unix_listener_name`], which first secures its parent
/// directory. This keeps the socket unreachable by other users even on systems that require a
/// post-bind `chmod`.
///
/// # Errors
///
/// Returns an error when the expected socket is absent, is a symlink or another file type, belongs
/// to another effective user, or cannot be restricted to mode `0600`.
pub(crate) fn secure_bound_socket(paths: &RuntimePaths) -> io::Result<()> {
  secure_owned_path(
    &unix_socket_path(paths),
    ExpectedFileType::Socket,
    OWNER_FILE_MODE,
  )
}

/// Removes a non-responsive owner-only socket before retrying a bind.
///
/// Callers must first attempt an authenticated connection. This helper only
/// permits reclaiming the expected socket type in Cadder's private directory;
/// it never deletes an arbitrary runtime path.
pub(crate) fn remove_stale_socket(paths: &RuntimePaths) -> io::Result<()> {
  let path = unix_socket_path(paths);
  secure_bound_socket(paths)?;
  fs::remove_file(&path)
}

fn unix_socket_directory() -> PathBuf {
  Path::new("/tmp").join(format!("cadder-{}", current_euid()))
}

fn secure_socket_directory() -> io::Result<()> {
  validate_socket_root(Path::new("/tmp"))?;
  let path = unix_socket_directory();
  let mut builder = DirBuilder::new();
  builder.mode(OWNER_DIRECTORY_MODE);
  match builder.create(&path) {
    Ok(()) => {}
    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
    Err(error) => return Err(error),
  }
  secure_owned_path(&path, ExpectedFileType::Directory, OWNER_DIRECTORY_MODE)
}

fn validate_socket_root(path: &Path) -> io::Result<()> {
  let metadata = fs::metadata(path)?;
  let mode = metadata.permissions().mode();
  let owner = metadata.uid();
  let trusted_owner = owner == 0 || owner == current_euid();
  let writable_by_group_or_others = mode & 0o022 != 0;
  let protects_entries = mode & 0o1000 != 0;
  if !metadata.is_dir() || !trusted_owner || writable_by_group_or_others && !protects_entries {
    return Err(permission_denied(
      path,
      "expected a root- or owner-controlled temporary directory whose writable entries are protected by the sticky bit",
    ));
  }
  Ok(())
}

/// Returns the process effective user ID used as the Unix IPC owner identity.
pub(crate) fn current_euid() -> u32 {
  // SAFETY: `geteuid` has no preconditions and only reads the process identity.
  unsafe { libc::geteuid() }
}

/// Returns the immutable effective user ID attached to a connected Unix socket peer.
///
/// # Errors
///
/// Returns the operating-system credential lookup error, or [`io::ErrorKind::PermissionDenied`]
/// when the platform does not expose an effective user ID. Missing credentials never fall back to
/// a process ID or an unauthenticated identity.
pub(crate) fn peer_euid(stream: &Stream) -> io::Result<u32> {
  stream.peer_creds()?.euid().ok_or_else(|| {
    io::Error::new(
      io::ErrorKind::PermissionDenied,
      "the local socket did not expose the peer effective user ID",
    )
  })
}

fn create_owner_only_file(path: &Path) -> io::Result<File> {
  let mut options = OpenOptions::new();
  options
    .write(true)
    .create_new(true)
    .mode(OWNER_FILE_MODE)
    .custom_flags(libc::O_NOFOLLOW);
  let file = options.open(path)?;
  secure_open_file(&file, path)?;
  Ok(file)
}

fn create_owner_only_lock_file(path: &Path) -> io::Result<File> {
  let mut options = OpenOptions::new();
  options
    .read(true)
    .write(true)
    .create_new(true)
    .mode(OWNER_FILE_MODE)
    .custom_flags(libc::O_NOFOLLOW);
  let file = options.open(path)?;
  secure_open_file(&file, path)?;
  Ok(file)
}

fn open_existing_lock_file_without_following_symlinks(path: &Path) -> io::Result<File> {
  let mut options = OpenOptions::new();
  options
    .read(true)
    .write(true)
    .custom_flags(libc::O_NOFOLLOW);
  options.open(path)
}

fn secure_open_file(file: &File, path: &Path) -> io::Result<()> {
  validate_owned_metadata(&file.metadata()?, path, ExpectedFileType::RegularFile)?;
  file.set_permissions(fs::Permissions::from_mode(OWNER_FILE_MODE))?;
  validate_mode(&file.metadata()?, path, OWNER_FILE_MODE)
}

fn secure_owned_path(path: &Path, expected: ExpectedFileType, mode: u32) -> io::Result<()> {
  validate_owned_path(path, expected)?;
  fs::set_permissions(path, fs::Permissions::from_mode(mode))?;

  let metadata = fs::symlink_metadata(path)?;
  validate_owned_metadata(&metadata, path, expected)?;
  validate_mode(&metadata, path, mode)
}

fn validate_owned_path(path: &Path, expected: ExpectedFileType) -> io::Result<()> {
  let metadata = fs::symlink_metadata(path)?;
  validate_owned_metadata(&metadata, path, expected)
}

fn validate_owned_metadata(
  metadata: &fs::Metadata,
  path: &Path,
  expected: ExpectedFileType,
) -> io::Result<()> {
  if metadata.file_type().is_symlink() || !expected.matches(metadata) {
    return Err(permission_denied(
      path,
      format!("expected an owner-controlled {}", expected.description()),
    ));
  }
  if metadata.uid() != current_euid() {
    return Err(permission_denied(
      path,
      "path is not owned by the current effective user",
    ));
  }
  Ok(())
}

fn validate_mode(metadata: &fs::Metadata, path: &Path, expected_mode: u32) -> io::Result<()> {
  let actual_mode = metadata.permissions().mode() & 0o777;
  if actual_mode != expected_mode {
    return Err(permission_denied(
      path,
      format!("expected mode {expected_mode:#05o}, found {actual_mode:#05o}"),
    ));
  }
  Ok(())
}

fn permission_denied(path: &Path, reason: impl AsRef<str>) -> io::Error {
  io::Error::new(
    io::ErrorKind::PermissionDenied,
    format!(
      "refuse insecure IPC path {}: {}",
      path.display(),
      reason.as_ref()
    ),
  )
}

#[derive(Debug, Clone, Copy)]
enum ExpectedFileType {
  Directory,
  RegularFile,
  Socket,
}

impl ExpectedFileType {
  fn matches(self, metadata: &fs::Metadata) -> bool {
    match self {
      Self::Directory => metadata.is_dir(),
      Self::RegularFile => metadata.is_file(),
      Self::Socket => metadata.file_type().is_socket(),
    }
  }

  fn description(self) -> &'static str {
    match self {
      Self::Directory => "directory",
      Self::RegularFile => "regular file",
      Self::Socket => "Unix domain socket",
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use interprocess::local_socket::tokio::Listener;
  use std::{io::Write, os::unix::fs::symlink};

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

  #[test]
  fn unix_ipc_security_discovery_file_is_owner_only() {
    let root = tempfile::tempdir().unwrap();
    let paths = runtime_paths(root.path());
    secure_runtime_paths(&paths).unwrap();

    let path = paths.runtime_dir().join(".cadder-ipc.test.tmp");
    let mut file = create_owner_only_runtime_file(&paths, &path).unwrap();
    file.write_all(b"first").unwrap();
    drop(file);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
    let file = open_owner_only_lock_file(&paths, &path).unwrap();
    drop(file);

    assert_eq!(mode(&path), OWNER_FILE_MODE);
    assert_eq!(fs::read(path).unwrap(), b"first");
  }

  #[test]
  fn unix_ipc_security_rejects_a_symlinked_discovery_file() {
    let root = tempfile::tempdir().unwrap();
    let paths = runtime_paths(root.path());
    secure_runtime_paths(&paths).unwrap();
    let target = root.path().join("target.json");
    fs::write(&target, b"keep").unwrap();
    symlink(&target, paths.ipc_endpoint_path()).unwrap();

    let error = open_owner_only_lock_file(&paths, &paths.ipc_endpoint_path()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read(target).unwrap(), b"keep");
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
}
