#[cfg(any(test, debug_assertions))]
use crate::caddy_image::{MINIMUM_CADDY_VERSION, required_caddy_modules};
use crate::{
  caddy_image::{
    CADDY_COMPATIBILITY_PROBE_REVISION, CaddyImageSource, OpenedCaddyImage, PinnedCaddyImage,
    VerifiedCaddyImage,
  },
  caddy_path_trust::{
    CaddyPathProvenance, same_file_identity, validate_trusted_config, validate_trusted_executable,
  },
  config::{CONFIG_FILE_NAME, CadderConfig},
  logs::CaddyLogStore,
  paths::{RuntimePaths, RuntimeProfile},
  runtime::{CaddyRuntime, ProcessRuntime},
};
use anyhow::{Context, Result, anyhow};
use cadder_protocol::{
  ConfigApplyStatus, ConfigDiagnostic, ConfigState, EntrypointRegistration, LogAttributionKind,
  LogSeverity, RegisteredDomain,
};
use chrono::Utc;
use semver::Version;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
  collections::{BTreeMap, BTreeSet},
  env, fmt,
  path::{Path, PathBuf},
  process::Stdio,
  str::FromStr,
  sync::{Arc, OnceLock},
  time::Duration,
};
use tokio::sync::OnceCell;

pub const CADDER_CADDY_BACKEND_ENV: &str = "CADDER_CADDY_BACKEND";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CaddyBackendMode {
  #[default]
  Real,
  Mock,
}

impl CaddyBackendMode {
  pub fn from_env() -> Result<Self> {
    env::var(CADDER_CADDY_BACKEND_ENV)
      .ok()
      .map_or(Ok(Self::Real), |value| value.parse())
  }

  pub fn parse_cli(value: &str) -> std::result::Result<Self, String> {
    value.parse::<Self>().map_err(|error| error.to_string())
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::Real => "real",
      Self::Mock => "mock",
    }
  }
}

impl fmt::Display for CaddyBackendMode {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for CaddyBackendMode {
  type Err = anyhow::Error;

  fn from_str(value: &str) -> Result<Self> {
    match value.trim().to_ascii_lowercase().as_str() {
      "" | "real" | "default" => Ok(Self::Real),
      "mock" | "dev" => Ok(Self::Mock),
      other => Err(anyhow!(
        "unknown Cadder Caddy backend `{other}`; expected `real` or `mock`"
      )),
    }
  }
}

#[derive(Debug, Clone)]
pub struct RealCaddyResolver {
  explicit_override: Option<PathBuf>,
  profile: RuntimeProfile,
  config_paths: TrustedConfigPaths,
  executable_path: Option<PathBuf>,
  trust_policy: CaddyTrustPolicy,
  resolved: Arc<OnceLock<ResolvedCaddyPath>>,
  pinned: Arc<OnceCell<Arc<PinnedCaddyImage>>>,
}

#[derive(Debug, Clone)]
struct ResolvedCaddyPath {
  path: PathBuf,
  source: CaddyImageSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaddyTrustPolicy {
  Enforce,
  #[cfg(any(test, debug_assertions))]
  TestFixture,
}

impl RealCaddyResolver {
  pub fn from_trusted_sources(profile: RuntimeProfile) -> Self {
    Self::for_daemon(None, profile)
  }

