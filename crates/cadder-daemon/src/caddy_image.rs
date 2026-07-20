//! Immutable evidence for the real-Caddy executable selected by the daemon.

use crate::{
  process_tree::ProcessTreeChild,
  runtime_guard_record::{RuntimeGuardImageIdentity, RuntimeGuardPinnedCaddyIdentity},
};
use anyhow::{Context, Result, ensure};
use semver::Version;
use sha2::{Digest, Sha256};
use std::{
  collections::BTreeSet,
  fs::{File, OpenOptions},
  io::{Read, Seek, SeekFrom},
  path::{Path, PathBuf},
  sync::Arc,
};
use tokio::process::Command;

pub(crate) const CADDY_COMPATIBILITY_PROBE_REVISION: &str = "cadder-v1-probe-1";
pub(crate) const MINIMUM_CADDY_VERSION: &str = "2.11.3";

const REQUIRED_CADDY_MODULES: &[&str] = &[
  "http",
  "http.encoders.gzip",
  "http.encoders.zstd",
  "http.handlers.encode",
  "http.handlers.file_server",
  "http.handlers.headers",
  "http.handlers.reverse_proxy",
  "http.handlers.rewrite",
  "http.handlers.static_response",
  "http.handlers.subroute",
  "http.matchers.header",
  "http.matchers.host",
  "http.matchers.method",
  "http.matchers.path",
  "http.matchers.query",
  "http.reverse_proxy.transport.http",
  "pki",
  "tls",
  "tls.issuance.internal",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaddyImageSource {
  ExplicitDaemonOverride,
  PortableConfiguration,
  UserConfiguration,
  SystemConfiguration,
  Path,
  #[cfg(any(test, debug_assertions))]
  TestFixture,
}

impl CaddyImageSource {
  pub(crate) fn description(self) -> &'static str {
    match self {
      Self::ExplicitDaemonOverride => "explicit daemon override",
      Self::PortableConfiguration => "portable configuration",
      Self::UserConfiguration => "per-user configuration",
      Self::SystemConfiguration => "system configuration",
      Self::Path => "PATH",
      #[cfg(any(test, debug_assertions))]
      Self::TestFixture => "test fixture",
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CaddyFileIdentity {
  #[cfg(unix)]
  Unix { device: u64, inode: u64 },
  #[cfg(windows)]
  Windows {
    volume_serial: u64,
    file_id: [u8; 16],
  },
}

impl CaddyFileIdentity {
  fn stable_id(&self) -> String {
    match self {
      #[cfg(unix)]
      Self::Unix { device, inode } => {
        format!("unix-device-{device:016x}-inode-{inode:016x}")
      }
      #[cfg(windows)]
      Self::Windows {
        volume_serial,
        file_id,
      } => format!(
        "windows-volume-{volume_serial:016x}-file-{}",
        hex::encode(file_id)
      ),
    }
  }
}

#[derive(Debug)]
pub(crate) struct OpenedCaddyImage {
  file: File,
  canonical_path: PathBuf,
  identity: CaddyFileIdentity,
  digest: [u8; 32],
}

impl OpenedCaddyImage {
  pub(crate) fn open(path: &Path) -> Result<Self> {
    ensure!(path.is_absolute(), "pinned Caddy path must be absolute");
    let canonical_path = path
      .canonicalize()
      .with_context(|| format!("canonicalize pinned Caddy image {}", path.display()))?;
    let file = open_image_file(&canonical_path)?;
    validate_image_file(&file, &canonical_path)?;
    let identity = file_identity(&file)?;
    let digest = file_digest(&file)?;
    Ok(Self {
      file,
      canonical_path,
      identity,
      digest,
    })
  }

  pub(crate) fn path(&self) -> &Path {
    &self.canonical_path
  }

  pub(crate) fn verify_runtime_guard_claim(
    claim: &RuntimeGuardPinnedCaddyIdentity,
  ) -> Result<Self> {
    let opened = Self::open(&claim.image.path).context("open runtime-guard Caddy image")?;
    ensure!(
      opened.canonical_path == claim.image.path,
      "runtime-guard Caddy path is not the claimed canonical path"
    );
    ensure!(
      opened.identity.stable_id() == claim.image.file_identity,
      "runtime-guard Caddy file identity does not match the pinned claim"
    );
    ensure!(
      hex::encode(opened.digest) == claim.image.sha256,
      "runtime-guard Caddy digest does not match the pinned claim"
    );
    let version = Version::parse(&claim.version)
      .context("runtime-guard Caddy claim contains an invalid semantic version")?;
    let minimum = Version::parse(MINIMUM_CADDY_VERSION).expect("minimum Caddy version is valid");
    ensure!(
      version >= minimum && version.major < 3,
      "runtime-guard Caddy claim contains unsupported version {version}"
    );
    ensure!(
      claim.probe_revision == CADDY_COMPATIBILITY_PROBE_REVISION,
      "runtime-guard Caddy claim contains an unsupported compatibility probe revision"
    );
    Ok(opened)
  }

  pub(crate) async fn spawn<F>(&self, operation: &str, configure: F) -> Result<ProcessTreeChild>
  where
    F: FnOnce(&mut Command),
  {
    self.reverify_image()?;
    spawn_reverified(self.path(), operation, configure, || self.reverify_image()).await
  }

  /// Confirms that the canonical pathname still resolves to the held executable image.
  pub(crate) fn reverify_image(&self) -> Result<()> {
    let current_path = self.canonical_path.canonicalize().with_context(|| {
      format!(
        "reverify pinned Caddy path {}",
        self.canonical_path.display()
      )
    })?;
    ensure!(
      current_path == self.canonical_path,
      "pinned Caddy canonical path changed from {} to {}",
      self.canonical_path.display(),
      current_path.display()
    );
    let current = open_image_file(&current_path)?;
    validate_image_file(&current, &current_path)?;
    ensure!(
      file_identity(&current)? == self.identity,
      "pinned Caddy file identity changed at {}",
      self.canonical_path.display()
    );
    ensure!(
      file_digest(&current)? == self.digest,
      "pinned Caddy digest changed at {}",
      self.canonical_path.display()
    );
    Ok(())
  }
}

#[derive(Debug)]
pub(crate) struct PinnedCaddyImage {
  _anchor: File,
  source: CaddyImageSource,
  canonical_path: PathBuf,
  identity: CaddyFileIdentity,
  digest: [u8; 32],
  version: Version,
  modules: BTreeSet<String>,
  probe_revision: &'static str,
}

#[derive(Debug)]
pub(crate) struct VerifiedCaddyImage {
  pinned: Arc<PinnedCaddyImage>,
  opened: OpenedCaddyImage,
}

impl VerifiedCaddyImage {
  pub(crate) fn path(&self) -> &Path {
    self.opened.path()
  }

  pub(crate) fn version(&self) -> &Version {
    self.pinned.version()
  }

  pub(crate) async fn spawn<F>(&self, operation: &str, configure: F) -> Result<ProcessTreeChild>
  where
    F: FnOnce(&mut Command),
  {
    self.opened.reverify_image()?;
    spawn_reverified(self.path(), operation, configure, || {
      self.opened.reverify_image()
    })
    .await
  }
}

async fn spawn_reverified<F, R>(
  path: &Path,
  operation: &str,
  configure: F,
  reverify: R,
) -> Result<ProcessTreeChild>
where
  F: FnOnce(&mut Command),
  R: FnOnce() -> Result<()>,
{
  let mut command = Command::new(path);
  configure(&mut command);
  let mut child = ProcessTreeChild::spawn(command)
    .with_context(|| format!("start {operation} from pinned Caddy image"))?;
  if let Err(error) = reverify() {
    child
      .terminate_and_join(operation)
      .await
      .with_context(|| format!("contain {operation} after pinned Caddy identity mismatch"))?;
    return Err(error).with_context(|| format!("reverify pinned Caddy after starting {operation}"));
  }
  Ok(child)
}

impl PinnedCaddyImage {
  pub(crate) fn capture(
    opened: &OpenedCaddyImage,
    source: CaddyImageSource,
    version: Version,
    modules: BTreeSet<String>,
    probe_revision: &'static str,
  ) -> Result<Self> {
    let pinned = Self {
      _anchor: opened
        .file
        .try_clone()
        .context("retain pinned Caddy image handle")?,
      source,
      canonical_path: opened.canonical_path.clone(),
      identity: opened.identity.clone(),
      digest: opened.digest,
      version,
      modules,
      probe_revision,
    };
    pinned.verify()?;
    Ok(pinned)
  }

  pub(crate) fn verify(&self) -> Result<OpenedCaddyImage> {
    self.validate_compatibility_metadata()?;
    let opened = OpenedCaddyImage::open(&self.canonical_path)
      .with_context(|| format!("reverify pinned Caddy from {}", self.source.description()))?;
    ensure!(
      opened.identity == self.identity,
      "pinned Caddy file identity changed at {}",
      self.canonical_path.display()
    );
    ensure!(
      opened.digest == self.digest,
      "pinned Caddy digest changed at {}",
      self.canonical_path.display()
    );
    Ok(opened)
  }

  pub(crate) fn verified(self: &Arc<Self>) -> Result<VerifiedCaddyImage> {
    Ok(VerifiedCaddyImage {
      pinned: self.clone(),
      opened: self.verify()?,
    })
  }

  pub(crate) fn version(&self) -> &Version {
    &self.version
  }

  #[allow(dead_code)]
  pub(crate) fn runtime_guard_identity(&self) -> RuntimeGuardPinnedCaddyIdentity {
    RuntimeGuardPinnedCaddyIdentity {
      image: RuntimeGuardImageIdentity {
        path: self.canonical_path.clone(),
        file_identity: self.identity.stable_id(),
        sha256: hex::encode(self.digest),
      },
      version: self.version.to_string(),
      probe_revision: self.probe_revision.to_string(),
    }
  }

  fn validate_compatibility_metadata(&self) -> Result<()> {
    let minimum = Version::parse(MINIMUM_CADDY_VERSION).expect("minimum Caddy version is valid");
    ensure!(
      self.version >= minimum && self.version.major < 3,
      "unsupported Caddy version {}; Cadder requires >={MINIMUM_CADDY_VERSION}, <3.0.0",
      self.version
    );
    let missing = REQUIRED_CADDY_MODULES
      .iter()
      .copied()
      .filter(|module| !self.modules.contains(*module))
      .collect::<Vec<_>>();
    ensure!(
      missing.is_empty(),
      "Caddy is missing required modules: {}",
      missing.join(", ")
    );
    ensure!(
      self.probe_revision == CADDY_COMPATIBILITY_PROBE_REVISION,
      "Caddy compatibility probe revision changed from {} to {}",
      CADDY_COMPATIBILITY_PROBE_REVISION,
      self.probe_revision
    );
    Ok(())
  }
}

fn file_digest(file: &File) -> Result<[u8; 32]> {
  let mut reader = file
    .try_clone()
    .context("clone pinned Caddy image handle")?;
  reader
    .seek(SeekFrom::Start(0))
    .context("seek pinned Caddy image")?;
  let mut hasher = Sha256::new();
  let mut buffer = [0_u8; 64 * 1024];
  loop {
    let read = reader
      .read(&mut buffer)
      .context("hash pinned Caddy image")?;
    if read == 0 {
      break;
    }
    hasher.update(&buffer[..read]);
  }
  Ok(hasher.finalize().into())
}

#[cfg(unix)]
fn open_image_file(path: &Path) -> Result<File> {
  use std::os::unix::fs::OpenOptionsExt;

  OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
    .open(path)
    .with_context(|| format!("open pinned Caddy image {}", path.display()))
}

#[cfg(windows)]
fn open_image_file(path: &Path) -> Result<File> {
  use std::os::windows::fs::OpenOptionsExt;
  use windows_sys::Win32::{
    Foundation::GENERIC_READ,
    Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ},
  };

  OpenOptions::new()
    .access_mode(GENERIC_READ)
    .share_mode(FILE_SHARE_READ)
    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
    .open(path)
    .with_context(|| format!("open pinned Caddy image {}", path.display()))
}

#[cfg(unix)]
fn validate_image_file(file: &File, path: &Path) -> Result<()> {
  use std::os::unix::fs::{MetadataExt, PermissionsExt};

  let metadata = file
    .metadata()
    .with_context(|| format!("inspect pinned Caddy image {}", path.display()))?;
  ensure!(
    metadata.is_file(),
    "pinned Caddy image is not a regular file"
  );
  ensure!(
    metadata.permissions().mode() & 0o111 != 0,
    "pinned Caddy image is not executable"
  );
  ensure!(
    metadata.nlink() > 0,
    "pinned Caddy image has no filesystem links"
  );
  Ok(())
}

#[cfg(windows)]
fn validate_image_file(file: &File, path: &Path) -> Result<()> {
  use std::os::windows::fs::MetadataExt;
  use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
  };

  let attributes = file
    .metadata()
    .with_context(|| format!("inspect pinned Caddy image {}", path.display()))?
    .file_attributes();
  ensure!(
    attributes & FILE_ATTRIBUTE_DIRECTORY == 0,
    "pinned Caddy image is not a regular file"
  );
  ensure!(
    attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
    "pinned Caddy image is a Windows reparse point"
  );
  Ok(())
}

#[cfg(unix)]
fn file_identity(file: &File) -> Result<CaddyFileIdentity> {
  use std::os::unix::fs::MetadataExt;

  let metadata = file
    .metadata()
    .context("inspect pinned Caddy file identity")?;
  Ok(CaddyFileIdentity::Unix {
    device: metadata.dev(),
    inode: metadata.ino(),
  })
}

#[cfg(windows)]
fn file_identity(file: &File) -> Result<CaddyFileIdentity> {
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::Storage::FileSystem::{
    FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
  };

  let mut information = FILE_ID_INFO::default();
  // SAFETY: `file` owns a live handle and `information` is a correctly sized output buffer.
  if unsafe {
    GetFileInformationByHandleEx(
      file.as_raw_handle(),
      FileIdInfo,
      std::ptr::from_mut(&mut information).cast(),
      std::mem::size_of::<FILE_ID_INFO>() as u32,
    )
  } == 0
  {
    return Err(std::io::Error::last_os_error()).context("inspect pinned Caddy file identity");
  }
  Ok(CaddyFileIdentity::Windows {
    volume_serial: information.VolumeSerialNumber,
    file_id: information.FileId.Identifier,
  })
}

#[cfg(any(test, debug_assertions))]
pub(crate) fn required_caddy_modules() -> BTreeSet<String> {
  REQUIRED_CADDY_MODULES
    .iter()
    .map(|module| (*module).to_string())
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use std::time::{Duration, Instant};

  const SPAWN_HELPER_MARKER_ENV: &str = "CADDER_PINNED_CADDY_SPAWN_HELPER_MARKER";

  fn write_image(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let mut permissions = fs::metadata(path).unwrap().permissions();
      permissions.set_mode(0o755);
      fs::set_permissions(path, permissions).unwrap();
    }
  }

  #[test]
  fn pinned_caddy_image_spawn_helper() {
    let Some(marker) = std::env::var_os(SPAWN_HELPER_MARKER_ENV) else {
      return;
    };
    fs::write(marker, b"started").unwrap();
    std::thread::sleep(Duration::from_secs(60));
  }

  #[tokio::test]
  async fn pinned_caddy_image_contains_a_post_spawn_identity_mismatch() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("spawned");
    let executable = std::env::current_exe().unwrap();
    let error = spawn_reverified(
      &executable,
      "pinned Caddy containment fixture",
      |command| {
        command
          .arg("--exact")
          .arg("caddy_image::tests::pinned_caddy_image_spawn_helper")
          .env(SPAWN_HELPER_MARKER_ENV, &marker)
          .stdin(std::process::Stdio::null())
          .stdout(std::process::Stdio::null())
          .stderr(std::process::Stdio::null());
      },
      || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !marker.is_file() {
          ensure!(
            Instant::now() < deadline,
            "pinned Caddy containment fixture did not start"
          );
          std::thread::sleep(Duration::from_millis(10));
        }
        anyhow::bail!("injected post-spawn identity mismatch")
      },
    )
    .await
    .unwrap_err();

    assert!(marker.is_file());
    assert!(format!("{error:#}").contains("identity mismatch"));
  }

  #[test]
  fn pinned_caddy_image_prevents_or_detects_mutation_through_hardlink() {
    let temp = tempfile::tempdir().unwrap();
    let image = temp.path().join("caddy.exe");
    let alias = temp.path().join("caddy-alias.exe");
    write_image(&image, b"first image");
    fs::hard_link(&image, &alias).unwrap();
    let opened = OpenedCaddyImage::open(&image).unwrap();
    let _pinned = PinnedCaddyImage::capture(
      &opened,
      CaddyImageSource::TestFixture,
      Version::parse(MINIMUM_CADDY_VERSION).unwrap(),
      required_caddy_modules(),
      CADDY_COMPATIBILITY_PROBE_REVISION,
    )
    .unwrap();
    drop(opened);
    let mutation = fs::write(alias, b"second image");

    #[cfg(windows)]
    assert!(
      mutation.is_err(),
      "the pinned Windows handle allowed a write"
    );
    #[cfg(unix)]
    {
      mutation.unwrap();
      let error = _pinned.verify().unwrap_err();
      assert!(error.to_string().contains("digest changed"));
    }
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn opened_caddy_image_rejects_digest_mutation_before_spawn() {
    let temp = tempfile::tempdir().unwrap();
    let image = temp.path().join("caddy");
    let alias = temp.path().join("caddy-alias");
    write_image(&image, b"first image");
    fs::hard_link(&image, &alias).unwrap();
    let opened = OpenedCaddyImage::open(&image).unwrap();
    fs::write(alias, b"second image").unwrap();

    let error = opened
      .spawn("mutated Caddy fixture", |_| {})
      .await
      .unwrap_err();

    assert!(error.to_string().contains("digest changed"));
  }

  #[cfg(windows)]
  #[test]
  fn pinned_caddy_image_blocks_rename_and_delete_on_windows() {
    let temp = tempfile::tempdir().unwrap();
    let image = temp.path().join("caddy.exe");
    let renamed = temp.path().join("replacement.exe");
    write_image(&image, b"pinned image");
    let opened = OpenedCaddyImage::open(&image).unwrap();
    let pinned = PinnedCaddyImage::capture(
      &opened,
      CaddyImageSource::TestFixture,
      Version::parse(MINIMUM_CADDY_VERSION).unwrap(),
      required_caddy_modules(),
      CADDY_COMPATIBILITY_PROBE_REVISION,
    )
    .unwrap();
    drop(opened);

    assert!(
      fs::rename(&image, &renamed).is_err(),
      "the pinned Windows handle allowed a rename"
    );
    assert!(
      fs::remove_file(&image).is_err(),
      "the pinned Windows handle allowed deletion"
    );
    pinned.verify().unwrap();
  }

  #[test]
  fn pinned_caddy_image_rejects_unsupported_version_and_missing_modules() {
    let temp = tempfile::tempdir().unwrap();
    let image = temp.path().join("caddy.exe");
    write_image(&image, b"image");
    let opened = OpenedCaddyImage::open(&image).unwrap();

    let version_error = PinnedCaddyImage::capture(
      &opened,
      CaddyImageSource::TestFixture,
      Version::new(2, 10, 0),
      required_caddy_modules(),
      CADDY_COMPATIBILITY_PROBE_REVISION,
    )
    .unwrap_err();
    let module_error = PinnedCaddyImage::capture(
      &opened,
      CaddyImageSource::TestFixture,
      Version::parse(MINIMUM_CADDY_VERSION).unwrap(),
      BTreeSet::new(),
      CADDY_COMPATIBILITY_PROBE_REVISION,
    )
    .unwrap_err();
    let probe_error = PinnedCaddyImage::capture(
      &opened,
      CaddyImageSource::TestFixture,
      Version::parse(MINIMUM_CADDY_VERSION).unwrap(),
      required_caddy_modules(),
      "stale-probe",
    )
    .unwrap_err();

    assert!(
      version_error
        .to_string()
        .contains("unsupported Caddy version")
    );
    assert!(
      module_error
        .to_string()
        .contains("missing required modules")
    );
    assert!(probe_error.to_string().contains("probe revision changed"));
  }

  #[test]
  fn pinned_caddy_image_required_module_inventory_matches_the_release_contract() {
    assert_eq!(
      required_caddy_modules().into_iter().collect::<Vec<_>>(),
      vec![
        "http",
        "http.encoders.gzip",
        "http.encoders.zstd",
        "http.handlers.encode",
        "http.handlers.file_server",
        "http.handlers.headers",
        "http.handlers.reverse_proxy",
        "http.handlers.rewrite",
        "http.handlers.static_response",
        "http.handlers.subroute",
        "http.matchers.header",
        "http.matchers.host",
        "http.matchers.method",
        "http.matchers.path",
        "http.matchers.query",
        "http.reverse_proxy.transport.http",
        "pki",
        "tls",
        "tls.issuance.internal",
      ]
    );
  }
}
