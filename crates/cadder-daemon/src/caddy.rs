use crate::{
  config::{CONFIG_FILE_NAME, CadderConfig, configured_real_caddy_command},
  logs::CaddyLogStore,
  paths::RuntimePaths,
  process_tree::ProcessTreeChild,
  runtime::{CaddyRuntime, ProcessRuntime},
};
use anyhow::{Context, Result, anyhow};
use cadder_protocol::{
  ConfigApplyStatus, ConfigDiagnostic, ConfigState, EntrypointRegistration, LogAttributionKind,
  LogSeverity, RegisteredDomain,
};
use chrono::Utc;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
  collections::{BTreeMap, BTreeSet},
  env, fmt,
  path::{Path, PathBuf},
  process::Stdio,
  str::FromStr,
  time::Duration,
};
use tokio::process::Command;

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
  configured_command: Option<String>,
  executable_path: Option<PathBuf>,
}

impl RealCaddyResolver {
  pub fn new(configured_command: Option<String>) -> Self {
    Self {
      configured_command,
      executable_path: env::current_exe().ok(),
    }
  }

  #[cfg(test)]
  fn with_executable_path(
    configured_command: Option<String>,
    executable_path: Option<PathBuf>,
  ) -> Self {
    Self {
      configured_command,
      executable_path,
    }
  }

  pub fn resolve(&self) -> Result<PathBuf> {
    let cwd = env::current_dir().context("resolve current directory for Cadder configuration")?;
    self.resolve_for_working_directory(&cwd)
  }

  pub fn resolve_for_working_directory(&self, cwd: &Path) -> Result<PathBuf> {
    let selected = self.selected_command(cwd)?;
    let excluded = shim_exclusions();
    if let Some(selected) = selected {
      return resolve_command(&selected.command, &excluded).with_context(|| {
        format!(
          "resolve real Caddy command `{}` from {}",
          selected.command, selected.source
        )
      });
    }

    resolve_command("caddy", &excluded).context(
      "could not resolve a safe real Caddy binary. Configure it with a CLI override, \
       [caddy].real_command in cadder.toml, CADDER_CADDY_REAL_COMMAND, or make a real \
       caddy executable available on PATH",
    )
  }

  pub fn resolution_help(error: &anyhow::Error) -> String {
    format!(
      "Cadder could not resolve a safe real Caddy binary.\n\n\
       Cause: {error}\n\n\
       Configure the real Caddy command with one of these options, in precedence order:\n\
       - CLI override: --real-caddy-command for cadderd/cadder or --cadder-real-caddy-command for the shim\n\
       - [caddy].real_command in cadder.toml in the project working directory\n\
       - [caddy].real_command in cadder.toml next to the Cadder executable\n\
       - CADDER_CADDY_REAL_COMMAND environment variable\n\
       - A real caddy executable on PATH that is not Cadder's shim"
    )
  }

  fn selected_command(&self, cwd: &Path) -> Result<Option<SelectedCaddyCommand>> {
    if let Some(command) = trimmed(self.configured_command.as_deref()) {
      return Ok(Some(SelectedCaddyCommand::new(
        command,
        "CLI override".to_string(),
      )));
    }

    let cwd_config = cwd.join(CONFIG_FILE_NAME);
    if let Some(command) = command_from_config_file(&cwd_config)? {
      return Ok(Some(SelectedCaddyCommand::new(
        command,
        format!("{} in the current working directory", cwd_config.display()),
      )));
    }

    if let Some(executable_config) = self.executable_config_path()
      && executable_config != cwd_config
      && let Some(command) = command_from_config_file(&executable_config)?
    {
      return Ok(Some(SelectedCaddyCommand::new(
        command,
        format!("{} next to the executable", executable_config.display()),
      )));
    }

    let environment_config = CadderConfig::from_environment()?;
    if let Some(command) = configured_real_caddy_command(&environment_config) {
      return Ok(Some(SelectedCaddyCommand::new(
        command,
        "environment variables".to_string(),
      )));
    }

    Ok(None)
  }

  fn executable_config_path(&self) -> Option<PathBuf> {
    self
      .executable_path
      .as_ref()
      .and_then(|path| path.parent())
      .map(|dir| dir.join(CONFIG_FILE_NAME))
  }
}

#[derive(Debug, Clone)]
struct SelectedCaddyCommand {
  command: String,
  source: String,
}

