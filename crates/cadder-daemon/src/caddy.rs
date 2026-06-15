use crate::{
  config::{CONFIG_FILE_NAME, CadderConfig, configured_real_caddy_command},
  logs::CaddyLogStore,
  runtime::ProcessRuntime,
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
  env,
  path::{Path, PathBuf},
  process::Stdio,
};
use tokio::process::Command;

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
       - CLI override: --real-caddy-command for cadderd/cadder-tui or --cadder-real-caddy-command for the shim\n\
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
}

#[derive(Debug, Clone)]
pub struct PreparedRegistration {
  pub registration: EntrypointRegistration,
  pub routes: Vec<Value>,
  pub diagnostics: Vec<ConfigDiagnostic>,
}

impl CaddyConfigAdapter {
  pub fn new(resolver: RealCaddyResolver) -> Self {
    Self { resolver }
  }

  pub async fn prepare(&self, registration: EntrypointRegistration) -> PreparedRegistration {
    match self.adapt(&registration).await {
      Ok(adapted) => {
        let hosts = extract_hosts(&adapted);
        let mut prepared = registration;
        if !hosts.is_empty() {
          prepared.registered_domains = hosts.into_iter().map(RegisteredDomain::active).collect();
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

    let output = Command::new(binary)
      .arg("adapt")
      .arg("--config")
      .arg(config_path)
      .arg("--adapter")
      .arg(adapter)
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .output()
      .await
      .context("run caddy adapt")?;

    if !output.status.success() {
      return Err(anyhow!(
        "caddy adapt failed: {}",
        String::from_utf8_lossy(&output.stderr)
      ));
    }

    serde_json::from_slice(&output.stdout).context("parse adapted Caddy JSON")
  }
}

#[derive(Debug)]
pub struct CaddyConfigCoordinator {
  adapter: CaddyConfigAdapter,
  runtime: ProcessRuntime,
  routes: BTreeMap<String, Vec<Value>>,
  iis_routes: BTreeMap<String, IisProxyRoute>,
  registration_diagnostics: BTreeMap<String, Vec<ConfigDiagnostic>>,
  current: ConfigState,
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

  pub async fn apply(
    &mut self,
    registrations: &[EntrypointRegistration],
    logs: &CaddyLogStore,
  ) -> ConfigState {
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
      return self.current.clone();
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
      if let Err(error) = self.runtime.stop().await {
        logs.append(
          cadder_protocol::LogStreamIdentity::runtime_control(),
          LogSeverity::Error,
          error.to_string(),
          LogAttributionKind::RuntimeControl,
          Some("idle-stop".to_string()),
        );
      }
      self.current = ConfigState {
        status: ConfigApplyStatus::Idle,
        last_attempted_at_utc: Some(attempted),
        last_successful_reload_at_utc: self.current.last_successful_reload_at_utc,
        effective_config_hash: None,
        diagnostics: Vec::new(),
      };
      return self.current.clone();
    }

    let config = compose_config(&active, &self.routes, &self.iis_routes);
    let rendered = serde_json::to_vec_pretty(&config).expect("config serialization");
    let hash = hex::encode(Sha256::digest(&rendered));

    match self.runtime.apply_config(&rendered, logs).await {
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
            source_config_paths: active
              .iter()
              .map(|registration| registration.source_config_path.raw.clone())
              .collect(),
          }],
        };
      }
    }
    self.current.clone()
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
  use cadder_protocol::{
    ActivationState, EntrypointInstanceIdentity, LogStreamIdentity, OwnerProcessIdentity,
    SourcePath,
  };
  use chrono::Utc;
  use std::fs;

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
  fn detects_active_domain_conflicts() {
    let left = registration("left", &["app.localhost"]);
    let right = registration("right", &["APP.localhost."]);

    let diagnostics = detect_conflicts(&[left, right]);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].domain_key.as_deref(), Some("app.localhost"));
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
