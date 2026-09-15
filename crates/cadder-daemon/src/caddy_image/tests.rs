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
async fn pinned_caddy_image_terminates_a_post_spawn_identity_mismatch() {
  let temp = tempfile::tempdir().unwrap();
  let marker = temp.path().join("spawned");
  let executable = std::env::current_exe().unwrap();
  let error = spawn_reverified(
    &executable,
    "pinned Caddy process-tree fixture",
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
          "pinned Caddy process-tree fixture did not start"
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
