use super::*;

#[derive(Debug, Clone)]
pub struct RealCaddyResolver {
  explicit_override: Option<PathBuf>,
  config_paths: TrustedConfigPaths,
  executable_path: Option<PathBuf>,
  trust_policy: CaddyTrustPolicy,
  resolved: Arc<OnceLock<ResolvedCaddyPath>>,
  pinned: Arc<OnceCell<Arc<PinnedCaddyImage>>>,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedCaddyPath {
  path: PathBuf,
  source: CaddyImageSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CaddyTrustPolicy {
  Enforce,
  #[cfg(any(test, debug_assertions))]
  TestFixture,
}

impl RealCaddyResolver {
  pub fn from_trusted_sources() -> Self {
    Self::for_daemon(None)
  }

  pub(crate) fn for_daemon(explicit_override: Option<PathBuf>) -> Self {
    Self {
      explicit_override,
      config_paths: TrustedConfigPaths::platform_defaults(),
      executable_path: env::current_exe().ok(),
      trust_policy: CaddyTrustPolicy::Enforce,
      resolved: Arc::new(OnceLock::new()),
      pinned: Arc::new(OnceCell::new()),
    }
  }

  #[cfg(test)]
  fn with_sources(
    explicit_override: Option<PathBuf>,
    config_paths: TrustedConfigPaths,
    executable_path: Option<PathBuf>,
  ) -> Self {
    Self {
      explicit_override,
      config_paths,
      executable_path,
      trust_policy: CaddyTrustPolicy::Enforce,
      resolved: Arc::new(OnceLock::new()),
      pinned: Arc::new(OnceCell::new()),
    }
  }

  #[cfg(test)]
  pub(super) fn with_executable_path(
    explicit_override: Option<String>,
    executable_path: Option<PathBuf>,
  ) -> Self {
    let mut resolver = Self::with_sources(
      explicit_override.map(PathBuf::from),
      TrustedConfigPaths {
        user: None,
        system: None,
      },
      executable_path,
    );
    resolver.trust_policy = CaddyTrustPolicy::TestFixture;
    resolver
  }

  #[cfg(test)]
  pub(super) fn with_test_sources(
    explicit_override: Option<PathBuf>,
    config_paths: TrustedConfigPaths,
    executable_path: Option<PathBuf>,
  ) -> Self {
    let mut resolver = Self::with_sources(explicit_override, config_paths, executable_path);
    resolver.trust_policy = CaddyTrustPolicy::TestFixture;
    resolver
  }

  #[cfg(debug_assertions)]
  #[doc(hidden)]
  pub fn for_test_fixture(path: PathBuf) -> Self {
    let mut resolver = Self::for_daemon(Some(path));
    resolver.config_paths = TrustedConfigPaths {
      user: None,
      system: None,
    };
    resolver.executable_path = None;
    resolver.trust_policy = CaddyTrustPolicy::TestFixture;
    resolver
  }

  pub fn resolve(&self) -> Result<PathBuf> {
    Ok(self.resolve_evidence()?.path)
  }

  fn resolve_evidence(&self) -> Result<ResolvedCaddyPath> {
    if let Some(resolved) = self.resolved.get() {
      return Ok(resolved.clone());
    }
    let selected = self.resolve_uncached()?;
    let _ = self.resolved.set(selected);
    Ok(
      self
        .resolved
        .get()
        .expect("resolved Caddy path is initialized")
        .clone(),
    )
  }

  pub(crate) async fn pin(&self) -> Result<Arc<PinnedCaddyImage>> {
    let pinned = self
      .pinned
      .get_or_try_init(|| async { self.capture_pinned_image().await.map(Arc::new) })
      .await?;
    Ok(pinned.clone())
  }

  pub(crate) async fn verify_for_spawn(&self) -> Result<VerifiedCaddyImage> {
    self.pin().await?.verified()
  }

  async fn capture_pinned_image(&self) -> Result<PinnedCaddyImage> {
    let resolved = self.resolve_evidence()?;
    let opened = OpenedCaddyImage::open(&resolved.path)?;
    opened.reverify_image()?;

    #[cfg(any(test, debug_assertions))]
    if self.trust_policy == CaddyTrustPolicy::TestFixture {
      return PinnedCaddyImage::capture(
        &opened,
        CaddyImageSource::TestFixture,
        Version::parse(MINIMUM_CADDY_VERSION).expect("minimum Caddy version is valid"),
        required_caddy_modules(),
        CADDY_COMPATIBILITY_PROBE_REVISION,
      );
    }

    let version_output = run_pinned_metadata_command(&opened, &["version"]).await?;
    let version = parse_caddy_version(&version_output)?;
    let module_output = run_pinned_metadata_command(&opened, &["list-modules", "--json"]).await?;
    let modules = parse_caddy_modules(&module_output)?;
    PinnedCaddyImage::capture(
      &opened,
      resolved.source,
      version,
      modules,
      CADDY_COMPATIBILITY_PROBE_REVISION,
    )
  }

  fn resolve_uncached(&self) -> Result<ResolvedCaddyPath> {
    let shim_candidates = self.shim_candidates();
    if let Some(path) = &self.explicit_override {
      return self.resolve_selected(
        path,
        CaddyImageSource::ExplicitDaemonOverride,
        &shim_candidates,
      );
    }

    for (path, source) in self.configured_sources() {
      if let Some(selected) = self.selection_from_config(&path, source)? {
        return self.resolve_configured_selection(selected, source, &shim_candidates);
      }
    }

    resolve_caddy_on_path(&shim_candidates, self.trust_policy)
      .map(|path| ResolvedCaddyPath {
        path,
        source: CaddyImageSource::Path,
      })
      .context(
        "could not resolve a usable real Caddy executable. Pass an absolute daemon override, \
         configure a command or absolute path in portable Cadder configuration, or install a \
         native caddy executable on PATH",
      )
  }

  pub fn resolution_help(error: &anyhow::Error) -> String {
    format!(
      "Cadder could not resolve a usable real Caddy executable.\n\n\
       Cause: {error}\n\n\
       Configure real Caddy with one of these explicit sources, in precedence order:\n\
       - an absolute --real-caddy daemon-start override\n\
       - caddy.real_command or caddy.real_path in cadder.toml beside the Cadder executables\n\
       - the same configuration in the per-user or system Cadder configuration\n\
       - a native real caddy executable on PATH\n\n\
       Project files, registration working directories, environment selectors, and shim flags never select the executable."
    )
  }

  fn selection_from_config(
    &self,
    path: &Path,
    source: CaddyImageSource,
  ) -> Result<Option<RealCaddySelection>> {
    let source_description = source.description();
    match std::fs::symlink_metadata(path) {
      Ok(_) => {}
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
      Err(error) => {
        return Err(error)
          .with_context(|| format!("inspect {source_description} at {}", path.display()));
      }
    }
    let config = match self.trust_policy {
      CaddyTrustPolicy::Enforce => {
        let trusted = open_caddy_config(path)
          .with_context(|| format!("validate {source_description} at {}", path.display()))?;
        let canonical_path = trusted.canonical_path().to_path_buf();
        CadderConfig::from_reader(trusted.into_file(), &canonical_path)?
      }
      #[cfg(any(test, debug_assertions))]
      CaddyTrustPolicy::TestFixture => {
        path
          .canonicalize()
          .with_context(|| format!("canonicalize test configuration {}", path.display()))?;
        CadderConfig::from_file(path)?
      }
    };
    config.real_caddy()
  }

  fn resolve_configured_selection(
    &self,
    selection: RealCaddySelection,
    source: CaddyImageSource,
    shim_candidates: &[PathBuf],
  ) -> Result<ResolvedCaddyPath> {
    match selection {
      RealCaddySelection::Path(path) => self.resolve_selected(&path, source, shim_candidates),
      RealCaddySelection::Command(command) => {
        let path = resolve_command_on_path(&command, shim_candidates, self.trust_policy)?;
        Ok(ResolvedCaddyPath { path, source })
      }
    }
  }

  fn configured_sources(&self) -> impl Iterator<Item = (PathBuf, CaddyImageSource)> + '_ {
    [
      self
        .portable_config_path()
        .map(|path| (path, CaddyImageSource::PortableConfiguration)),
      self
        .config_paths
        .user
        .clone()
        .map(|path| (path, CaddyImageSource::UserConfiguration)),
      self
        .config_paths
        .system
        .clone()
        .map(|path| (path, CaddyImageSource::SystemConfiguration)),
    ]
    .into_iter()
    .flatten()
  }

  fn portable_config_path(&self) -> Option<PathBuf> {
    self
      .executable_path
      .as_deref()?
      .parent()
      .map(|directory| directory.join(CONFIG_FILE_NAME))
  }

  fn resolve_selected(
    &self,
    path: &Path,
    source: CaddyImageSource,
    shim_candidates: &[PathBuf],
  ) -> Result<ResolvedCaddyPath> {
    let source_description = source.description();
    if !path.is_absolute() {
      return Err(anyhow!(
        "{source_description} selects relative real-Caddy path {}; use an absolute path",
        path.display()
      ));
    }
    let canonical = match self.trust_policy {
      CaddyTrustPolicy::Enforce => validate_caddy_executable(path).with_context(|| {
        format!(
          "validate real Caddy from {source_description}: {}",
          path.display()
        )
      })?,
      #[cfg(any(test, debug_assertions))]
      CaddyTrustPolicy::TestFixture => path
        .canonicalize()
        .with_context(|| format!("canonicalize test Caddy fixture {}", path.display()))?,
    };
    reject_shim_identity(&canonical, shim_candidates)?;
    Ok(ResolvedCaddyPath {
      path: canonical,
      source: if self.trust_policy == CaddyTrustPolicy::Enforce {
        source
      } else {
        #[cfg(any(test, debug_assertions))]
        {
          CaddyImageSource::TestFixture
        }
        #[cfg(not(any(test, debug_assertions)))]
        unreachable!()
      },
    })
  }

  fn shim_candidates(&self) -> Vec<PathBuf> {
    let Some(executable) = &self.executable_path else {
      return Vec::new();
    };
    let mut candidates = vec![executable.clone()];
    if let Some(parent) = executable.parent() {
      for name in shim_binary_names() {
        candidates.push(parent.join(name));
      }
    }
    candidates
  }
}

#[derive(Debug, Clone)]
pub(super) struct TrustedConfigPaths {
  pub(super) user: Option<PathBuf>,
  pub(super) system: Option<PathBuf>,
}

impl TrustedConfigPaths {
  fn platform_defaults() -> Self {
    let user = directories::ProjectDirs::from("dev", "Cadder", "Cadder")
      .map(|dirs| dirs.config_dir().join(CONFIG_FILE_NAME));
    Self {
      user,
      system: system_config_path(),
    }
  }
}

#[cfg(windows)]
pub(super) fn system_config_path() -> Option<PathBuf> {
  use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr::null_mut, slice};
  use windows_sys::Win32::{
    Foundation::S_OK,
    System::Com::CoTaskMemFree,
    UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath},
  };

  let mut raw_path = null_mut();
  let folder_id = FOLDERID_ProgramData;
  // SAFETY: The known-folder identifier and output pointer follow the Windows API contract. The
  // returned buffer is released with `CoTaskMemFree` on every path.
  let result = unsafe { SHGetKnownFolderPath(&raw const folder_id, 0, null_mut(), &mut raw_path) };
  if result != S_OK || raw_path.is_null() {
    // SAFETY: `CoTaskMemFree` accepts the pointer returned by `SHGetKnownFolderPath`, including null.
    unsafe { CoTaskMemFree(raw_path.cast()) };
    return None;
  }

  let mut length = 0;
  // SAFETY: A successful `SHGetKnownFolderPath` returns a NUL-terminated UTF-16 buffer.
  unsafe {
    while *raw_path.add(length) != 0 {
      length += 1;
    }
  }
  // SAFETY: The scan above found the terminator within the Windows-owned buffer.
  let path = PathBuf::from(OsString::from_wide(unsafe {
    slice::from_raw_parts(raw_path, length)
  }));
  // SAFETY: `SHGetKnownFolderPath` allocated this buffer for the caller.
  unsafe { CoTaskMemFree(raw_path.cast()) };
  Some(path.join("Cadder").join(CONFIG_FILE_NAME))
}