impl SelectedCaddyCommand {
  fn new(command: String, source: String) -> Self {
    Self { command, source }
  }
}

fn command_from_config_file(path: &Path) -> Result<Option<String>> {
  if !path.is_file() {
    return Ok(None);
  }
  let config = CadderConfig::from_file(path)?;
  Ok(configured_real_caddy_command(&config).map(|command| {
    path
      .parent()
      .map(|base| anchor_configured_command(&command, base))
      .unwrap_or(command)
  }))
}

fn anchor_configured_command(command: &str, base: &Path) -> String {
  let path = Path::new(command);
  if path.is_absolute() || path.components().count() <= 1 {
    command.to_string()
  } else {
    base.join(path).display().to_string()
  }
}

fn trimmed(value: Option<&str>) -> Option<String> {
  value
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(ToOwned::to_owned)
}

fn shim_exclusions() -> BTreeSet<PathBuf> {
  let mut excluded = BTreeSet::new();
  if let Ok(path) = env::current_exe().and_then(|path| path.canonicalize()) {
    excluded.insert(path);
  }
  if let Some(path) = env::var_os("CADDER_CADDY_SHIM_PATH")
    && let Ok(path) = PathBuf::from(path).canonicalize()
  {
    excluded.insert(path);
  }
  excluded
}

fn resolve_command(command: &str, excluded: &BTreeSet<PathBuf>) -> Result<PathBuf> {
  let path = PathBuf::from(command);
  if path.components().count() > 1 || path.is_absolute() {
    let canonical = path
      .canonicalize()
      .with_context(|| format!("canonicalize {}", path.display()))?;
    if excluded.contains(&canonical) {
      return Err(anyhow!("resolved command points at the Cadder shim"));
    }
    return Ok(canonical);
  }

  let path_var = env::var_os("PATH").ok_or_else(|| anyhow!("PATH is not set"))?;
  resolve_command_with_path(command, excluded, &path_var)
}

fn resolve_command_with_path(
  command: &str,
  excluded: &BTreeSet<PathBuf>,
  path_var: &std::ffi::OsStr,
) -> Result<PathBuf> {
  for dir in env::split_paths(path_var) {
    for candidate in executable_candidates(&dir, command) {
      if candidate.is_file() {
        let canonical = candidate.canonicalize().unwrap_or(candidate);
        if !excluded.contains(&canonical) {
          return Ok(canonical);
        }
      }
    }
  }
  Err(anyhow!("command `{command}` not found on PATH"))
}