  pub(crate) fn for_daemon(explicit_override: Option<PathBuf>, profile: RuntimeProfile) -> Self {
    Self {
      explicit_override,
      profile,
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
    profile: RuntimeProfile,
    config_paths: TrustedConfigPaths,
    executable_path: Option<PathBuf>,
  ) -> Self {
    Self {
      explicit_override,
      profile,
      config_paths,
      executable_path,
      trust_policy: CaddyTrustPolicy::Enforce,
      resolved: Arc::new(OnceLock::new()),
      pinned: Arc::new(OnceCell::new()),
    }
  }

  #[cfg(test)]
  fn with_executable_path(
    explicit_override: Option<String>,
    executable_path: Option<PathBuf>,
  ) -> Self {
    let mut resolver = Self::with_sources(
      explicit_override.map(PathBuf::from),
      RuntimeProfile::Default,
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
  fn with_test_sources(
    explicit_override: Option<PathBuf>,
    profile: RuntimeProfile,
    config_paths: TrustedConfigPaths,
    executable_path: Option<PathBuf>,
  ) -> Self {
    let mut resolver =
      Self::with_sources(explicit_override, profile, config_paths, executable_path);
    resolver.trust_policy = CaddyTrustPolicy::TestFixture;
    resolver
  }

  #[cfg(debug_assertions)]
  #[doc(hidden)]
  pub fn for_test_fixture(path: PathBuf) -> Self {
    let mut resolver = Self::for_daemon(Some(path), RuntimeProfile::Default);
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
    opened.reverify_path()?;

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
        CaddyPathProvenance::UserOwned,
        &shim_candidates,
      );
    }

    if let Some(path) = &self.config_paths.user
      && let Some(selected) = self.selection_from_config(
        path,
        CaddyImageSource::UserConfiguration,
        CaddyPathProvenance::UserOwned,
      )?
    {
      return self.resolve_selected(
        &selected,
        CaddyImageSource::UserConfiguration,
        CaddyPathProvenance::UserOwned,
        &shim_candidates,
      );
    }

    if let Some(path) = &self.config_paths.system
      && let Some(selected) = self.selection_from_config(
        path,
        CaddyImageSource::SystemConfiguration,
        CaddyPathProvenance::SystemOwned,
      )?
    {
      return self.resolve_selected(
        &selected,
        CaddyImageSource::SystemConfiguration,
        CaddyPathProvenance::SystemOwned,
        &shim_candidates,
      );
    }

    resolve_caddy_on_path(&shim_candidates, self.trust_policy)
      .map(|path| ResolvedCaddyPath {
        path,
        source: CaddyImageSource::Path,
      })
      .context(
        "could not resolve a trusted real Caddy executable. Pass an absolute daemon override, \
         configure an absolute path in the per-user or system Cadder configuration, or install \
         a trusted caddy executable on PATH",
      )
  }

  pub fn resolution_help(error: &anyhow::Error) -> String {
    format!(
      "Cadder could not resolve a trusted real Caddy executable.\n\n\
       Cause: {error}\n\n\
       Configure real Caddy with one of these trusted sources, in precedence order:\n\
       - an absolute --real-caddy daemon-start override\n\
       - defaults.real_caddy or profiles.<profile>.real_caddy in the per-user Cadder configuration\n\
       - the same key in the administrator-owned system Cadder configuration\n\
       - a trusted real caddy executable on PATH\n\n\
       Project files, registration working directories, environment selectors, and shim flags never select the executable."
    )
  }

  fn selection_from_config(
    &self,
    path: &Path,
    source: CaddyImageSource,
    provenance: CaddyPathProvenance,
  ) -> Result<Option<PathBuf>> {
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
        let trusted = validate_trusted_config(path, provenance)
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
    let selected = config
      .real_caddy_for_profile(self.profile)
      .map(PathBuf::from);
    if let Some(selected) = &selected
      && !selected.is_absolute()
    {
      return Err(anyhow!(
        "{source_description} {} selects relative real-Caddy path {}; use an absolute path",
        path.display(),
        selected.display()
      ));
    }
    Ok(selected)
  }

  fn resolve_selected(
    &self,
    path: &Path,
    source: CaddyImageSource,
    provenance: CaddyPathProvenance,
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
      CaddyTrustPolicy::Enforce => {
        validate_trusted_executable(path, provenance).with_context(|| {
          format!(
            "validate real Caddy from {source_description}: {}",
            path.display()
          )
        })?
      }
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
struct TrustedConfigPaths {
  user: Option<PathBuf>,
  system: Option<PathBuf>,
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
fn system_config_path() -> Option<PathBuf> {
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
fn system_config_path() -> Option<PathBuf> {
  Some(PathBuf::from("/Library/Application Support/Cadder").join(CONFIG_FILE_NAME))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn system_config_path() -> Option<PathBuf> {
  Some(PathBuf::from("/etc/cadder").join(CONFIG_FILE_NAME))
}

#[cfg(not(any(unix, windows)))]
fn system_config_path() -> Option<PathBuf> {
  None
}

fn resolve_caddy_on_path(
  shim_candidates: &[PathBuf],
  trust_policy: CaddyTrustPolicy,
) -> Result<PathBuf> {
  let path_var = env::var_os("PATH").ok_or_else(|| anyhow!("PATH is not set"))?;
  for dir in env::split_paths(&path_var) {
    if !dir.is_absolute() {
      continue;
    }
    for candidate in executable_candidates(&dir) {
      if !candidate.is_file() {
        continue;
      }
      let canonical = match trust_policy {
        CaddyTrustPolicy::Enforce => {
          let Ok(canonical) =
            validate_trusted_executable(&candidate, CaddyPathProvenance::UserOwned)
          else {
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
      if reject_shim_identity(&canonical, shim_candidates).is_ok() {
        return Ok(canonical);
      }
    }
  }
  Err(anyhow!("trusted executable `caddy` not found on PATH"))
}

fn reject_shim_identity(candidate: &Path, shim_candidates: &[PathBuf]) -> Result<()> {
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
fn shim_binary_names() -> [&'static str; 2] {
  ["cadder-caddy.exe", "caddy.exe"]
}

#[cfg(not(windows))]
fn shim_binary_names() -> [&'static str; 2] {
  ["cadder-caddy", "caddy"]
}

fn executable_candidates(dir: &Path) -> Vec<PathBuf> {
  #[cfg(windows)]
  {
    vec![dir.join("caddy.exe")]
  }

  #[cfg(not(windows))]
  {
    vec![dir.join("caddy")]
  }
}

const CADDY_METADATA_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CADDY_METADATA_BYTES: usize = 1024 * 1024;

async fn run_pinned_metadata_command(image: &OpenedCaddyImage, args: &[&str]) -> Result<Vec<u8>> {
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

fn parse_caddy_version(output: &[u8]) -> Result<Version> {
  let output = std::str::from_utf8(output).context("decode caddy version output as UTF-8")?;
  let token = output
    .split_whitespace()
    .next()
    .context("caddy version returned empty output")?;
  let token = token.strip_prefix('v').unwrap_or(token);
  Version::parse(token).with_context(|| format!("parse Caddy semantic version `{token}`"))
}

fn parse_caddy_modules(output: &[u8]) -> Result<BTreeSet<String>> {
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

#[derive(Debug, Clone)]
pub struct CaddyConfigAdapter {
  resolver: RealCaddyResolver,
  command_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct PreparedRegistration {
  pub registration: EntrypointRegistration,
  pub routes: Vec<Value>,
  pub diagnostics: Vec<ConfigDiagnostic>,
}

impl CaddyConfigAdapter {
  pub fn new(resolver: RealCaddyResolver) -> Self {
    Self {
      resolver,
      command_timeout: Duration::from_secs(30),
    }
  }

  pub fn with_command_timeout(resolver: RealCaddyResolver, command_timeout: Duration) -> Self {
    Self {
      resolver,
      command_timeout,
    }
  }

  pub async fn prepare(&self, registration: EntrypointRegistration) -> PreparedRegistration {
    match self.adapt(&registration).await {
      Ok(adapted) => {
        let domains = extract_registered_domains(&adapted);
        let mut prepared = registration;
        if !domains.is_empty() {
          prepared.registered_domains = domains;
        }
        let routes = extract_http_routes(&adapted);
        PreparedRegistration {
          registration: prepared,
          routes,
          diagnostics: Vec::new(),
        }
      }
      Err(error) => PreparedRegistration {
        registration,
        routes: Vec::new(),
        diagnostics: vec![ConfigDiagnostic {
          code: "adapt-failed".to_string(),
          message: error.to_string(),
          domain_key: None,
          source_config_paths: Vec::new(),
        }],
      },
    }
  }

  async fn adapt(&self, registration: &EntrypointRegistration) -> Result<Value> {
    let image = self.resolver.verify_for_spawn().await?;
    let config_path = registration
      .source_config_path
      .canonical
      .as_deref()
      .unwrap_or(&registration.source_config_path.raw);
    let adapter = registration
      .shim_run
      .as_ref()
      .and_then(|run| run.adapter.as_deref())
      .unwrap_or("caddyfile");

    let child = image
      .spawn("caddy adapt", |command| {
        command
          .arg("adapt")
          .arg("--config")
          .arg(config_path)
          .arg("--adapter")
          .arg(adapter)
          .stdout(Stdio::piped())
          .stderr(Stdio::piped());
      })
      .await?;
    let output = child
      .wait_for_output(self.command_timeout, "caddy adapt")
      .await?;

    if !output.status.success() {
      return Err(anyhow!(
        "caddy adapt failed: {}",
        String::from_utf8_lossy(&output.stderr)
      ));
    }

    serde_json::from_slice(&output.stdout).context("parse adapted Caddy JSON")
  }
}

#[derive(Debug, Clone)]
pub enum CaddyRegistrationAdapter {
  Real(CaddyConfigAdapter),
  Mock(MockCaddyConfigAdapter),
}

impl CaddyRegistrationAdapter {
  pub async fn prepare(&self, registration: EntrypointRegistration) -> PreparedRegistration {
    match self {
      Self::Real(adapter) => adapter.prepare(registration).await,
      Self::Mock(adapter) => adapter.prepare(registration).await,
    }
  }
}

impl From<CaddyConfigAdapter> for CaddyRegistrationAdapter {
  fn from(adapter: CaddyConfigAdapter) -> Self {
    Self::Real(adapter)
  }
}

#[derive(Debug, Clone, Default)]
pub struct MockCaddyConfigAdapter;

impl MockCaddyConfigAdapter {
  pub async fn prepare(&self, mut registration: EntrypointRegistration) -> PreparedRegistration {
    if registration.registered_domains.is_empty() {
      let config_path = registration
        .source_config_path
        .canonical
        .as_deref()
        .unwrap_or(&registration.source_config_path.raw);
      if let Ok(config) = tokio::fs::read_to_string(config_path).await {
        let domains = mock_caddyfile_hosts(&config)
          .into_iter()
          .map(RegisteredDomain::active)
          .collect::<Vec<_>>();
        if !domains.is_empty() {
          registration.registered_domains = domains;
        }
      }
    }

    let routes = registration
      .registered_domains
      .iter()
      .map(|domain| mock_route_for_domain(&domain.name.canonical))
      .collect();
    PreparedRegistration {
      registration,
      routes,
      diagnostics: Vec::new(),
    }
  }
}

#[derive(Debug, Clone)]
pub struct CaddyConfigCoordinator {
  adapter: CaddyRegistrationAdapter,
  runtime: CaddyRuntime,
  routes: BTreeMap<String, Vec<Value>>,
  iis_routes: BTreeMap<String, IisProxyRoute>,
  registration_diagnostics: BTreeMap<String, Vec<ConfigDiagnostic>>,
  current: ConfigState,
}

#[derive(Debug)]
pub enum CaddyApplyAction {
  Current(ConfigState),
  Stop {
    attempted: chrono::DateTime<Utc>,
  },
  Apply {
    attempted: chrono::DateTime<Utc>,
    rendered: Vec<u8>,
    hash: String,
    source_config_paths: Vec<String>,
  },
}

#[derive(Debug, Clone)]
struct IisProxyRoute {
  domain_key: String,
  route: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IisProxyBackendProtocol {
  Http,
  Https,
}

impl IisProxyBackendProtocol {
  pub fn from_iis_protocol(protocol: &str) -> Self {
    if protocol.eq_ignore_ascii_case("https") {
      Self::Https
    } else {
      Self::Http
    }
  }
}

impl CaddyConfigCoordinator {
  pub fn new(adapter: CaddyConfigAdapter, runtime: ProcessRuntime) -> Self {
    Self::with_backend(adapter.into(), runtime.into())
  }

  pub fn new_mock(paths: RuntimePaths) -> Self {
    Self::with_backend(
      CaddyRegistrationAdapter::Mock(MockCaddyConfigAdapter),
      CaddyRuntime::mock(paths),
    )
  }

  pub fn with_backend(adapter: CaddyRegistrationAdapter, runtime: CaddyRuntime) -> Self {
    Self {
      adapter,
      runtime,
      routes: BTreeMap::new(),
      iis_routes: BTreeMap::new(),
      registration_diagnostics: BTreeMap::new(),
      current: ConfigState::idle(),
    }
  }

  pub fn current_state(&self) -> ConfigState {
    self.current.clone()
  }

  pub fn adapter(&self) -> CaddyRegistrationAdapter {
    self.adapter.clone()
  }

  pub fn runtime(&self) -> CaddyRuntime {
    self.runtime.clone()
  }

  pub async fn runtime_state(&self) -> cadder_protocol::RuntimeState {
    self.runtime.inspect().await
  }

  pub fn set_iis_proxy_route(
    &mut self,
    binding_id: impl Into<String>,
    domain_key: impl Into<String>,
    backend_dial: impl Into<String>,
    backend_protocol: IisProxyBackendProtocol,
  ) {
    let binding_id = binding_id.into();
    let domain_key = domain_key.into();
    let backend_dial = backend_dial.into();
    let route = iis_proxy_route(&binding_id, &domain_key, &backend_dial, backend_protocol);
    self
      .iis_routes
      .insert(domain_key.clone(), IisProxyRoute { domain_key, route });
  }

  pub fn remove_iis_proxy_route(&mut self, domain_key: &str) {
    self.iis_routes.remove(domain_key);
  }

  pub fn has_iis_proxy_routes_except(&self, domain_key: &str) -> bool {
    self
      .iis_routes
      .values()
      .any(|route| !route.domain_key.eq_ignore_ascii_case(domain_key))
  }

  pub async fn prepare_registration(
    &mut self,
    registration: EntrypointRegistration,
  ) -> EntrypointRegistration {
    let source_path = registration.source_config_path.raw.clone();
    let prepared = self.adapter.prepare(registration).await;
    self.commit_prepared_registration(source_path, prepared)
  }

  pub fn commit_prepared_registration(
    &mut self,
    source_path: String,
    prepared: PreparedRegistration,
  ) -> EntrypointRegistration {
    if prepared.diagnostics.is_empty() {
      self
        .registration_diagnostics
        .remove(&prepared.registration.registration_id);
      self.routes.insert(
        prepared.registration.registration_id.clone(),
        prepared.routes,
      );
    } else {
      self.routes.remove(&prepared.registration.registration_id);
      let diagnostics = prepared
        .diagnostics
        .into_iter()
        .map(|mut diagnostic| {
          if diagnostic.source_config_paths.is_empty() {
            diagnostic.source_config_paths = vec![source_path.clone()];
          }
          diagnostic
        })
        .collect::<Vec<_>>();
      self.registration_diagnostics.insert(
        prepared.registration.registration_id.clone(),
        diagnostics.clone(),
      );
      self.current = ConfigState {
        status: ConfigApplyStatus::Failed,
        last_attempted_at_utc: Some(Utc::now()),
        last_successful_reload_at_utc: self.current.last_successful_reload_at_utc,
        effective_config_hash: self.current.effective_config_hash.clone(),
        diagnostics,
      };
    }
    prepared.registration
  }

  pub fn begin_apply(&mut self, registrations: &[EntrypointRegistration]) -> CaddyApplyAction {
    let active_registration_ids = registrations
      .iter()
      .map(|registration| registration.registration_id.clone())
      .collect::<BTreeSet<_>>();
    self
      .routes
      .retain(|registration_id, _| active_registration_ids.contains(registration_id));
    self
      .registration_diagnostics
      .retain(|registration_id, _| active_registration_ids.contains(registration_id));

    let enabled_registration_ids = registrations
      .iter()
      .filter(|registration| registration.activation_state.is_enabled())
      .map(|registration| registration.registration_id.clone())
      .collect::<BTreeSet<_>>();
    let mut diagnostics = self
      .registration_diagnostics
      .iter()
      .filter(|(registration_id, _)| enabled_registration_ids.contains(*registration_id))
      .flat_map(|(_, diagnostics)| diagnostics.iter().cloned())
      .collect::<Vec<_>>();
    diagnostics.extend(detect_conflicts(registrations));
    diagnostics.extend(detect_iis_route_conflicts(registrations, &self.iis_routes));
    let attempted = Utc::now();
    if !diagnostics.is_empty() {
      self.current = ConfigState {
        status: ConfigApplyStatus::Failed,
        last_attempted_at_utc: Some(attempted),
        last_successful_reload_at_utc: self.current.last_successful_reload_at_utc,
        effective_config_hash: self.current.effective_config_hash.clone(),
        diagnostics,
      };
      return CaddyApplyAction::Current(self.current.clone());
    }

    let active: Vec<_> = registrations
      .iter()
      .filter(|registration| registration.activation_state.is_enabled())
      .cloned()
      .collect();
    if self.iis_routes.is_empty()
      && active
        .iter()
        .all(|registration| active_domains(registration).is_empty())
    {
      return CaddyApplyAction::Stop { attempted };
    }

    let config = compose_config(&active, &self.routes, &self.iis_routes);
    let rendered = serde_json::to_vec_pretty(&config).expect("config serialization");
    let hash = hex::encode(Sha256::digest(&rendered));
    let source_config_paths = active
      .iter()
      .map(|registration| registration.source_config_path.raw.clone())
      .collect();

    CaddyApplyAction::Apply {
      attempted,
      rendered,
      hash,
      source_config_paths,
    }
  }

  pub fn finish_idle(&mut self, attempted: chrono::DateTime<Utc>) -> ConfigState {
    self.current = ConfigState {
      status: ConfigApplyStatus::Idle,
      last_attempted_at_utc: Some(attempted),
      last_successful_reload_at_utc: self.current.last_successful_reload_at_utc,
      effective_config_hash: None,
      diagnostics: Vec::new(),
    };
    self.current.clone()
  }

  pub fn finish_runtime_apply(
    &mut self,
    attempted: chrono::DateTime<Utc>,
    hash: String,
    source_config_paths: Vec<String>,
    result: Result<()>,
  ) -> ConfigState {
    match result {
      Ok(()) => {
        self.current = ConfigState {
          status: ConfigApplyStatus::Applied,
          last_attempted_at_utc: Some(attempted),
          last_successful_reload_at_utc: Some(Utc::now()),
          effective_config_hash: Some(hash),
          diagnostics: Vec::new(),
        };
      }
      Err(error) => {
        self.current = ConfigState {
          status: ConfigApplyStatus::Failed,
          last_attempted_at_utc: Some(attempted),
          last_successful_reload_at_utc: self.current.last_successful_reload_at_utc,
          effective_config_hash: self.current.effective_config_hash.clone(),
          diagnostics: vec![ConfigDiagnostic {
            code: "runtime-apply-failed".to_string(),
            message: error.to_string(),
            domain_key: None,
            source_config_paths,
          }],
        };
      }
    }
    self.current.clone()
  }

  pub async fn apply(
    &mut self,
    registrations: &[EntrypointRegistration],
    logs: &CaddyLogStore,
  ) -> ConfigState {
    match self.begin_apply(registrations) {
      CaddyApplyAction::Current(state) => state,
      CaddyApplyAction::Stop { attempted } => {
        if let Err(error) = self.runtime.stop().await {
          logs.append(
            cadder_protocol::LogStreamIdentity::runtime_control(),
            LogSeverity::Error,
            error.to_string(),
            LogAttributionKind::RuntimeControl,
            Some("idle-stop".to_string()),
          );
        }
        self.finish_idle(attempted)
      }
      CaddyApplyAction::Apply {
        attempted,
        rendered,
        hash,
        source_config_paths,
      } => {
        let result = self.runtime.apply_config(&rendered, logs).await;
        self.finish_runtime_apply(attempted, hash, source_config_paths, result)
      }
    }
  }

  pub async fn shutdown(&mut self) -> Result<()> {
    self.runtime.stop().await
  }
}

fn iis_proxy_route(
  binding_id: &str,
  domain_key: &str,
  backend_dial: &str,
  backend_protocol: IisProxyBackendProtocol,
) -> Value {
  let mut handler = json!({
      "handler": "reverse_proxy",
      "upstreams": [{ "dial": backend_dial }]
  });
  if backend_protocol == IisProxyBackendProtocol::Https {
    handler["transport"] = json!({
        "protocol": "http",
        "tls": {
            "server_name": domain_key,
            "insecure_skip_verify": true
        }
    });
  }
  json!({
      "@id": format!("iis_handoff_{}", route_id_fragment(binding_id)),
      "match": [{ "host": [domain_key] }],
      "handle": [handler],
      "terminal": true
  })
}

fn compose_config(
  registrations: &[EntrypointRegistration],
  routes_by_registration: &BTreeMap<String, Vec<Value>>,
  iis_routes: &BTreeMap<String, IisProxyRoute>,
) -> Value {
  let mut routes = Vec::new();
  let mut tls_subjects = BTreeSet::new();
  for registration in registrations {
    let enabled_hosts = active_domains(registration);
    if enabled_hosts.is_empty() {
      continue;
    }
    tls_subjects.extend(enabled_hosts.iter().cloned());

    if let Some(source_routes) = routes_by_registration.get(&registration.registration_id) {
      for route in source_routes {
        if let Some(filtered) = filter_route_hosts(route.clone(), &enabled_hosts) {
          routes.push(filtered);
        }
      }
    } else {
      for host in enabled_hosts {
        routes.push(json!({
            "match": [{ "host": [host] }],
            "handle": [{ "handler": "static_response", "body": "Cadder route placeholder" }],
            "terminal": true
        }));
      }
    }
  }
  tls_subjects.extend(iis_routes.keys().cloned());
  routes.extend(iis_routes.values().map(|route| route.route.clone()));
  let tls_subjects = tls_subjects.into_iter().collect::<Vec<_>>();
  let http_routes = routes.clone();

  json!({
      "admin": { "listen": "localhost:2019" },
      "apps": {
          "http": {
              "servers": {
                  "cadder_http": {
                      "listen": [":80"],
                      "routes": http_routes
                  },
                  "cadder_https": {
                      "listen": [":443"],
                      "tls_connection_policies": [{}],
                      "routes": routes
                  }
              }
          },
          "tls": {
              "automation": {
                  "policies": [{
                      "subjects": tls_subjects,
                      "issuers": [{ "module": "internal" }]
                  }]
              }
          }
      }
  })
}

fn mock_route_for_domain(domain: &str) -> Value {
  json!({
      "match": [{ "host": [domain] }],
      "handle": [{ "handler": "static_response", "body": "Cadder mock Caddy route" }],
      "terminal": true
  })
}

fn mock_caddyfile_hosts(config: &str) -> BTreeSet<String> {
  let mut hosts = BTreeSet::new();
  for line in config.lines() {
    let line = line.split('#').next().unwrap_or_default().trim();
    let Some((site_labels, _)) = line.split_once('{') else {
      continue;
    };
    for token in site_labels.split([',', ' ', '\t']) {
      if let Some(host) = mock_host_from_site_token(token) {
        hosts.insert(cadder_protocol::canonicalize_domain(&host));
      }
    }
  }
  hosts
}

fn mock_host_from_site_token(token: &str) -> Option<String> {
  let token = token
    .trim()
    .trim_matches('"')
    .trim_matches('\'')
    .trim_end_matches(',');
  if token.is_empty() || token.starts_with('@') || token.starts_with(':') {
    return None;
  }

  let without_scheme = token
    .split_once("://")
    .map(|(_, rest)| rest)
    .unwrap_or(token);
  let without_path = without_scheme
    .split_once('/')
    .map(|(host, _)| host)
    .unwrap_or(without_scheme);
  let host = without_path
    .rsplit_once(':')
    .filter(|(_, port)| port.chars().all(|ch| ch.is_ascii_digit()))
    .map(|(host, _)| host)
    .unwrap_or(without_path)
    .trim_matches('.');

  if host.eq_ignore_ascii_case("localhost") || host.contains('.') {
    Some(host.to_string())
  } else {
    None
  }
}

fn route_id_fragment(value: &str) -> String {
  value
    .chars()
    .map(|ch| {
      if ch.is_ascii_alphanumeric() {
        ch.to_ascii_lowercase()
      } else {
        '_'
      }
    })
    .collect()
}

fn active_domains(registration: &EntrypointRegistration) -> BTreeSet<String> {
  registration
    .registered_domains
    .iter()
    .filter(|domain| domain.activation_state.is_enabled())
    .map(|domain| domain.name.canonical.clone())
    .collect()
}

fn filter_route_hosts(mut route: Value, enabled_hosts: &BTreeSet<String>) -> Option<Value> {
  let mut retained_any = false;
  filter_hosts_recursive(&mut route, enabled_hosts, &mut retained_any);
  retained_any.then_some(route)
}

fn filter_hosts_recursive(
  value: &mut Value,
  enabled_hosts: &BTreeSet<String>,
  retained_any: &mut bool,
) {
  match value {
    Value::Object(map) => {
      if let Some(Value::Array(hosts)) = map.get_mut("host") {
        hosts.retain(|host| {
          let keep = host
            .as_str()
            .map(cadder_protocol::canonicalize_domain)
            .is_some_and(|host| enabled_hosts.contains(&host));
          if keep {
            *retained_any = true;
          }
          keep
        });
      }
      for child in map.values_mut() {
        filter_hosts_recursive(child, enabled_hosts, retained_any);
      }
    }
    Value::Array(items) => {
      for item in items {
        filter_hosts_recursive(item, enabled_hosts, retained_any);
      }
    }
    _ => {}
  }
}

fn extract_hosts(value: &Value) -> BTreeSet<String> {
  let mut hosts = BTreeSet::new();
  collect_hosts(value, &mut hosts);
  hosts
}

fn extract_registered_domains(value: &Value) -> Vec<RegisteredDomain> {
  let upstreams = extract_upstreams_by_host(value);
  extract_hosts(value)
    .into_iter()
    .map(|host| {
      let mut domain = RegisteredDomain::active(&host);
      domain.upstream = upstreams.get(&host).cloned();
      domain
    })
    .collect()
}

fn extract_upstreams_by_host(value: &Value) -> BTreeMap<String, String> {
  let mut upstreams = BTreeMap::new();
  for route in extract_http_routes(value) {
    let Some(upstream) = first_reverse_proxy_dial(&route) else {
      continue;
    };
    for host in extract_hosts(&route) {
      upstreams.entry(host).or_insert_with(|| upstream.clone());
    }
  }
  upstreams
}

fn first_reverse_proxy_dial(value: &Value) -> Option<String> {
  match value {
    Value::Object(map) => {
      if map.get("handler").and_then(Value::as_str) == Some("reverse_proxy")
        && let Some(dial) = map
          .get("upstreams")
          .and_then(Value::as_array)
          .and_then(|upstreams| {
            upstreams
              .iter()
              .filter_map(|upstream| upstream.get("dial").and_then(Value::as_str))
              .find(|dial| !dial.trim().is_empty())
          })
      {
        return Some(dial.to_string());
      }
      map.values().find_map(first_reverse_proxy_dial)
    }
    Value::Array(items) => items.iter().find_map(first_reverse_proxy_dial),
    _ => None,
  }
}

fn collect_hosts(value: &Value, hosts: &mut BTreeSet<String>) {
  match value {
    Value::Object(map) => {
      if let Some(Value::Array(values)) = map.get("host") {
        for value in values {
          if let Some(host) = value.as_str() {
            hosts.insert(cadder_protocol::canonicalize_domain(host));
          }
        }
      }
      for child in map.values() {
        collect_hosts(child, hosts);
      }
    }
    Value::Array(items) => {
      for item in items {
        collect_hosts(item, hosts);
      }
    }
    _ => {}
  }
}

fn extract_http_routes(value: &Value) -> Vec<Value> {
  value
    .pointer("/apps/http/servers")
    .and_then(Value::as_object)
    .map(|servers| {
      servers
        .values()
        .filter_map(|server| server.get("routes"))
        .filter_map(Value::as_array)
        .flat_map(|routes| routes.iter().cloned())
        .collect()
    })
    .unwrap_or_default()
}

fn detect_conflicts(registrations: &[EntrypointRegistration]) -> Vec<ConfigDiagnostic> {
  let mut owners: BTreeMap<String, Vec<&EntrypointRegistration>> = BTreeMap::new();
  for registration in registrations
    .iter()
    .filter(|registration| registration.activation_state.is_enabled())
  {
    for domain in registration
      .registered_domains
      .iter()
      .filter(|domain| domain.activation_state.is_enabled())
    {
      owners
        .entry(domain.name.canonical.clone())
        .or_default()
        .push(registration);
    }
  }

  owners
    .into_iter()
    .filter_map(|(domain, registrations)| {
      (registrations.len() > 1).then(|| ConfigDiagnostic {
        code: "domain-conflict".to_string(),
        message: format!("domain `{domain}` is registered by multiple entrypoints"),
        domain_key: Some(domain),
        source_config_paths: registrations
          .into_iter()
          .map(|registration| registration.source_config_path.raw.clone())
          .collect(),
      })
    })
    .collect()
}

fn detect_iis_route_conflicts(
  registrations: &[EntrypointRegistration],
  iis_routes: &BTreeMap<String, IisProxyRoute>,
) -> Vec<ConfigDiagnostic> {
  registrations
    .iter()
    .filter(|registration| registration.activation_state.is_enabled())
    .flat_map(|registration| {
      registration
        .registered_domains
        .iter()
        .filter(|domain| domain.activation_state.is_enabled())
        .filter_map(|domain| {
          iis_routes
            .get(&domain.name.canonical)
            .map(|iis_route| ConfigDiagnostic {
              code: "iis-domain-conflict".to_string(),
              message: format!(
                "domain `{}` is already owned by an IIS handoff route",
                iis_route.domain_key
              ),
              domain_key: Some(iis_route.domain_key.clone()),
              source_config_paths: vec![registration.source_config_path.raw.clone()],
            })
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{paths::RuntimePaths, runtime::RuntimeTimeouts};
  use cadder_protocol::{
    ActivationState, EntrypointInstanceIdentity, LogStreamIdentity, OwnerProcessIdentity,
    SourcePath,
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

  fn write_real_caddy_config(path: &Path, default: &Path, dev: Option<&Path>) {
    let escape = |value: &Path| {
      value
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
    };
    let mut config = format!("[defaults]\nreal_caddy = \"{}\"\n", escape(default));
    if let Some(dev) = dev {
      config.push_str(&format!(
        "[profiles.dev]\nreal_caddy = \"{}\"\n",
        escape(dev)
      ));
    }
    fs::write(path, config).unwrap();
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
        r#"#!/usr/bin/env sh
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
          r#"#!/usr/bin/env sh
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
        r#"#!/usr/bin/env sh
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
    write_real_caddy_config(&user_config, &user, None);
    write_real_caddy_config(&system_config, &system, None);
    let resolver = RealCaddyResolver::with_test_sources(
      Some(explicit.clone()),
      RuntimeProfile::Default,
      TrustedConfigPaths {
        user: Some(user_config),
        system: Some(system_config),
      },
      None,
    );

    assert_eq!(resolver.resolve().unwrap(), canonical(&explicit));
  }

  #[test]
  fn trusted_caddy_source_profile_and_file_precedence_is_stable() {
    let dir = tempfile::tempdir().unwrap();
    let user_default = dir.path().join(exe_name_for_test("user-default"));
    let user_dev = dir.path().join(exe_name_for_test("user-dev"));
    let system_default = dir.path().join(exe_name_for_test("system-default"));
    for path in [&user_default, &user_dev, &system_default] {
      write_file(path);
    }
    let user_config = dir.path().join("user.toml");
    let system_config = dir.path().join("system.toml");
    write_real_caddy_config(&user_config, &user_default, Some(&user_dev));
    write_real_caddy_config(&system_config, &system_default, None);
    let paths = TrustedConfigPaths {
      user: Some(user_config),
      system: Some(system_config),
    };

    let dev = RealCaddyResolver::with_test_sources(None, RuntimeProfile::Dev, paths.clone(), None);
    let default = RealCaddyResolver::with_test_sources(None, RuntimeProfile::Default, paths, None);

    assert_eq!(dev.resolve().unwrap(), canonical(&user_dev));
    assert_eq!(default.resolve().unwrap(), canonical(&user_default));
  }

  #[test]
  fn trusted_caddy_source_uses_system_profile_and_default_after_empty_user_config() {
    let dir = tempfile::tempdir().unwrap();
    let system_default = dir.path().join(exe_name_for_test("system-default"));
    let system_dev = dir.path().join(exe_name_for_test("system-dev"));
    write_file(&system_default);
    write_file(&system_dev);
    let user_config = dir.path().join("user.toml");
    let system_config = dir.path().join("system.toml");
    fs::write(&user_config, "[defaults]\n").unwrap();
    write_real_caddy_config(&system_config, &system_default, Some(&system_dev));
    let paths = TrustedConfigPaths {
      user: Some(user_config),
      system: Some(system_config),
    };

    let dev = RealCaddyResolver::with_test_sources(None, RuntimeProfile::Dev, paths.clone(), None);
    let default = RealCaddyResolver::with_test_sources(None, RuntimeProfile::Default, paths, None);

    assert_eq!(dev.resolve().unwrap(), canonical(&system_dev));
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
    fs::write(&user_config, "[defaults]\nreal_caddy = 'relative-caddy'\n").unwrap();
    write_real_caddy_config(&system_config, &system, None);
    let resolver = RealCaddyResolver::with_test_sources(
      None,
      RuntimeProfile::Default,
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
    write_real_caddy_config(&system_config, &system, None);
    let resolver = RealCaddyResolver::with_test_sources(
      None,
      RuntimeProfile::Default,
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
      RuntimeProfile::Default,
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
  fn trusted_caddy_source_ignores_project_executable_adjacent_and_environment_selectors() {
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
    write_real_caddy_config(&project.join(CONFIG_FILE_NAME), &rejected, None);
    write_real_caddy_config(&bin.join(CONFIG_FILE_NAME), &rejected, None);
    unsafe {
      env::set_var("CADDER_CADDY_REAL_COMMAND", &rejected);
      env::set_var("CADDER_CADDY__REAL_COMMAND", &rejected);
      env::set_var("PATH", env::join_paths([path_dir]).unwrap());
    }
    let resolver = RealCaddyResolver::with_test_sources(
      None,
      RuntimeProfile::Default,
      TrustedConfigPaths {
        user: None,
        system: None,
      },
      Some(bin.join(exe_name_for_test("cadderd"))),
    );

    assert_eq!(resolver.resolve().unwrap(), canonical(&selected));
  }

  #[tokio::test]
  async fn mock_adapter_prepares_domains_without_running_caddy_adapt() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("Caddyfile");
    fs::write(
      &config_path,
      r#"
app.localhost, http://api.localhost:8080 {
  respond ok
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
    assert_eq!(runtime.status, cadder_protocol::RuntimeStatus::Running);
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
    let resolver = RealCaddyResolver::with_executable_path(
      Some(fake_caddy.display().to_string()),
      Some(dir.path().join(exe_name_for_test("cadderd"))),
    );
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

    assert_eq!(prepared.registered_domains.len(), 1);
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
  fn trusted_caddy_source_resolution_help_names_only_trusted_sources() {
    let resolver = RealCaddyResolver::with_test_sources(
      Some(PathBuf::from("relative-caddy")),
      RuntimeProfile::Default,
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

  #[tokio::test]
  async fn coordinator_accessors_iis_routes_and_idle_shutdown_are_stable() {
    let mut coordinator = coordinator_for_test();

    assert_eq!(coordinator.current_state().status, ConfigApplyStatus::Idle);
    assert_eq!(
      coordinator.runtime_state().await.status,
      cadder_protocol::RuntimeStatus::Idle
    );
    assert!(!coordinator.has_iis_proxy_routes_except("app.localhost"));

    coordinator.set_iis_proxy_route(
      "Default Web Site|http|*:80:app.localhost",
      "app.localhost",
      "127.0.0.1:53000",
      IisProxyBackendProtocol::Http,
    );

    assert!(!coordinator.has_iis_proxy_routes_except("app.localhost"));
    assert!(coordinator.has_iis_proxy_routes_except("other.localhost"));
    let CaddyApplyAction::Apply {
      rendered,
      source_config_paths,
      ..
    } = coordinator.begin_apply(&[])
    else {
      panic!("expected IIS route-only apply action");
    };
    let config: Value = serde_json::from_slice(&rendered).unwrap();
    assert!(source_config_paths.is_empty());
    assert_eq!(
      config
        .pointer("/apps/http/servers/cadder_https/routes/0/match/0/host/0")
        .and_then(Value::as_str),
      Some("app.localhost")
    );

    coordinator.remove_iis_proxy_route("app.localhost");
    assert!(!coordinator.has_iis_proxy_routes_except("app.localhost"));
    coordinator.shutdown().await.unwrap();
  }

  #[test]
  fn trusted_caddy_source_rejects_explicit_shim_by_file_identity() {
    let dir = tempfile::tempdir().unwrap();
    let shim = dir.path().join(exe_name_for_test("caddy"));
    write_file(&shim);
    let resolver = RealCaddyResolver::with_test_sources(
      Some(shim.clone()),
      RuntimeProfile::Default,
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
  fn trusted_caddy_source_path_uses_only_the_caddy_executable_name() {
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
      RuntimeProfile::Default,
      TrustedConfigPaths {
        user: None,
        system: None,
      },
      Some(shim),
    );

    let resolved = resolver.resolve().unwrap();

    assert_eq!(resolved, canonical(&real));
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
    let config = compose_config(&registrations, &BTreeMap::new(), &BTreeMap::new());

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

    let config = compose_config(&[registration], &routes_by_registration, &BTreeMap::new());
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
  fn compose_config_drops_inactive_domain_routes_but_keeps_iis_subjects() {
    let mut registration = registration("shim", &["app.localhost"]);
    registration.registered_domains[0].activation_state = ActivationState::Inactive;
    let routes_by_registration = BTreeMap::from([(
      "shim".to_string(),
      vec![json!({
          "match": [{ "host": ["app.localhost"] }],
          "handle": [{ "handler": "static_response", "body": "app" }],
          "terminal": true
      })],
    )]);
    let iis_routes = BTreeMap::from([(
      "iis.localhost".to_string(),
      IisProxyRoute {
        domain_key: "iis.localhost".to_string(),
        route: iis_proxy_route(
          "Default Web Site|http|*:80:iis.localhost",
          "iis.localhost",
          "127.0.0.1:41080",
          IisProxyBackendProtocol::Http,
        ),
      },
    )]);

    let config = compose_config(&[registration], &routes_by_registration, &iis_routes);
    let routes = config
      .pointer("/apps/http/servers/cadder_https/routes")
      .and_then(Value::as_array)
      .unwrap();
    let tls_subjects = config
      .pointer("/apps/tls/automation/policies/0/subjects")
      .and_then(Value::as_array)
      .unwrap();

    assert_eq!(routes.len(), 1);
    assert_eq!(
      routes[0].pointer("/match/0/host/0").and_then(Value::as_str),
      Some("iis.localhost")
    );
    assert_eq!(tls_subjects, &[json!("iis.localhost")]);
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
    let resolver = RealCaddyResolver::from_trusted_sources(RuntimeProfile::Default);
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
    let proxy_route = IisProxyRoute {
      domain_key: "app.localhost".to_string(),
      route: iis_proxy_route(
        "Default Web Site|http|*:80:app.localhost",
        "app.localhost",
        "127.0.0.1:41000",
        IisProxyBackendProtocol::Http,
      ),
    };
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
    assert!(format!("{:?}", proxy_route.clone()).contains("IisProxyRoute"));
    assert_eq!(coordinator.current_state().status, ConfigApplyStatus::Idle);
    assert_eq!(
      coordinator.runtime_state().await.status,
      cadder_protocol::RuntimeStatus::Idle
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
        reload: Duration::from_millis(500),
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
    let runtime_logs = logs.query(
      crate::logs::LogQuery {
        stream: cadder_protocol::LogStreamIdentity::runtime_control(),
        limit: 20,
        after_sequence: None,
        minimum_severity: None,
      },
      true,
    );

    assert_eq!(started.status, ConfigApplyStatus::Applied);
    assert_eq!(idle.status, ConfigApplyStatus::Idle);
    assert!(runtime_logs.entries.iter().any(|entry| {
      entry.operation.as_deref() == Some("idle-stop")
        && entry.raw_message.contains("caddy stop timed out")
    }));
  }

  #[test]
  fn iis_proxy_route_helpers_track_case_insensitive_exceptions() {
    let mut coordinator = coordinator_for_test();
    coordinator.set_iis_proxy_route(
      "Default Web Site|http|*:80:app.localhost",
      "app.localhost",
      "127.0.0.1:41043",
      IisProxyBackendProtocol::Http,
    );
    coordinator.set_iis_proxy_route(
      "Default Web Site|http|*:80:api.localhost",
      "api.localhost",
      "127.0.0.1:41044",
      IisProxyBackendProtocol::Http,
    );

    assert!(coordinator.has_iis_proxy_routes_except("APP.localhost"));
    coordinator.remove_iis_proxy_route("api.localhost");
    assert!(!coordinator.has_iis_proxy_routes_except("APP.localhost"));
  }

  #[test]
  fn iis_protocol_and_route_id_helpers_are_stable() {
    assert_eq!(
      IisProxyBackendProtocol::from_iis_protocol("HTTPS"),
      IisProxyBackendProtocol::Https
    );
    assert_eq!(
      IisProxyBackendProtocol::from_iis_protocol("http"),
      IisProxyBackendProtocol::Http
    );
    assert_eq!(
      route_id_fragment("Default Web Site|HTTPS|*:443:App.Localhost"),
      "default_web_site_https___443_app_localhost"
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

  #[test]
  fn compose_config_appends_iis_proxy_routes_after_registration_routes() {
    let registrations = vec![registration("shim", &["app.localhost"])];
    let routes_by_registration = BTreeMap::from([(
      "shim".to_string(),
      vec![json!({
          "match": [{ "host": ["app.localhost"] }],
          "handle": [{ "handler": "static_response", "body": "app" }],
          "terminal": true
      })],
    )]);
    let iis_routes = BTreeMap::from([(
      "iis-app.localhost".to_string(),
      IisProxyRoute {
        domain_key: "iis-app.localhost".to_string(),
        route: json!({
            "match": [{ "host": ["iis-app.localhost"] }],
            "handle": [{ "handler": "reverse_proxy", "upstreams": [{ "dial": "127.0.0.1:41043" }] }],
            "terminal": true
        }),
      },
    )]);

    let config = compose_config(&registrations, &routes_by_registration, &iis_routes);
    let routes = config
      .pointer("/apps/http/servers/cadder_https/routes")
      .and_then(Value::as_array)
      .unwrap();
    let http_listen = config
      .pointer("/apps/http/servers/cadder_http/listen")
      .and_then(Value::as_array)
      .unwrap();
    let https_listen = config
      .pointer("/apps/http/servers/cadder_https/listen")
      .and_then(Value::as_array)
      .unwrap();
    let tls_connection_policies = config
      .pointer("/apps/http/servers/cadder_https/tls_connection_policies")
      .and_then(Value::as_array)
      .unwrap();
    let tls_subjects = config
      .pointer("/apps/tls/automation/policies/0/subjects")
      .and_then(Value::as_array)
      .unwrap();
    let tls_issuer = config
      .pointer("/apps/tls/automation/policies/0/issuers/0/module")
      .and_then(Value::as_str);

    assert_eq!(http_listen, &[json!(":80")]);
    assert_eq!(https_listen, &[json!(":443")]);
    assert_eq!(tls_connection_policies, &[json!({})]);
    assert_eq!(
      tls_subjects,
      &[json!("app.localhost"), json!("iis-app.localhost")]
    );
    assert_eq!(tls_issuer, Some("internal"));
    assert_eq!(routes.len(), 2);
    assert_eq!(
      routes[0].pointer("/match/0/host/0").and_then(Value::as_str),
      Some("app.localhost")
    );
    assert_eq!(
      routes[1].pointer("/match/0/host/0").and_then(Value::as_str),
      Some("iis-app.localhost")
    );
  }

  #[test]
  fn compose_config_includes_iis_only_routes_in_http_and_https_servers() {
    let iis_routes = BTreeMap::from([(
      "secure.localhost".to_string(),
      IisProxyRoute {
        domain_key: "secure.localhost".to_string(),
        route: iis_proxy_route(
          "Default Web Site|https|*:443:secure.localhost",
          "secure.localhost",
          "127.0.0.1:41443",
          IisProxyBackendProtocol::Https,
        ),
      },
    )]);

    let config = compose_config(&[], &BTreeMap::new(), &iis_routes);
    let http_route = config
      .pointer("/apps/http/servers/cadder_http/routes/0")
      .unwrap();
    let https_route = config
      .pointer("/apps/http/servers/cadder_https/routes/0")
      .unwrap();
    let tls_subjects = config
      .pointer("/apps/tls/automation/policies/0/subjects")
      .and_then(Value::as_array)
      .unwrap();

    assert_eq!(http_route, https_route);
    assert_eq!(
      https_route
        .pointer("/handle/0/transport/tls/server_name")
        .and_then(Value::as_str),
      Some("secure.localhost")
    );
    assert_eq!(tls_subjects, &[json!("secure.localhost")]);
  }

  #[test]
  fn iis_proxy_route_leaves_http_backend_plaintext() {
    let route = iis_proxy_route(
      "Default Web Site|http|*:80:app.localhost",
      "app.localhost",
      "127.0.0.1:41043",
      IisProxyBackendProtocol::Http,
    );

    assert_eq!(
      route
        .pointer("/handle/0/upstreams/0/dial")
        .and_then(Value::as_str),
      Some("127.0.0.1:41043")
    );
    assert!(route.pointer("/handle/0/transport").is_none());
  }

  #[test]
  fn iis_proxy_route_configures_tls_transport_for_https_backend() {
    let route = iis_proxy_route(
      "Default Web Site|https|*:443:secure.localhost",
      "secure.localhost",
      "127.0.0.1:41043",
      IisProxyBackendProtocol::Https,
    );

    assert_eq!(
      route
        .pointer("/handle/0/upstreams/0/dial")
        .and_then(Value::as_str),
      Some("127.0.0.1:41043")
    );
    assert_eq!(
      route
        .pointer("/handle/0/transport/protocol")
        .and_then(Value::as_str),
      Some("http")
    );
    assert_eq!(
      route
        .pointer("/handle/0/transport/tls/server_name")
        .and_then(Value::as_str),
      Some("secure.localhost")
    );
    assert_eq!(
      route
        .pointer("/handle/0/transport/tls/insecure_skip_verify")
        .and_then(Value::as_bool),
      Some(true)
    );
  }

  #[test]
  fn detects_iis_proxy_route_conflicts_with_active_registration_domains() {
    let registrations = vec![registration("shim", &["app.localhost"])];
    let iis_routes = BTreeMap::from([(
      "app.localhost".to_string(),
      IisProxyRoute {
        domain_key: "app.localhost".to_string(),
        route: json!({ "match": [{ "host": ["app.localhost"] }] }),
      },
    )]);

    let diagnostics = detect_iis_route_conflicts(&registrations, &iis_routes);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "iis-domain-conflict");
  }
}