#[cfg(target_os = "macos")]
pub(super) fn system_config_path() -> Option<PathBuf> {
  Some(PathBuf::from("/Library/Application Support/Cadder").join(CONFIG_FILE_NAME))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn system_config_path() -> Option<PathBuf> {
  Some(PathBuf::from("/etc/cadder").join(CONFIG_FILE_NAME))
}

#[cfg(not(any(unix, windows)))]
pub(super) fn system_config_path() -> Option<PathBuf> {
  None
}

pub(super) fn resolve_caddy_on_path(
  shim_candidates: &[PathBuf],
  trust_policy: CaddyTrustPolicy,
) -> Result<PathBuf> {
  resolve_command_on_path("caddy", shim_candidates, trust_policy)
    .context("trusted executable `caddy` not found on PATH")
}

pub(super) fn resolve_command_on_path(
  command: &str,
  shim_candidates: &[PathBuf],
  trust_policy: CaddyTrustPolicy,
) -> Result<PathBuf> {
  let mut components = Path::new(command).components();
  if command.split_whitespace().count() != 1
    || !matches!(components.next(), Some(std::path::Component::Normal(_)))
    || components.next().is_some()
  {
    return Err(anyhow!(
      "configured real-Caddy command `{command}` must be a single program name"
    ));
  }
  let path_var = env::var_os("PATH").ok_or_else(|| anyhow!("PATH is not set"))?;
  for dir in env::split_paths(&path_var) {
    if !dir.is_absolute() {
      continue;
    }
    for candidate in executable_candidates(&dir, command) {
      if !candidate.is_file() {
        continue;
      }
      let canonical = match trust_policy {
        CaddyTrustPolicy::Enforce => {
          let Ok(canonical) = validate_caddy_executable(&candidate) else {
            continue;
          };
          canonical
        }
        #[cfg(any(test, debug_assertions))]
        CaddyTrustPolicy::TestFixture => {
          let Ok(canonical) = candidate.canonicalize() else {
            continue;
          };
          canonical
        }
      };
      #[cfg(windows)]
      let canonical = if let Some(target) = scoop_shim_target(&canonical) {
        match trust_policy {
          CaddyTrustPolicy::Enforce => {
            let Ok(canonical) = validate_caddy_executable(&target) else {
              continue;
            };
            canonical
          }
          #[cfg(any(test, debug_assertions))]
          CaddyTrustPolicy::TestFixture => {
            let Ok(canonical) = target.canonicalize() else {
              continue;
            };
            canonical
          }
        }
      } else {
        canonical
      };
      if reject_shim_identity(&canonical, shim_candidates).is_ok() {
        return Ok(canonical);
      }
    }
  }
  Err(anyhow!(
    "configured real-Caddy command `{command}` was not found on PATH"
  ))
}

pub(super) fn reject_shim_identity(candidate: &Path, shim_candidates: &[PathBuf]) -> Result<()> {
  for shim in shim_candidates.iter().filter(|path| path.is_file()) {
    if same_file_identity(candidate, shim).with_context(|| {
      format!(
        "compare real-Caddy candidate {} with Cadder shim {}",
        candidate.display(),
        shim.display()
      )
    })? {
      return Err(anyhow!("resolved executable is the Cadder Caddy shim"));
    }
  }
  Ok(())
}

#[cfg(windows)]
pub(super) fn scoop_shim_target(candidate: &Path) -> Option<PathBuf> {
  let descriptor = candidate.with_extension("shim");
  let contents = std::fs::read_to_string(&descriptor).ok()?;
  let target = contents.lines().find_map(scoop_shim_target_value)?;
  Some(if target.is_absolute() {
    target
  } else {
    descriptor
      .parent()
      .map(|parent| parent.join(&target))
      .unwrap_or(target)
  })
}

#[cfg(windows)]
pub(super) fn scoop_shim_target_value(line: &str) -> Option<PathBuf> {
  let value = line.trim().strip_prefix("path")?.trim_start();
  let value = value.strip_prefix('=')?.trim();
  let value = value.strip_prefix('"')?.strip_suffix('"')?;
  (!value.is_empty()).then(|| PathBuf::from(value))
}

#[cfg(windows)]
pub(super) fn shim_binary_names() -> [&'static str; 2] {
  ["cadder-caddy.exe", "caddy.exe"]
}

#[cfg(not(windows))]
pub(super) fn shim_binary_names() -> [&'static str; 2] {
  ["cadder-caddy", "caddy"]
}

pub(super) fn executable_candidates(dir: &Path, command: &str) -> Vec<PathBuf> {
  #[cfg(windows)]
  {
    let path = Path::new(command);
    if path.extension().is_some() {
      vec![dir.join(path)]
    } else {
      vec![dir.join(format!("{command}.exe"))]
    }
  }

  #[cfg(not(windows))]
  {
    vec![dir.join(command)]
  }
}

pub(super) const CADDY_METADATA_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const MAX_CADDY_METADATA_BYTES: usize = 1024 * 1024;

pub(super) async fn run_pinned_metadata_command(
  image: &OpenedCaddyImage,
  args: &[&str],
) -> Result<Vec<u8>> {
  let operation = format!("caddy {}", args.join(" "));
  let child = image
    .spawn(&operation, |command| {
      command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    })
    .await?;
  let output = child
    .wait_for_bounded_output(CADDY_METADATA_TIMEOUT, &operation, MAX_CADDY_METADATA_BYTES)
    .await?;
  if !output.status.success() {
    return Err(anyhow!(
      "{operation} failed: {}",
      String::from_utf8_lossy(&output.stderr).trim()
    ));
  }
  Ok(output.stdout)
}

pub(super) fn parse_caddy_version(output: &[u8]) -> Result<Version> {
  let output = std::str::from_utf8(output).context("decode caddy version output as UTF-8")?;
  let token = output
    .split_whitespace()
    .next()
    .context("caddy version returned empty output")?;
  let token = token.strip_prefix('v').unwrap_or(token);
  Version::parse(token).with_context(|| format!("parse Caddy semantic version `{token}`"))
}

pub(super) fn parse_caddy_modules(output: &[u8]) -> Result<BTreeSet<String>> {
  #[derive(serde::Deserialize)]
  struct ModuleInfo {
    module_name: String,
  }

  let modules = serde_json::from_slice::<Vec<ModuleInfo>>(output)
    .context("decode caddy list-modules --json output")?
    .into_iter()
    .map(|module| module.module_name)
    .filter(|module| !module.trim().is_empty())
    .collect::<BTreeSet<_>>();
  if modules.is_empty() {
    return Err(anyhow!("caddy list-modules returned no module identifiers"));
  }
  Ok(modules)
}
