use super::*;

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
    let config_path = registration
      .source_config_path
      .canonical
      .as_deref()
      .unwrap_or(&registration.source_config_path.raw);
    let parsed_routes = match tokio::fs::read_to_string(config_path).await {
      Ok(config) => match mock_caddyfile_routes(&config) {
        Ok(routes) => Some(routes),
        Err(error) => {
          return PreparedRegistration {
            registration,
            routes: Vec::new(),
            diagnostics: vec![ConfigDiagnostic {
              code: "adapt-failed".to_string(),
              message: format!("parse Caddyfile for mock backend: {error}"),
              domain_key: None,
              source_config_paths: Vec::new(),
            }],
          };
        }
      },
      Err(_) => None,
    };
    let routes = parsed_routes
      .filter(|routes| !routes.is_empty())
      .unwrap_or_else(|| {
        registration
          .registered_domains
          .iter()
          .map(|domain| mock_route_for_domain(&domain.name.canonical))
          .collect()
      });

    let parsed_domains = extract_registered_domains_from_routes(&routes);
    if registration.registered_domains.is_empty() {
      registration.registered_domains = parsed_domains;
    } else {
      let upstreams = parsed_domains
        .into_iter()
        .map(|domain| (domain.name.canonical, domain.upstream))
        .collect::<BTreeMap<_, _>>();
      for domain in &mut registration.registered_domains {
        domain.upstream = upstreams.get(&domain.name.canonical).cloned().flatten();
      }
    }

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
  pub(super) routes: BTreeMap<String, Vec<Value>>,
  pub(super) registration_diagnostics: BTreeMap<String, Vec<ConfigDiagnostic>>,
  pub(super) current: ConfigState,
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

  pub async fn runtime_state(&self) -> cadder_ipc::RuntimeState {
    self.runtime.inspect().await
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
    if active
      .iter()
      .all(|registration| active_domains(registration).is_empty())
    {
      return CaddyApplyAction::Stop { attempted };
    }

    let config = compose_config(&active, &self.routes);
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
          logs
            .append(
              cadder_ipc::LogStreamIdentity::runtime_control(),
              LogSeverity::Error,
              error.to_string(),
              LogAttributionKind::RuntimeControl,
              Some("idle-stop".to_string()),
            )
            .await;
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

pub(super) fn namespace_config_ids(value: &mut Value, namespace: &str) {
  match value {
    Value::Array(values) => {
      for value in values {
        namespace_config_ids(value, namespace);
      }
    }
    Value::Object(object) => {
      if let Some(Value::String(id)) = object.get_mut("@id") {
        id.insert_str(0, namespace);
      }
      for value in object.values_mut() {
        namespace_config_ids(value, namespace);
      }
    }
    _ => {}
  }
}

pub(super) fn compose_config(
  registrations: &[EntrypointRegistration],
  routes_by_registration: &BTreeMap<String, Vec<Value>>,
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
  let tls_subjects = tls_subjects.into_iter().collect::<Vec<_>>();
  let mut http_routes = routes.clone();
  for route in &mut http_routes {
    namespace_config_ids(route, "http_");
  }

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

pub(super) fn mock_route_for_domain(domain: &str) -> Value {
  mock_route_for_domain_and_upstream(domain, None)
}

fn mock_route_for_domain_and_upstream(domain: &str, upstream: Option<&str>) -> Value {
  let handler = upstream.map_or_else(
    || json!({ "handler": "static_response", "body": "Cadder mock Caddy route" }),
    |dial| json!({ "handler": "reverse_proxy", "upstreams": [{ "dial": dial }] }),
  );
  json!({
      "match": [{ "host": [domain] }],
      "handle": [handler],
      "terminal": true
  })
}

fn mock_caddyfile_routes(config: &str) -> std::result::Result<Vec<Value>, caddyfile_rs::Error> {
  let caddyfile = caddyfile_rs::parse_str(config)?;
  Ok(
    caddyfile
      .sites
      .iter()
      .flat_map(|site| {
        let upstream = first_mock_reverse_proxy(&site.directives);
        site
          .addresses
          .iter()
          .filter(|address| !address.host.is_empty())
          .map(move |address| {
            mock_route_for_domain_and_upstream(
              &cadder_ipc::canonicalize_domain(&address.host),
              upstream,
            )
          })
      })
      .collect(),
  )
}

fn first_mock_reverse_proxy(directives: &[caddyfile_rs::Directive]) -> Option<&str> {
  for directive in directives {
    if directive.name == "reverse_proxy"
      && let Some(upstream) = directive.arguments.first()
    {
      return Some(upstream.value());
    }
    if let Some(upstream) = directive
      .block
      .as_deref()
      .and_then(first_mock_reverse_proxy)
    {
      return Some(upstream);
    }
  }
  None
}

pub(super) fn active_domains(registration: &EntrypointRegistration) -> BTreeSet<String> {
  registration
    .registered_domains
    .iter()
    .filter(|domain| domain.activation_state.is_enabled())
    .map(|domain| domain.name.canonical.clone())
    .collect()
}

pub(super) fn filter_route_hosts(
  mut route: Value,
  enabled_hosts: &BTreeSet<String>,
) -> Option<Value> {
  let mut retained_any = false;
  filter_hosts_recursive(&mut route, enabled_hosts, &mut retained_any);
  retained_any.then_some(route)
}

pub(super) fn filter_hosts_recursive(
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
            .map(cadder_ipc::canonicalize_domain)
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

pub(super) fn extract_hosts(value: &Value) -> BTreeSet<String> {
  let mut hosts = BTreeSet::new();
  collect_hosts(value, &mut hosts);
  hosts
}

pub(super) fn extract_registered_domains(value: &Value) -> Vec<RegisteredDomain> {
  extract_registered_domains_from_routes(&extract_http_routes(value))
}

fn extract_registered_domains_from_routes(routes: &[Value]) -> Vec<RegisteredDomain> {
  let mut domains = BTreeMap::new();
  for route in routes {
    let upstream = first_reverse_proxy_dial(route);
    for host in extract_hosts(route) {
      let mut domain = RegisteredDomain::active(&host);
      domain.upstream = upstream.clone();
      domains.entry(host).or_insert(domain);
    }
  }
  domains.into_values().collect()
}

pub(super) fn first_reverse_proxy_dial(value: &Value) -> Option<String> {
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

pub(super) fn collect_hosts(value: &Value, hosts: &mut BTreeSet<String>) {
  match value {
    Value::Object(map) => {
      if let Some(Value::Array(values)) = map.get("host") {
        for value in values {
          if let Some(host) = value.as_str() {
            hosts.insert(cadder_ipc::canonicalize_domain(host));
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

pub(super) fn extract_http_routes(value: &Value) -> Vec<Value> {
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

pub(super) fn detect_conflicts(registrations: &[EntrypointRegistration]) -> Vec<ConfigDiagnostic> {
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