fn executable_candidates(dir: &Path, command: &str) -> Vec<PathBuf> {
  #[cfg(windows)]
  {
    let pathext = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let mut candidates = vec![dir.join(command)];
    for ext in pathext.split(';').filter(|ext| !ext.is_empty()) {
      candidates.push(dir.join(format!("{command}{ext}")));
    }
    candidates
  }

  #[cfg(not(windows))]
  {
    vec![dir.join(command)]
  }
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
    let working_directory = registration
      .source_working_directory
      .canonical
      .as_deref()
      .unwrap_or(&registration.source_working_directory.raw);
    let binary = self
      .resolver
      .resolve_for_working_directory(Path::new(working_directory))?;
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

    let mut command = Command::new(binary);
    command
      .arg("adapt")
      .arg("--config")
      .arg(config_path)
      .arg("--adapter")
      .arg(adapter)
      .stdout(Stdio::piped())
      .stderr(Stdio::piped());
    let child = ProcessTreeChild::spawn(command).context("start caddy adapt")?;
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

  const ENV_KEYS: [&str; 2] = ["CADDER_CADDY__REAL_COMMAND", "CADDER_CADDY_REAL_COMMAND"];

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
  }

  fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
  }

  #[test]
  fn resolve_command_reports_missing_path_variable() {
    let _guard = lock_env();
    let _env = EnvSnapshot::capture(&["PATH"]);
    unsafe {
      env::remove_var("PATH");
    }

    let error = resolve_command("caddy", &BTreeSet::new()).unwrap_err();

    assert!(error.to_string().contains("PATH is not set"));
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
  fn resolver_prefers_cli_override_over_working_directory_config() {
    let dir = tempfile::tempdir().unwrap();
    let cli_caddy = dir.path().join("cli-caddy");
    let cwd_caddy = dir.path().join("cwd-caddy");
    write_file(&cli_caddy);
    write_file(&cwd_caddy);
    fs::write(
      dir.path().join(CONFIG_FILE_NAME),
      format!(
        "[caddy]\nreal_command = \"{}\"\n",
        cwd_caddy.display().to_string().replace('\\', "\\\\")
      ),
    )
    .unwrap();

    let resolver = RealCaddyResolver::with_executable_path(
      Some(cli_caddy.display().to_string()),
      Some(dir.path().join("cadderd")),
    );

    assert_eq!(
      resolver.resolve_for_working_directory(dir.path()).unwrap(),
      canonical(&cli_caddy)
    );
  }

  #[test]
  fn resolver_prefers_working_directory_config_over_executable_config() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("project");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let cwd_caddy = cwd.join("cwd-caddy");
    let exe_caddy = bin.join("exe-caddy");
    write_file(&cwd_caddy);
    write_file(&exe_caddy);
    fs::write(
      cwd.join(CONFIG_FILE_NAME),
      "[caddy]\nreal_command = \"./cwd-caddy\"\n",
    )
    .unwrap();
    fs::write(
      bin.join(CONFIG_FILE_NAME),
      "[caddy]\nreal_command = \"./exe-caddy\"\n",
    )
    .unwrap();

    let resolver =
      RealCaddyResolver::with_executable_path(None, Some(bin.join(exe_name_for_test("cadderd"))));

    assert_eq!(
      resolver.resolve_for_working_directory(&cwd).unwrap(),
      canonical(&cwd_caddy)
    );
  }

  #[test]
  fn resolver_uses_executable_config_when_working_directory_config_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("project");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let exe_caddy = bin.join("exe-caddy");
    write_file(&exe_caddy);
    fs::write(
      bin.join(CONFIG_FILE_NAME),
      "[caddy]\nreal_command = \"./exe-caddy\"\n",
    )
    .unwrap();

    let resolver =
      RealCaddyResolver::with_executable_path(None, Some(bin.join(exe_name_for_test("cadderd"))));

    assert_eq!(
      resolver.resolve_for_working_directory(&cwd).unwrap(),
      canonical(&exe_caddy)
    );
  }

  #[test]
  fn resolver_uses_environment_real_command_when_no_file_config_exists() {
    let _guard = lock_env();
    let _snapshot = EnvSnapshot::capture(&ENV_KEYS);
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("project");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let env_caddy = bin.join("env-caddy");
    write_file(&env_caddy);
    unsafe {
      env::remove_var("CADDER_CADDY__REAL_COMMAND");
      env::set_var("CADDER_CADDY_REAL_COMMAND", env_caddy.display().to_string());
    }
    let resolver = RealCaddyResolver::with_executable_path(
      None,
      Some(dir.path().join(exe_name_for_test("cadderd"))),
    );

    assert_eq!(
      resolver.resolve_for_working_directory(&cwd).unwrap(),
      canonical(&env_caddy)
    );
  }

  #[tokio::test]
  async fn adapter_resolves_real_caddy_from_registration_working_directory_config() {
    let dir = tempfile::tempdir().unwrap();
    let daemon_cwd = dir.path().join("daemon");
    let project_cwd = dir.path().join("project");
    fs::create_dir_all(&daemon_cwd).unwrap();
    fs::create_dir_all(&project_cwd).unwrap();
    let fake_caddy = project_cwd.join(fake_caddy_name_for_test());
    write_fake_caddy(&fake_caddy);
    fs::write(
      project_cwd.join(CONFIG_FILE_NAME),
      format!(
        "[caddy]\nreal_command = \"./{}\"\n",
        fake_caddy_name_for_test()
      ),
    )
    .unwrap();
    let config_path = project_cwd.join("Caddyfile");
    fs::write(&config_path, "project.localhost { respond ok }").unwrap();
    let mut registration = registration("project", &[]);
    registration.source_working_directory = SourcePath::new(
      project_cwd.display().to_string(),
      Some(project_cwd.display().to_string()),
    );
    registration.source_config_path = SourcePath::new(
      config_path.display().to_string(),
      Some(config_path.display().to_string()),
    );

    let adapter = CaddyConfigAdapter::new(RealCaddyResolver::with_executable_path(
      None,
      Some(daemon_cwd.join(exe_name_for_test("cadderd"))),
    ));
    let prepared = adapter.prepare(registration).await;

    assert!(
      prepared.diagnostics.is_empty(),
      "{:?}",
      prepared.diagnostics
    );
    assert_eq!(prepared.registration.registered_domains.len(), 1);
    assert_eq!(
      prepared.registration.registered_domains[0].name.canonical,
      "project.localhost"
    );
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

    let adapter = CaddyConfigAdapter::new(RealCaddyResolver::new(Some(
      fake_caddy.display().to_string(),
    )));
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
  async fn adapter_reports_missing_real_caddy_command() {
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
        .contains("resolve real Caddy command")
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

    write_fake_caddy_with_adapt(&failing_caddy, "not-json", 0);
    let invalid = adapter.prepare(registration).await;
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
  fn resolver_resolution_help_and_path_edges_are_stable() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing-caddy");
    let error = resolve_command(&missing.display().to_string(), &BTreeSet::new()).unwrap_err();
    let help = RealCaddyResolver::resolution_help(&error);

    assert!(help.contains("Cadder could not resolve"));
    assert_eq!(anchor_configured_command("caddy", dir.path()), "caddy");
    assert!(anchor_configured_command("./bin/caddy", dir.path()).contains("bin"));
    assert_eq!(trimmed(Some("  caddy  ")).as_deref(), Some("caddy"));
    assert_eq!(trimmed(Some("   ")), None);
  }

  #[test]
  fn resolver_reports_no_selected_command_when_no_source_is_configured() {
    let _guard = lock_env();
    let _snapshot = EnvSnapshot::capture(&ENV_KEYS);
    unsafe {
      env::remove_var("CADDER_CADDY__REAL_COMMAND");
      env::remove_var("CADDER_CADDY_REAL_COMMAND");
    }
    let dir = tempfile::tempdir().unwrap();
    let resolver = RealCaddyResolver::with_executable_path(None, None);

    let selected = resolver.selected_command(dir.path()).unwrap();
    let explicit = SelectedCaddyCommand::new("caddy".to_string(), "test".to_string());

    assert!(selected.is_none());
    assert_eq!(resolver.executable_config_path(), None);
    assert_eq!(explicit.command, "caddy");
    assert_eq!(explicit.source, "test");
    assert!(format!("{explicit:?}").contains("SelectedCaddyCommand"));
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
  fn resolve_command_rejects_configured_shim_path() {
    let dir = tempfile::tempdir().unwrap();
    let shim = dir.path().join(exe_name_for_test("caddy"));
    write_file(&shim);
    let excluded = BTreeSet::from([canonical(&shim)]);

    let error = resolve_command(&shim.display().to_string(), &excluded).unwrap_err();

    assert!(error.to_string().contains("Cadder shim"));
  }

  #[test]
  fn path_fallback_uses_caddy_without_implicit_caddy_real_default() {
    let dir = tempfile::tempdir().unwrap();
    let caddy_real = dir.path().join(exe_name_for_test("caddy-real"));
    write_file(&caddy_real);
    let excluded = BTreeSet::new();

    let error = resolve_command_with_path("caddy", &excluded, dir.path().as_os_str()).unwrap_err();

    assert!(error.to_string().contains("command `caddy` not found"));
  }

  #[test]
  fn path_resolution_skips_excluded_shim_and_uses_next_caddy_candidate() {
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

    let resolved =
      resolve_command_with_path("caddy", &BTreeSet::from([canonical(&shim)]), &path_var).unwrap();

    assert_eq!(resolved, canonical(&real));
  }

  #[test]
  fn resolver_honors_caddy_shim_path_exclusion_when_searching_path() {
    let _guard = lock_env();
    let _snapshot = EnvSnapshot::capture(&[
      "PATH",
      "CADDER_CADDY_SHIM_PATH",
      "CADDER_CADDY_REAL_COMMAND",
      "CADDER_CADDY__REAL_COMMAND",
    ]);
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
      env::remove_var("CADDER_CADDY_REAL_COMMAND");
      env::remove_var("CADDER_CADDY__REAL_COMMAND");
      env::set_var("CADDER_CADDY_SHIM_PATH", shim.display().to_string());
      env::set_var("PATH", path_var);
    }
    let resolver = RealCaddyResolver::with_executable_path(
      None,
      Some(dir.path().join(exe_name_for_test("cadderd"))),
    );

    let resolved = resolver.resolve_for_working_directory(dir.path()).unwrap();

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
    let resolver = RealCaddyResolver::new(None);
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
