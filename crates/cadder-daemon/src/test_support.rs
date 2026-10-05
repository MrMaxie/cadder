use std::{
  fs,
  path::{Path, PathBuf},
};

pub(crate) fn tempdir(prefix: &str) -> tempfile::TempDir {
  let profile_dir = test_process_path()
    .parent()
    .expect("test process binary has a profile directory")
    .to_path_buf();
  tempfile::Builder::new()
    .prefix(prefix)
    .tempdir_in(profile_dir)
    .expect("create test directory beside Cargo test artifacts")
}

pub(crate) fn install_test_process(directory: &Path, mode: &str) -> PathBuf {
  let source = test_process_path();
  let destination = test_process_destination(directory);
  fs::hard_link(&source, &destination).unwrap_or_else(|error| {
    panic!(
      "hard-link test process {} to {}: {error}",
      source.display(),
      destination.display()
    )
  });
  configure_test_process(directory, destination, mode)
}

pub(crate) fn copy_test_process(directory: &Path, mode: &str) -> PathBuf {
  let source = test_process_path();
  let destination = test_process_destination(directory);
  fs::copy(&source, &destination).unwrap_or_else(|error| {
    panic!(
      "copy test process {} to {}: {error}",
      source.display(),
      destination.display()
    )
  });
  configure_test_process(directory, destination, mode)
}

fn configure_test_process(directory: &Path, path: PathBuf, mode: &str) -> PathBuf {
  fs::write(directory.join("cadder-test.mode"), mode).expect("write test process mode");
  path
}

fn test_process_destination(directory: &Path) -> PathBuf {
  directory.join(if cfg!(windows) {
    "cadder-test-process.exe"
  } else {
    "cadder-test-process"
  })
}

fn test_process_path() -> PathBuf {
  let current = std::env::current_exe().expect("resolve current test executable");
  let deps_dir = current
    .parent()
    .expect("current test executable has a parent directory");
  let profile_dir = if deps_dir.file_name().is_some_and(|name| name == "deps") {
    deps_dir
      .parent()
      .expect("Cargo deps directory has a profile parent")
  } else {
    deps_dir
  };
  let path = profile_dir.join(if cfg!(windows) {
    "cadder-test-process.exe"
  } else {
    "cadder-test-process"
  });
  assert!(
    path.is_file(),
    "missing native test process at {}; run tests through Cargo without restricting targets to --lib",
    path.display()
  );
  path
}
