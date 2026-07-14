use anyhow::{Context, Result, anyhow};
use cadder_ipc::{
  IisBinding, IisBindingIdentity, IisHandoffState, IisIssue, IisIssueKind,
  IisRestoreMetadataSummary, canonicalize_domain,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IisBindingRecord {
  pub site_name: String,
  pub protocol: String,
  pub binding_information: String,
  pub ip_address: String,
  pub port: u16,
  pub host_header: String,
  #[serde(default)]
  pub tls_certificate: Option<IisTlsCertificate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IisTlsCertificate {
  pub thumbprint: String,
  pub store_name: String,
  #[serde(default)]
  pub ssl_flags: Option<i32>,
}

impl IisBindingRecord {
  pub fn from_binding_information(
    site_name: impl Into<String>,
    protocol: impl Into<String>,
    binding_information: impl Into<String>,
  ) -> Result<Self> {
    let site_name = site_name.into();
    let protocol = protocol.into();
    let binding_information = binding_information.into();
    let mut parts = binding_information.rsplitn(3, ':');
    let host_header = parts
      .next()
      .ok_or_else(|| anyhow!("IIS binding `{binding_information}` is missing a host segment"))?;
    let port = parts
      .next()
      .ok_or_else(|| anyhow!("IIS binding `{binding_information}` is missing a port segment"))?
      .parse::<u16>()
      .with_context(|| format!("parse IIS binding port in `{binding_information}`"))?;
    let ip_address = parts
      .next()
      .ok_or_else(|| anyhow!("IIS binding `{binding_information}` is missing an IP segment"))?;
    let ip_address = ip_address.to_string();
    let host_header = host_header.to_string();

    Ok(Self {
      site_name,
      protocol,
      binding_information,
      ip_address,
      port,
      host_header,
      tls_certificate: None,
    })
  }

  pub fn binding_id(&self) -> String {
    format!(
      "{}|{}|{}",
      self.site_name, self.protocol, self.binding_information
    )
  }

  pub fn identity(&self) -> IisBindingIdentity {
    IisBindingIdentity {
      binding_id: self.binding_id(),
      site_name: self.site_name.clone(),
      protocol: self.protocol.clone(),
      binding_information: self.binding_information.clone(),
    }
  }

  pub fn restore_summary(&self) -> IisRestoreMetadataSummary {
    IisRestoreMetadataSummary {
      site_name: self.site_name.clone(),
      protocol: self.protocol.clone(),
      ip_address: self.ip_address.clone(),
      port: self.port,
      host_header: self.host_header.clone(),
      binding_information: self.binding_information.clone(),
    }
  }

  pub fn backend_http_binding(&self, port: u16, route_host: &str) -> Self {
    self.backend_binding_with_protocol(port, route_host, "http", None)
  }

  pub fn backend_binding(&self, port: u16, route_host: &str) -> Result<Self, IisIssue> {
    if self.protocol.eq_ignore_ascii_case("http") {
      return Ok(self.backend_http_binding(port, route_host));
    }
    if self.protocol.eq_ignore_ascii_case("https") {
      let Some(tls_certificate) = self.usable_tls_certificate() else {
        return Err(IisIssue::new(
          IisIssueKind::MissingTlsCertificate,
          format!(
            "HTTPS IIS binding `{}` cannot be handed off because IIS did not expose usable TLS certificate metadata. Repair the IIS HTTPS certificate binding, refresh IIS discovery, and retry handoff.",
            self.binding_information
          ),
        ));
      };
      return Ok(self.backend_binding_with_protocol(
        port,
        route_host,
        "https",
        Some(tls_certificate.clone()),
      ));
    }
    Err(IisIssue::new(
      IisIssueKind::UnsupportedBindingShape,
      format!(
        "IIS protocol `{}` is not supported for handoff.",
        self.protocol
      ),
    ))
  }

  fn backend_binding_with_protocol(
    &self,
    port: u16,
    route_host: &str,
    protocol: &str,
    tls_certificate: Option<IisTlsCertificate>,
  ) -> Self {
    let ip_address = "127.0.0.1".to_string();
    let original_host = self.host_header.trim();
    let host_header = if original_host.is_empty() || original_host == "*" {
      canonicalize_domain(route_host)
    } else {
      self.host_header.clone()
    };
    let binding_information = format!("{ip_address}:{port}:{host_header}");
    Self {
      site_name: self.site_name.clone(),
      protocol: protocol.to_string(),
      binding_information,
      ip_address,
      port,
      host_header,
      tls_certificate,
    }
  }

  fn usable_tls_certificate(&self) -> Option<&IisTlsCertificate> {
    self
      .tls_certificate
      .as_ref()
      .filter(|certificate| !certificate.thumbprint.trim().is_empty())
      .filter(|certificate| !certificate.store_name.trim().is_empty())
  }

  #[cfg(windows)]
  fn with_tls_certificate(mut self, tls_certificate: Option<IisTlsCertificate>) -> Self {
    self.tls_certificate = tls_certificate;
    self
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IisRestoreRecord {
  pub binding: IisBindingRecord,
  pub domain_key: String,
  #[serde(default)]
  pub registration_id: Option<String>,
  #[serde(default)]
  pub backend_binding: Option<IisBindingRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IisMetadata {
  pub handoffs: BTreeMap<String, IisRestoreRecord>,
}

#[derive(Debug, Clone)]
pub struct IisMetadataStore {
  path: Option<PathBuf>,
  metadata: Arc<Mutex<IisMetadata>>,
}

impl IisMetadataStore {
  pub fn memory() -> Self {
    Self {
      path: None,
      metadata: Arc::new(Mutex::new(IisMetadata::default())),
    }
  }

  pub async fn load(path: PathBuf) -> Result<Self> {
    let metadata = if path.is_file() {
      let raw = tokio::fs::read_to_string(&path)
        .await
        .with_context(|| format!("read daemon metadata {}", path.display()))?;
      serde_json::from_str(&raw)
        .with_context(|| format!("parse daemon metadata {}", path.display()))?
    } else {
      IisMetadata::default()
    };
    Ok(Self {
      path: Some(path),
      metadata: Arc::new(Mutex::new(metadata)),
    })
  }

  pub async fn snapshot(&self) -> BTreeMap<String, IisRestoreRecord> {
    self.metadata.lock().await.handoffs.clone()
  }

  pub async fn insert(&self, binding_id: String, record: IisRestoreRecord) -> Result<()> {
    let mut metadata = self.metadata.lock().await;
    metadata.handoffs.insert(binding_id, record);
    self.save_locked(&metadata).await
  }

  pub async fn remove(&self, binding_id: &str) -> Result<Option<IisRestoreRecord>> {
    let mut metadata = self.metadata.lock().await;
    let removed = metadata.handoffs.remove(binding_id);
    self.save_locked(&metadata).await?;
    Ok(removed)
  }

  async fn save_locked(&self, metadata: &IisMetadata) -> Result<()> {
    let Some(path) = &self.path else {
      return Ok(());
    };
    if let Some(parent) = path.parent() {
      tokio::fs::create_dir_all(parent)
        .await
        .with_context(|| format!("create metadata directory {}", parent.display()))?;
    }
    let rendered = serde_json::to_string_pretty(metadata)?;
    tokio::fs::write(path, rendered)
      .await
      .with_context(|| format!("write daemon metadata {}", path.display()))
  }
}

#[derive(Debug, Clone)]
pub struct IisProvider {
  inner: IisProviderInner,
}

#[derive(Debug, Clone)]
enum IisProviderInner {
  System,
  #[cfg(test)]
  Fake(Arc<Mutex<FakeIisProviderState>>),
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub enum IisMutation {
  Add(IisBindingRecord),
  Remove(IisBindingRecord),
  Restore(IisBindingRecord),
}

#[cfg(test)]
impl IisMutation {
  pub fn add(binding: IisBindingRecord) -> Self {
    Self::Add(binding)
  }

  pub fn remove(binding: IisBindingRecord) -> Self {
    Self::Remove(binding)
  }

  pub fn restore(binding: IisBindingRecord) -> Self {
    Self::Restore(binding)
  }
}

impl Default for IisProvider {
  fn default() -> Self {
    Self::system()
  }
}

impl IisProvider {
  pub fn system() -> Self {
    Self {
      inner: IisProviderInner::System,
    }
  }

  #[cfg(test)]
  pub fn fake(bindings: Vec<IisBindingRecord>) -> Self {
    Self {
      inner: IisProviderInner::Fake(Arc::new(Mutex::new(FakeIisProviderState {
        bindings,
        fail_discovery: None,
        fail_remove: None,
        fail_restore: None,
        elevation_issue: None,
      }))),
    }
  }

  pub async fn discover(&self) -> Result<Vec<IisBindingRecord>, IisIssue> {
    match &self.inner {
      IisProviderInner::System => system_discover().await,
      #[cfg(test)]
      IisProviderInner::Fake(state) => state.lock().await.discover(),
    }
  }

  #[cfg(test)]
  pub async fn remove_binding(&self, binding: &IisBindingRecord) -> Result<(), IisIssue> {
    match &self.inner {
      IisProviderInner::System => system_remove_binding(binding).await,
      #[cfg(test)]
      IisProviderInner::Fake(state) => state.lock().await.remove_binding(binding),
    }
  }

  #[cfg(test)]
  pub async fn add_binding(&self, binding: &IisBindingRecord) -> Result<(), IisIssue> {
    match &self.inner {
      IisProviderInner::System => system_add_binding(binding).await,
      #[cfg(test)]
      IisProviderInner::Fake(state) => state.lock().await.add_binding(binding),
    }
  }

  #[cfg(test)]
  pub async fn restore_binding(&self, binding: &IisBindingRecord) -> Result<(), IisIssue> {
    match &self.inner {
      IisProviderInner::System => system_restore_binding(binding).await,
      #[cfg(test)]
      IisProviderInner::Fake(state) => state.lock().await.restore_binding(binding),
    }
  }

  #[cfg(test)]
  pub async fn execute_privileged_batch(
    &self,
    reason: &str,
    mutations: &[IisMutation],
  ) -> Result<(), IisIssue> {
    match &self.inner {
      IisProviderInner::System => system_execute_privileged_batch(reason, mutations).await,
      #[cfg(test)]
      IisProviderInner::Fake(state) => state.lock().await.execute_privileged_batch(mutations),
    }
  }

  #[cfg(test)]
  pub async fn set_fail_discovery(&self, issue: IisIssue) {
    if let IisProviderInner::Fake(state) = &self.inner {
      state.lock().await.fail_discovery = Some(issue);
    }
  }

  #[cfg(test)]
  pub async fn set_fail_remove(&self, issue: IisIssue) {
    if let IisProviderInner::Fake(state) = &self.inner {
      state.lock().await.fail_remove = Some(issue);
    }
  }

  #[cfg(test)]
  pub async fn set_fail_restore(&self, issue: IisIssue) {
    if let IisProviderInner::Fake(state) = &self.inner {
      state.lock().await.fail_restore = Some(issue);
    }
  }

  #[cfg(test)]
  pub async fn set_elevation_issue(&self, issue: IisIssue) {
    if let IisProviderInner::Fake(state) = &self.inner {
      state.lock().await.elevation_issue = Some(issue);
    }
  }
}

#[cfg(test)]
#[derive(Debug)]
struct FakeIisProviderState {
  bindings: Vec<IisBindingRecord>,
  fail_discovery: Option<IisIssue>,
  fail_remove: Option<IisIssue>,
  fail_restore: Option<IisIssue>,
  elevation_issue: Option<IisIssue>,
}

#[cfg(test)]
impl FakeIisProviderState {
  fn discover(&self) -> Result<Vec<IisBindingRecord>, IisIssue> {
    if let Some(issue) = &self.fail_discovery {
      return Err(issue.clone());
    }
    Ok(self.bindings.clone())
  }

  fn remove_binding(&mut self, binding: &IisBindingRecord) -> Result<(), IisIssue> {
    if let Some(issue) = &self.fail_remove {
      return Err(issue.clone());
    }
    let before = self.bindings.len();
    self
      .bindings
      .retain(|candidate| candidate.binding_id() != binding.binding_id());
    if self.bindings.len() == before {
      return Err(IisIssue::new(
        IisIssueKind::MissingBinding,
        "IIS binding was not found.",
      ));
    }
    Ok(())
  }

  fn restore_binding(&mut self, binding: &IisBindingRecord) -> Result<(), IisIssue> {
    if let Some(issue) = &self.fail_restore {
      return Err(issue.clone());
    }
    self.add_binding(binding)
  }

  fn add_binding(&mut self, binding: &IisBindingRecord) -> Result<(), IisIssue> {
    if !self
      .bindings
      .iter()
      .any(|candidate| candidate.binding_id() == binding.binding_id())
    {
      self.bindings.push(binding.clone());
    }
    Ok(())
  }

  fn execute_privileged_batch(&mut self, mutations: &[IisMutation]) -> Result<(), IisIssue> {
    if let Some(issue) = &self.elevation_issue {
      return Err(issue.clone());
    }
    for mutation in mutations {
      match mutation {
        IisMutation::Add(binding) => self.add_binding(binding)?,
        IisMutation::Remove(binding) => self.remove_binding(binding)?,
        IisMutation::Restore(binding) => self.restore_binding(binding)?,
      }
    }
    Ok(())
  }
}

#[cfg(windows)]
async fn system_discover() -> Result<Vec<IisBindingRecord>, IisIssue> {
  let script = r#"
$ErrorActionPreference = 'Stop'
Import-Module WebAdministration -ErrorAction Stop
Get-WebBinding | ForEach-Object {
  $certificateHash = $null
  if ($_.certificateHash) {
    if ($_.certificateHash -is [byte[]]) {
      $certificateHashParts = foreach ($byte in $_.certificateHash) { '{0:x2}' -f $byte }
      $certificateHash = $certificateHashParts -join ''
    } else {
      $certificateHash = ([string]$_.certificateHash).Trim()
    }
  }
  [PSCustomObject]@{
    siteName = ($_.ItemXPath -replace "^.*name='([^']+)'.*$", '$1')
    protocol = $_.protocol
    bindingInformation = $_.bindingInformation
    certificateHash = $certificateHash
    certificateStoreName = $_.certificateStoreName
    sslFlags = $_.sslFlags
  }
} | ConvertTo-Json -Depth 4
"#;
  let output = powershell_command()
    .arg("-NoProfile")
    .arg("-NonInteractive")
    .arg("-Command")
    .arg(script)
    .output()
    .await
    .map_err(provider_error)?;
  if !output.status.success() {
    return Err(classify_powershell_error(&output.stderr));
  }
  parse_powershell_bindings(&output.stdout)
}

#[cfg(not(windows))]
async fn system_discover() -> Result<Vec<IisBindingRecord>, IisIssue> {
  Err(IisIssue::new(
    IisIssueKind::IisUnavailable,
    "IIS handoff is only available on Windows.",
  ))
}

#[cfg(windows)]
#[cfg(test)]
async fn system_remove_binding(binding: &IisBindingRecord) -> Result<(), IisIssue> {
  run_powershell_mutation(system_remove_binding_script(binding)).await
}

#[cfg(windows)]
#[cfg(test)]
fn system_remove_binding_script(binding: &IisBindingRecord) -> String {
  format!(
    "$ErrorActionPreference = 'Stop'; Import-Module WebAdministration -ErrorAction Stop; Remove-WebBinding -Name '{}' -Protocol '{}' -BindingInformation '{}'",
    ps_escape(&binding.site_name),
    ps_escape(&binding.protocol),
    ps_escape(&binding.binding_information)
  )
}

#[cfg(all(test, not(windows)))]
async fn system_remove_binding(_binding: &IisBindingRecord) -> Result<(), IisIssue> {
  Err(IisIssue::new(
    IisIssueKind::IisUnavailable,
    "IIS handoff is only available on Windows.",
  ))
}

#[cfg(windows)]
#[cfg(test)]
async fn system_add_binding(binding: &IisBindingRecord) -> Result<(), IisIssue> {
  run_powershell_mutation(system_add_binding_script(binding)).await
}

#[cfg(windows)]
#[cfg(test)]
fn system_add_binding_script(binding: &IisBindingRecord) -> String {
  let ssl_flags = binding
    .tls_certificate
    .as_ref()
    .and_then(|certificate| certificate.ssl_flags)
    .unwrap_or(0);
  let ssl_flags_argument = if binding.protocol.eq_ignore_ascii_case("https") {
    format!(" -SslFlags {ssl_flags}")
  } else {
    String::new()
  };
  let certificate_script = binding
    .tls_certificate
    .as_ref()
    .filter(|_| binding.protocol.eq_ignore_ascii_case("https"))
    .map(|certificate| {
      format!(
        r#"
$binding = Get-WebBinding -Name '{site_name}' -Protocol '{protocol}' | Where-Object {{ $_.bindingInformation -eq '{binding_information}' }} | Select-Object -First 1
if ($null -eq $binding) {{
  throw "IIS binding '{binding_information}' was created but could not be found for certificate restore."
}}
$binding.AddSslCertificate('{thumbprint}', '{store_name}')
"#,
        site_name = ps_escape(&binding.site_name),
        protocol = ps_escape(&binding.protocol),
        binding_information = ps_escape(&binding.binding_information),
        thumbprint = ps_escape(&certificate.thumbprint),
        store_name = ps_escape(&certificate.store_name),
      )
    })
    .unwrap_or_default();
  format!(
    r#"
$ErrorActionPreference = 'Stop'
Import-Module WebAdministration -ErrorAction Stop
$created = $false
try {{
  New-WebBinding -Name '{site_name}' -Protocol '{protocol}' -IPAddress '{ip_address}' -Port {port} -HostHeader '{host_header}'{ssl_flags_argument}
  $created = $true
  {certificate_script}
  $site = Get-Website -Name '{site_name}'
  if ($site.State -ne 'Started') {{
    Start-WebSite -Name '{site_name}'
  }}
}} catch {{
  if ($created) {{
    Remove-WebBinding -Name '{site_name}' -Protocol '{protocol}' -BindingInformation '{binding_information}' -ErrorAction SilentlyContinue
  }}
  throw
}}
"#,
    site_name = ps_escape(&binding.site_name),
    protocol = ps_escape(&binding.protocol),
    ip_address = ps_escape(&binding.ip_address),
    port = binding.port,
    host_header = ps_escape(&binding.host_header),
    ssl_flags_argument = ssl_flags_argument,
    certificate_script = certificate_script,
    binding_information = ps_escape(&binding.binding_information),
  )
}

#[cfg(all(test, not(windows)))]
async fn system_add_binding(_binding: &IisBindingRecord) -> Result<(), IisIssue> {
  Err(IisIssue::new(
    IisIssueKind::IisUnavailable,
    "IIS handoff is only available on Windows.",
  ))
}

#[cfg(windows)]
#[cfg(test)]
async fn system_restore_binding(binding: &IisBindingRecord) -> Result<(), IisIssue> {
  system_add_binding(binding).await
}

#[cfg(windows)]
#[cfg(test)]
fn system_restore_binding_script(binding: &IisBindingRecord) -> String {
  system_add_binding_script(binding)
}

#[cfg(all(test, not(windows)))]
async fn system_restore_binding(_binding: &IisBindingRecord) -> Result<(), IisIssue> {
  Err(IisIssue::new(
    IisIssueKind::IisUnavailable,
    "IIS handoff is only available on Windows.",
  ))
}

#[cfg(windows)]
#[cfg(test)]
async fn system_execute_privileged_batch(
  reason: &str,
  mutations: &[IisMutation],
) -> Result<(), IisIssue> {
  if mutations.is_empty() {
    return Ok(());
  }
  let scripts = mutations
    .iter()
    .map(|mutation| match mutation {
      IisMutation::Add(binding) => system_add_binding_script(binding),
      IisMutation::Remove(binding) => system_remove_binding_script(binding),
      IisMutation::Restore(binding) => system_restore_binding_script(binding),
    })
    .collect::<Vec<_>>()
    .join("\n");
  run_elevated_powershell_batch(reason, scripts).await
}

#[cfg(all(test, not(windows)))]
async fn system_execute_privileged_batch(
  _reason: &str,
  _mutations: &[IisMutation],
) -> Result<(), IisIssue> {
  Err(IisIssue::new(
    IisIssueKind::ElevationUnsupported,
    "IIS elevation prompts are only available on Windows.",
  ))
}

#[cfg(windows)]
#[cfg(test)]
async fn run_powershell_mutation(script: String) -> Result<(), IisIssue> {
  let output = powershell_command()
    .arg("-NoProfile")
    .arg("-NonInteractive")
    .arg("-Command")
    .arg(script)
    .output()
    .await
    .map_err(provider_error)?;
  if output.status.success() {
    Ok(())
  } else {
    Err(classify_powershell_error(&output.stderr))
  }
}

#[cfg(windows)]
#[cfg(test)]
async fn run_elevated_powershell_batch(reason: &str, script: String) -> Result<(), IisIssue> {
  let stamp = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|duration| duration.as_nanos())
    .unwrap_or(0);
  let base = format!("cadder-iis-elevated-{}-{stamp}", std::process::id());
  let temp_dir = std::env::temp_dir();
  let script_path = temp_dir.join(format!("{base}.ps1"));
  let status_path = temp_dir.join(format!("{base}.status"));
  let error_path = temp_dir.join(format!("{base}.error"));
  let elevated_script = format!(
    r#"
$ErrorActionPreference = 'Stop'
try {{
  # Cadder reason: {reason}
  {script}
  Set-Content -LiteralPath '{status_path}' -Value '0'
  exit 0
}} catch {{
  Set-Content -LiteralPath '{error_path}' -Value $_.Exception.Message
  Set-Content -LiteralPath '{status_path}' -Value '1'
  exit 1
}}
"#,
    reason = reason.replace('\n', " "),
    script = script,
    status_path = ps_escape(&status_path.display().to_string()),
    error_path = ps_escape(&error_path.display().to_string()),
  );
  tokio::fs::write(&script_path, elevated_script)
    .await
    .map_err(provider_error)?;

  let command = format!(
    "$ErrorActionPreference = 'Stop'; $p = Start-Process -FilePath 'powershell' -Verb RunAs -Wait -PassThru -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File','{}'); exit $p.ExitCode",
    ps_escape(&script_path.display().to_string())
  );
  let output = powershell_command()
    .arg("-NoProfile")
    .arg("-NonInteractive")
    .arg("-Command")
    .arg(command)
    .output()
    .await
    .map_err(provider_error)?;

  let error_text = tokio::fs::read_to_string(&error_path).await.ok();
  let _ = tokio::fs::remove_file(&script_path).await;
  let _ = tokio::fs::remove_file(&status_path).await;
  let _ = tokio::fs::remove_file(&error_path).await;

  if output.status.success() {
    return Ok(());
  }

  let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
  let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
  let message = error_text
    .filter(|message| !message.trim().is_empty())
    .unwrap_or_else(|| {
      [stderr, stdout]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
    });
  let lower_message = message.to_ascii_lowercase();
  if lower_message.contains("canceled by the user")
    || lower_message.contains("cancelled by the user")
    || lower_message.contains("operation canceled")
  {
    return Err(IisIssue::new(
      IisIssueKind::ElevationDenied,
      "Administrator approval was denied.",
    ));
  }
  if message.is_empty() {
    Err(IisIssue::new(
      IisIssueKind::ProviderError,
      "Elevated IIS mutation failed without diagnostic output.",
    ))
  } else {
    Err(classify_powershell_error(message.as_bytes()))
  }
}

#[cfg(windows)]
fn powershell_command() -> tokio::process::Command {
  #[cfg(test)]
  if let Some(program) = std::env::var_os("CADDER_TEST_POWERSHELL") {
    let mut command = tokio::process::Command::new(program);
    configure_hidden_child(&mut command);
    return command;
  }

  let mut command = tokio::process::Command::new("powershell");
  configure_hidden_child(&mut command);
  command
}

#[cfg(windows)]
fn configure_hidden_child(command: &mut tokio::process::Command) {
  command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(windows)]
#[cfg(test)]
fn ps_escape(value: &str) -> String {
  value.replace('\'', "''")
}

#[cfg(windows)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPowerShellBinding {
  site_name: String,
  protocol: String,
  binding_information: String,
  #[serde(default)]
  certificate_hash: Option<String>,
  #[serde(default)]
  certificate_store_name: Option<String>,
  #[serde(default)]
  ssl_flags: Option<i32>,
}

#[cfg(windows)]
fn parse_powershell_bindings(raw: &[u8]) -> Result<Vec<IisBindingRecord>, IisIssue> {
  let text = String::from_utf8_lossy(raw).trim().to_string();
  if text.is_empty() {
    return Ok(Vec::new());
  }
  let values = if text.starts_with('[') {
    serde_json::from_str::<Vec<RawPowerShellBinding>>(&text)
  } else {
    serde_json::from_str::<RawPowerShellBinding>(&text).map(|binding| vec![binding])
  }
  .map_err(|error| {
    IisIssue::new(
      IisIssueKind::ProviderError,
      format!("Could not parse IIS binding output: {error}"),
    )
  })?;

  values
    .into_iter()
    .map(|binding| {
      let tls_certificate = binding
        .certificate_hash
        .filter(|thumbprint| !thumbprint.trim().is_empty())
        .map(|thumbprint| IisTlsCertificate {
          thumbprint,
          store_name: binding
            .certificate_store_name
            .filter(|store| !store.trim().is_empty())
            .unwrap_or_else(|| "My".to_string()),
          ssl_flags: binding.ssl_flags,
        });
      Ok(
        IisBindingRecord::from_binding_information(
          binding.site_name,
          binding.protocol,
          binding.binding_information,
        )
        .map_err(|error| IisIssue::new(IisIssueKind::UnsupportedBindingShape, error.to_string()))?
        .with_tls_certificate(tls_certificate),
      )
    })
    .collect()
}

#[cfg(windows)]
fn provider_error(error: std::io::Error) -> IisIssue {
  IisIssue::new(
    IisIssueKind::IisUnavailable,
    format!("Could not execute PowerShell WebAdministration command: {error}"),
  )
}

#[cfg(windows)]
fn classify_powershell_error(stderr: &[u8]) -> IisIssue {
  let message = String::from_utf8_lossy(stderr).trim().to_string();
  let normalized = message.to_lowercase();
  let (kind, message) = if powershell_message_needs_admin(&normalized) {
    (
      IisIssueKind::InsufficientPrivileges,
      "IIS access requires administrator approval.".to_string(),
    )
  } else if message.contains("WebAdministration") || normalized.contains("module") {
    (
      IisIssueKind::IisUnavailable,
      "IIS WebAdministration module is unavailable.".to_string(),
    )
  } else {
    (
      IisIssueKind::ProviderError,
      if message.is_empty() {
        "PowerShell IIS command failed.".to_string()
      } else {
        message
      },
    )
  };
  IisIssue::new(kind, message)
}

#[cfg(windows)]
fn powershell_message_needs_admin(normalized: &str) -> bool {
  normalized.contains("access is denied")
    || normalized.contains("unauthorizedaccess")
    || normalized.contains("administrator")
    || normalized.contains("elevated")
    || normalized.contains("requires elevation")
    || normalized.contains("insufficient privilege")
    || (normalized.contains("podwy") && normalized.contains("uprawnieni"))
}

pub fn binding_to_view(
  binding: &IisBindingRecord,
  state: IisHandoffState,
  issue: Option<IisIssue>,
  restore_metadata: Option<IisRestoreMetadataSummary>,
) -> IisBinding {
  let host = binding.host_header.trim();
  IisBinding {
    identity: binding.identity(),
    ip_address: binding.ip_address.clone(),
    port: binding.port,
    host_header: binding.host_header.clone(),
    domain_key: (!host.is_empty() && host != "*").then(|| canonicalize_domain(host)),
    handoff_state: state,
    issue,
    restore_metadata,
  }
}

pub fn unsupported_binding_issue(binding: &IisBindingRecord) -> Option<IisIssue> {
  let protocol = binding.protocol.to_ascii_lowercase();
  if protocol != "http" && protocol != "https" {
    return Some(IisIssue::new(
      IisIssueKind::UnsupportedBindingShape,
      format!(
        "IIS protocol `{}` is not supported for handoff.",
        binding.protocol
      ),
    ));
  }
  if protocol == "http" && binding.port != 80 {
    return Some(IisIssue::new(
      IisIssueKind::UnsupportedBindingShape,
      format!(
        "IIS port {} is not supported for HTTP handoff.",
        binding.port
      ),
    ));
  }
  if protocol == "https" && binding.port != 443 {
    return Some(IisIssue::new(
      IisIssueKind::UnsupportedBindingShape,
      format!(
        "IIS port {} is not supported for HTTPS handoff.",
        binding.port
      ),
    ));
  }
  None
}

#[cfg(test)]
mod tests {
  use super::*;
  #[cfg(windows)]
  use std::{env, ffi::OsString, fs, path::Path, process::Command as StdCommand};

  fn tls_certificate() -> IisTlsCertificate {
    IisTlsCertificate {
      thumbprint: "aabbcc".to_string(),
      store_name: "My".to_string(),
      ssl_flags: Some(1),
    }
  }

  fn with_tls_certificate(mut binding: IisBindingRecord) -> IisBindingRecord {
    binding.tls_certificate = Some(tls_certificate());
    binding
  }

  #[test]
  fn parses_iis_binding_information_from_right() {
    let binding =
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:80:App.Localhost")
        .unwrap();

    assert_eq!(binding.ip_address, "*");
    assert_eq!(binding.port, 80);
    assert_eq!(binding.host_header, "App.Localhost");
    assert_eq!(
      binding.binding_id(),
      "Default Web Site|http|*:80:App.Localhost"
    );
  }

  #[test]
  fn rejects_unsupported_binding_shapes() {
    let https = IisBindingRecord::from_binding_information(
      "Default Web Site",
      "https",
      "*:443:app.localhost",
    )
    .unwrap();
    let ftp =
      IisBindingRecord::from_binding_information("Default Web Site", "ftp", "*:21:app.localhost")
        .unwrap();
    let high_port = IisBindingRecord::from_binding_information(
      "Default Web Site",
      "http",
      "*:8080:app.localhost",
    )
    .unwrap();

    assert!(unsupported_binding_issue(&https).is_none());
    assert!(
      unsupported_binding_issue(&ftp)
        .unwrap()
        .message
        .contains("protocol")
    );
    assert!(
      unsupported_binding_issue(&high_port)
        .unwrap()
        .message
        .contains("port 8080")
    );
  }

  #[test]
  fn rejects_invalid_binding_information() {
    assert!(
      IisBindingRecord::from_binding_information("Default Web Site", "http", "missing-port")
        .unwrap_err()
        .to_string()
        .contains("port segment")
    );
    assert!(
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:not-a-port:host")
        .unwrap_err()
        .to_string()
        .contains("parse IIS binding port")
    );
  }

  #[test]
  fn binding_to_view_sets_domain_key_and_restore_metadata() {
    let binding =
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:80:App.Localhost")
        .unwrap();

    let view = binding_to_view(
      &binding,
      IisHandoffState::HandedOff,
      None,
      Some(binding.restore_summary()),
    );

    assert_eq!(view.domain_key.as_deref(), Some("app.localhost"));
    assert_eq!(view.identity.site_name, "Default Web Site");
    assert!(view.restore_metadata.is_some());
  }

  #[test]
  fn backend_http_binding_uses_loopback_ip_and_route_host_for_iis_listener() {
    let binding =
      IisBindingRecord::from_binding_information("Default Web Site", "https", "*:443:").unwrap();

    let backend = binding.backend_http_binding(41043, "iis-app.localhost");

    assert_eq!(backend.protocol, "http");
    assert_eq!(backend.ip_address, "127.0.0.1");
    assert_eq!(
      backend.binding_information,
      "127.0.0.1:41043:iis-app.localhost"
    );
  }

  #[test]
  fn backend_binding_preserves_https_certificate_for_iis_listener() {
    let binding = with_tls_certificate(
      IisBindingRecord::from_binding_information("Default Web Site", "https", "*:443:").unwrap(),
    );

    let backend = binding.backend_binding(41043, "iis-app.localhost").unwrap();

    assert_eq!(backend.protocol, "https");
    assert_eq!(backend.ip_address, "127.0.0.1");
    assert_eq!(
      backend.binding_information,
      "127.0.0.1:41043:iis-app.localhost"
    );
    assert_eq!(backend.tls_certificate, Some(tls_certificate()));
  }

  #[test]
  fn backend_binding_rejects_https_without_certificate_metadata() {
    let binding = IisBindingRecord::from_binding_information(
      "Default Web Site",
      "https",
      "*:443:secure.localhost",
    )
    .unwrap();

    let issue = binding
      .backend_binding(41043, "secure.localhost")
      .unwrap_err();

    assert_eq!(issue.kind, IisIssueKind::MissingTlsCertificate);
    assert!(issue.message.contains("TLS certificate metadata"));
  }

  #[test]
  fn backend_binding_rejects_unsupported_backend_protocol() {
    let binding =
      IisBindingRecord::from_binding_information("Default Web Site", "ftp", "*:21:app.localhost")
        .unwrap();

    let issue = binding.backend_binding(41021, "app.localhost").unwrap_err();

    assert_eq!(issue.kind, IisIssueKind::UnsupportedBindingShape);
    assert!(issue.message.contains("protocol `ftp`"));
  }

  #[test]
  fn backend_binding_rejects_blank_https_certificate_fields() {
    let mut binding = IisBindingRecord::from_binding_information(
      "Default Web Site",
      "https",
      "*:443:secure.localhost",
    )
    .unwrap();
    binding.tls_certificate = Some(IisTlsCertificate {
      thumbprint: " ".to_string(),
      store_name: "My".to_string(),
      ssl_flags: None,
    });
    let blank_thumbprint = binding.backend_binding(41043, "secure.localhost");
    binding.tls_certificate = Some(IisTlsCertificate {
      thumbprint: "aabbcc".to_string(),
      store_name: " ".to_string(),
      ssl_flags: None,
    });
    let blank_store = binding.backend_binding(41043, "secure.localhost");

    assert_eq!(
      blank_thumbprint.unwrap_err().kind,
      IisIssueKind::MissingTlsCertificate
    );
    assert_eq!(
      blank_store.unwrap_err().kind,
      IisIssueKind::MissingTlsCertificate
    );
  }

  #[tokio::test]
  async fn metadata_store_persists_and_removes_handoffs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("daemon.json");
    let binding =
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:80:app.localhost")
        .unwrap();
    let record = IisRestoreRecord {
      binding: binding.clone(),
      domain_key: "app.localhost".to_string(),
      registration_id: Some("shim-1".to_string()),
      backend_binding: Some(binding.backend_http_binding(41000, "app.localhost")),
    };

    let store = IisMetadataStore::load(path.clone()).await.unwrap();
    store.insert(binding.binding_id(), record).await.unwrap();
    let reloaded = IisMetadataStore::load(path).await.unwrap();

    assert_eq!(reloaded.snapshot().await.len(), 1);
    let removed = reloaded.remove(&binding.binding_id()).await.unwrap();
    assert_eq!(removed.unwrap().domain_key, "app.localhost");
    assert!(reloaded.snapshot().await.is_empty());
  }

  #[tokio::test]
  async fn metadata_store_memory_mode_accepts_noop_persistence() {
    let binding =
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:80:app.localhost")
        .unwrap();
    let record = IisRestoreRecord {
      binding: binding.clone(),
      domain_key: "app.localhost".to_string(),
      registration_id: None,
      backend_binding: None,
    };
    let store = IisMetadataStore::memory();

    store.insert(binding.binding_id(), record).await.unwrap();
    let removed = store.remove(&binding.binding_id()).await.unwrap();

    assert_eq!(removed.unwrap().domain_key, "app.localhost");
    assert!(store.snapshot().await.is_empty());
  }

  #[tokio::test]
  async fn metadata_store_reports_write_failures() {
    let dir = tempfile::tempdir().unwrap();
    let binding =
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:80:app.localhost")
        .unwrap();
    let record = IisRestoreRecord {
      binding: binding.clone(),
      domain_key: "app.localhost".to_string(),
      registration_id: None,
      backend_binding: None,
    };
    let store = IisMetadataStore::load(dir.path().to_path_buf())
      .await
      .unwrap();

    let error = store
      .insert(binding.binding_id(), record)
      .await
      .unwrap_err();

    assert!(error.to_string().contains("write daemon metadata"));
  }

  #[tokio::test]
  async fn metadata_store_persists_https_certificate_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.json");
    let mut binding = IisBindingRecord::from_binding_information(
      "Default Web Site",
      "https",
      "*:443:secure.localhost",
    )
    .unwrap();
    binding.tls_certificate = Some(tls_certificate());
    let backend_binding = binding.backend_binding(41043, "secure.localhost").unwrap();
    let record = IisRestoreRecord {
      binding: binding.clone(),
      domain_key: "secure.localhost".to_string(),
      registration_id: None,
      backend_binding: Some(backend_binding),
    };

    let store = IisMetadataStore::load(path.clone()).await.unwrap();
    store.insert(binding.binding_id(), record).await.unwrap();
    let reloaded = IisMetadataStore::load(path).await.unwrap();
    let snapshot = reloaded.snapshot().await;
    let tls = snapshot
      .get(&binding.binding_id())
      .and_then(|record| record.binding.tls_certificate.as_ref())
      .unwrap();

    assert_eq!(tls.thumbprint, "aabbcc");
    assert_eq!(tls.store_name, "My");
    assert_eq!(tls.ssl_flags, Some(1));
    assert_eq!(
      snapshot
        .get(&binding.binding_id())
        .and_then(|record| record.backend_binding.as_ref())
        .and_then(|binding| binding.tls_certificate.as_ref()),
      Some(&tls_certificate())
    );
  }

  #[tokio::test]
  async fn metadata_store_reports_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daemon.json");
    tokio::fs::write(&path, "{not-json").await.unwrap();

    let error = IisMetadataStore::load(path).await.unwrap_err();

    assert!(error.to_string().contains("parse daemon metadata"));
  }

  #[tokio::test]
  async fn fake_provider_reports_configured_mutation_failures() {
    let binding =
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:80:app.localhost")
        .unwrap();
    let provider = IisProvider::fake(vec![binding.clone()]);
    provider
      .set_fail_remove(IisIssue::new(
        IisIssueKind::InsufficientPrivileges,
        "denied",
      ))
      .await;

    let remove = provider.remove_binding(&binding).await.unwrap_err();
    provider
      .set_fail_restore(IisIssue::new(IisIssueKind::ProviderError, "restore failed"))
      .await;
    let restore = provider.restore_binding(&binding).await.unwrap_err();

    assert_eq!(remove.kind, IisIssueKind::InsufficientPrivileges);
    assert_eq!(restore.kind, IisIssueKind::ProviderError);
  }

  #[tokio::test]
  async fn fake_provider_reports_configured_discovery_failure() {
    let provider = IisProvider::fake(Vec::new());
    provider
      .set_fail_discovery(IisIssue::new(
        IisIssueKind::IisUnavailable,
        "discovery failed",
      ))
      .await;

    let error = provider.discover().await.unwrap_err();

    assert_eq!(error.kind, IisIssueKind::IisUnavailable);
    assert!(error.message.contains("discovery failed"));
  }

  #[tokio::test]
  async fn fake_provider_reports_missing_remove_binding() {
    let binding =
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:80:app.localhost")
        .unwrap();
    let provider = IisProvider::fake(Vec::new());

    let error = provider.remove_binding(&binding).await.unwrap_err();

    assert_eq!(error.kind, IisIssueKind::MissingBinding);
  }

  #[tokio::test]
  async fn fake_provider_discovers_and_restores_without_duplicates() {
    let binding =
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:80:app.localhost")
        .unwrap();
    let provider = IisProvider::fake(vec![binding.clone()]);

    assert_eq!(provider.discover().await.unwrap().len(), 1);
    provider.restore_binding(&binding).await.unwrap();
    assert_eq!(provider.discover().await.unwrap().len(), 1);
  }

  #[tokio::test]
  async fn fake_provider_privileged_batch_applies_all_mutation_variants() {
    let original =
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:80:app.localhost")
        .unwrap();
    let added =
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:80:api.localhost")
        .unwrap();
    let provider = IisProvider::fake(vec![original.clone()]);

    provider
      .execute_privileged_batch(
        "replace IIS bindings",
        &[
          IisMutation::remove(original.clone()),
          IisMutation::add(added.clone()),
          IisMutation::restore(original.clone()),
        ],
      )
      .await
      .unwrap();
    let bindings = provider.discover().await.unwrap();

    assert_eq!(bindings.len(), 2);
    assert!(
      bindings
        .iter()
        .any(|binding| binding.binding_id() == original.binding_id())
    );
    assert!(
      bindings
        .iter()
        .any(|binding| binding.binding_id() == added.binding_id())
    );
    assert!(format!("{:?}", IisProvider::default()).contains("System"));
    assert!(format!("{:?}", IisMutation::add(added.clone())).contains("Add"));

    provider
      .set_elevation_issue(IisIssue::new(IisIssueKind::ElevationDenied, "denied"))
      .await;
    let error = provider
      .execute_privileged_batch(
        "denied IIS bindings",
        &[IisMutation::remove(original.clone())],
      )
      .await
      .unwrap_err();

    assert_eq!(error.kind, IisIssueKind::ElevationDenied);
  }

  #[cfg(windows)]
  #[test]
  fn parses_powershell_binding_json_shapes() {
    let single = br#"{"siteName":"Default Web Site","protocol":"http","bindingInformation":"*:80:app.localhost"}"#;
    let array = br#"[{"siteName":"Default Web Site","protocol":"http","bindingInformation":"*:80:app.localhost"}]"#;

    assert_eq!(parse_powershell_bindings(single).unwrap().len(), 1);
    assert_eq!(parse_powershell_bindings(array).unwrap().len(), 1);
    assert!(parse_powershell_bindings(b"").unwrap().is_empty());
    assert_eq!(
      parse_powershell_bindings(b"{bad-json").unwrap_err().kind,
      IisIssueKind::ProviderError
    );
  }

  #[cfg(windows)]
  #[test]
  fn parses_powershell_binding_tls_metadata() {
    let raw = br#"{"siteName":"Default Web Site","protocol":"https","bindingInformation":"*:443:secure.localhost","certificateHash":"aabbcc","certificateStoreName":"WebHosting","sslFlags":1}"#;

    let bindings = parse_powershell_bindings(raw).unwrap();
    let tls = bindings[0].tls_certificate.as_ref().unwrap();

    assert_eq!(tls.thumbprint, "aabbcc");
    assert_eq!(tls.store_name, "WebHosting");
    assert_eq!(tls.ssl_flags, Some(1));
  }

  #[cfg(windows)]
  #[test]
  fn classifies_powershell_errors_and_escapes_strings() {
    assert_eq!(ps_escape("Bob's Site"), "Bob''s Site");
    assert_eq!(
      classify_powershell_error(b"Access is denied").kind,
      IisIssueKind::InsufficientPrivileges
    );
    assert_eq!(
      classify_powershell_error(
        b"Import-Module WebAdministration failed: podwy\xBFszonymi uprawnieniami"
      )
      .kind,
      IisIssueKind::InsufficientPrivileges
    );
    assert_eq!(
      classify_powershell_error(b"WebAdministration module missing").kind,
      IisIssueKind::IisUnavailable
    );
    assert_eq!(
      classify_powershell_error(b"unexpected").kind,
      IisIssueKind::ProviderError
    );
  }

  #[cfg(windows)]
  #[test]
  fn windows_system_iis_scripts_escape_binding_and_certificate_metadata() {
    let binding = with_tls_certificate(
      IisBindingRecord::from_binding_information("Bob's Site", "https", "*:443:secure.localhost")
        .unwrap(),
    );

    let remove = system_remove_binding_script(&binding);
    let add = system_add_binding_script(&binding);
    let restore = system_restore_binding_script(&binding);

    assert!(remove.contains("Remove-WebBinding"));
    assert!(remove.contains("Bob''s Site"));
    assert!(add.contains("New-WebBinding"));
    assert!(add.contains("-SslFlags 1"));
    assert!(add.contains("AddSslCertificate('aabbcc', 'My')"));
    assert_eq!(restore, add);
  }

  #[cfg(windows)]
  #[tokio::test]
  // The guard serializes process-wide environment overrides while awaited fake PowerShell calls read them.
  #[allow(clippy::await_holding_lock)]
  async fn windows_powershell_boundaries_use_process_results_without_real_iis() {
    let _lock = crate::TEST_ENV_LOCK.lock().unwrap();
    let _snapshot = EnvSnapshot::capture(["CADDER_TEST_POWERSHELL", "FAKE_POWERSHELL_MODE"]);
    let temp = tempfile::tempdir().unwrap();
    write_fake_powershell(temp.path());
    unsafe {
      env::set_var("CADDER_TEST_POWERSHELL", temp.path().join("powershell.exe"));
    }

    unsafe {
      env::set_var("FAKE_POWERSHELL_MODE", "discover-success");
    }
    let discovered = system_discover().await.unwrap();
    assert_eq!(discovered[0].host_header, "app.localhost");

    unsafe {
      env::set_var("FAKE_POWERSHELL_MODE", "module-failure");
    }
    assert_eq!(
      system_discover().await.unwrap_err().kind,
      IisIssueKind::IisUnavailable
    );

    unsafe {
      env::set_var("FAKE_POWERSHELL_MODE", "success");
    }
    run_powershell_mutation("$true".to_string()).await.unwrap();

    unsafe {
      env::set_var("FAKE_POWERSHELL_MODE", "denied");
    }
    assert_eq!(
      run_powershell_mutation("$true".to_string())
        .await
        .unwrap_err()
        .kind,
      IisIssueKind::InsufficientPrivileges
    );

    let binding =
      IisBindingRecord::from_binding_information("Default Web Site", "http", "*:80:app.localhost")
        .unwrap();
    unsafe {
      env::set_var("FAKE_POWERSHELL_MODE", "success");
    }
    system_execute_privileged_batch(
      "test elevated success",
      &[
        IisMutation::add(binding.clone()),
        IisMutation::remove(binding.clone()),
        IisMutation::restore(binding.clone()),
      ],
    )
    .await
    .unwrap();

    unsafe {
      env::set_var("FAKE_POWERSHELL_MODE", "cancel");
    }
    assert_eq!(
      system_execute_privileged_batch("test elevated cancel", &[IisMutation::remove(binding)])
        .await
        .unwrap_err()
        .kind,
      IisIssueKind::ElevationDenied
    );
  }

  #[cfg(windows)]
  #[tokio::test]
  async fn windows_empty_privileged_batch_short_circuits_without_powershell() {
    system_execute_privileged_batch("nothing to do", &[])
      .await
      .unwrap();
  }

  #[cfg(windows)]
  fn write_fake_powershell(dir: &Path) {
    let source_path = dir.join("fake_powershell.rs");
    let exe_path = dir.join("powershell.exe");
    fs::write(
      &source_path,
      r##"
fn main() {
  let mode = std::env::var("FAKE_POWERSHELL_MODE").unwrap_or_default();
  match mode.as_str() {
    "discover-success" => {
      println!(r#"{{"siteName":"Default Web Site","protocol":"http","bindingInformation":"*:80:app.localhost"}}"#);
      std::process::exit(0);
    }
    "module-failure" => {
      eprintln!("WebAdministration module missing");
      std::process::exit(1);
    }
    "denied" => {
      eprintln!("Access is denied");
      std::process::exit(1);
    }
    "cancel" => {
      eprintln!("Operation canceled");
      std::process::exit(1);
    }
    "success" => std::process::exit(0),
    _ => {
      eprintln!("unknown fake powershell mode {mode}");
      std::process::exit(1);
    }
  }
}
"##,
    )
    .unwrap();
    let status = StdCommand::new("rustc")
      .arg(&source_path)
      .arg("-o")
      .arg(&exe_path)
      .status()
      .unwrap();
    assert!(status.success());
  }

  #[cfg(windows)]
  struct EnvSnapshot {
    values: Vec<(&'static str, Option<OsString>)>,
  }

  #[cfg(windows)]
  impl EnvSnapshot {
    fn capture<const N: usize>(names: [&'static str; N]) -> Self {
      Self {
        values: names
          .into_iter()
          .map(|name| (name, env::var_os(name)))
          .collect(),
      }
    }
  }

  #[cfg(windows)]
  impl Drop for EnvSnapshot {
    fn drop(&mut self) {
      for (name, value) in &self.values {
        unsafe {
          if let Some(value) = value {
            env::set_var(name, value);
          } else {
            env::remove_var(name);
          }
        }
      }
    }
  }
}
