use cadder_protocol::{
  EntrypointRegistration, GuiStateSnapshot, LogEntry, LogSeverity, LogStreamIdentity,
  LogStreamStatus, RegisteredDomain,
};
#[cfg(windows)]
use cadder_protocol::{IisBinding, QueryIisBindingsResponse};
#[cfg(windows)]
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
  Overview,
  Entrypoints,
  Domains,
  #[cfg(windows)]
  IisHandoff,
  Logs,
  Settings,
  Diagnostics,
}

impl View {
  #[cfg(windows)]
  pub const ALL: [Self; 7] = [
    Self::Overview,
    Self::Entrypoints,
    Self::Domains,
    Self::IisHandoff,
    Self::Logs,
    Self::Settings,
    Self::Diagnostics,
  ];

  #[cfg(not(windows))]
  pub const ALL: [Self; 6] = [
    Self::Overview,
    Self::Entrypoints,
    Self::Domains,
    Self::Logs,
    Self::Settings,
    Self::Diagnostics,
  ];

  pub fn title(self) -> &'static str {
    match self {
      Self::Overview => "Overview",
      Self::Entrypoints => "Entrypoints",
      Self::Domains => "Domains",
      #[cfg(windows)]
      Self::IisHandoff => "IIS Handoff",
      Self::Logs => "Logs",
      Self::Settings => "Settings",
      Self::Diagnostics => "Diagnostics",
    }
  }

  pub fn index(self) -> usize {
    Self::ALL.iter().position(|view| *view == self).unwrap_or(0)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeverityFilter {
  All,
  Info,
  Warn,
  Error,
}

impl SeverityFilter {
  pub const ALL: [Self; 4] = [Self::All, Self::Info, Self::Warn, Self::Error];

  pub fn label(self) -> &'static str {
    match self {
      Self::All => "All",
      Self::Info => "Info and higher",
      Self::Warn => "Warnings and errors",
      Self::Error => "Errors only",
    }
  }

  pub fn description(self) -> &'static str {
    match self {
      Self::All => "Show every retained log entry",
      Self::Info => "Hide trace and debug noise",
      Self::Warn => "Show warnings and errors",
      Self::Error => "Show errors and fatal entries",
    }
  }

  pub fn minimum_severity(self) -> Option<LogSeverity> {
    match self {
      Self::All => None,
      Self::Info => Some(LogSeverity::Info),
      Self::Warn => Some(LogSeverity::Warn),
      Self::Error => Some(LogSeverity::Error),
    }
  }

  pub fn from_minimum_severity(minimum_severity: Option<LogSeverity>) -> Self {
    match minimum_severity {
      None => Self::All,
      Some(LogSeverity::Info) => Self::Info,
      Some(LogSeverity::Warn) => Self::Warn,
      Some(LogSeverity::Error | LogSeverity::Fatal) => Self::Error,
      Some(LogSeverity::Unknown | LogSeverity::Trace | LogSeverity::Debug) => Self::All,
    }
  }

  pub fn index(self) -> usize {
    Self::ALL
      .iter()
      .position(|filter| *filter == self)
      .unwrap_or(0)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonUnavailableKind {
  NotRunning,
  ConnectionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonConnectionState {
  Connected,
  NotRunning { message: String },
  ConnectionFailed { message: String },
  Starting { message: String },
  StartFailed { message: String },
}

impl DaemonConnectionState {
  pub fn label(&self) -> &'static str {
    match self {
      Self::Connected => "Connected",
      Self::NotRunning { .. } => "Not running",
      Self::ConnectionFailed { .. } => "Connection failed",
      Self::Starting { .. } => "Starting",
      Self::StartFailed { .. } => "Start failed",
    }
  }

  pub fn detail(&self) -> &str {
    match self {
      Self::Connected => "Daemon state is live.",
      Self::NotRunning { message }
      | Self::ConnectionFailed { message }
      | Self::Starting { message }
      | Self::StartFailed { message } => message,
    }
  }

  pub fn guidance(&self) -> &'static str {
    match self {
      Self::Connected => {
        "Use r to refresh or reattach the dashboard, or d to request backend shutdown."
      }
      Self::Starting { .. } => "Starting cadderd; controls stay responsive.",
      Self::NotRunning { .. } => "Press s to start cadderd, r to retry attach, or q to quit.",
      Self::ConnectionFailed { .. } => {
        "Press r to retry attach, s to start cadderd if needed, or q to quit."
      }
      Self::StartFailed { .. } => {
        "Fix the backend start error, then press s to retry or r to attach if cadderd is already running."
      }
    }
  }

  pub fn is_connected(&self) -> bool {
    matches!(self, Self::Connected)
  }

  pub fn is_starting(&self) -> bool {
    matches!(self, Self::Starting { .. })
  }

  pub fn can_start(&self) -> bool {
    !self.is_starting()
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonViewModel {
  pub state: DaemonConnectionState,
}

impl Default for DaemonViewModel {
  fn default() -> Self {
    Self {
      state: DaemonConnectionState::NotRunning {
        message: "Daemon connection has not been checked yet.".to_string(),
      },
    }
  }
}

impl DaemonViewModel {
  pub fn mark_connected(&mut self) {
    self.state = DaemonConnectionState::Connected;
  }

  pub fn mark_unavailable(&mut self, kind: DaemonUnavailableKind, message: impl Into<String>) {
    let message = message.into();
    self.state = match kind {
      DaemonUnavailableKind::NotRunning => DaemonConnectionState::NotRunning { message },
      DaemonUnavailableKind::ConnectionFailed => {
        DaemonConnectionState::ConnectionFailed { message }
      }
    };
  }

  pub fn mark_starting(&mut self, message: impl Into<String>) {
    self.state = DaemonConnectionState::Starting {
      message: message.into(),
    };
  }

  pub fn mark_start_failed(&mut self, message: impl Into<String>) {
    self.state = DaemonConnectionState::StartFailed {
      message: message.into(),
    };
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsViewModel {
  pub selected_severity: usize,
}

impl Default for SettingsViewModel {
  fn default() -> Self {
    Self {
      selected_severity: SeverityFilter::All.index(),
    }
  }
}

impl SettingsViewModel {
  pub fn selected_filter(&self) -> SeverityFilter {
    SeverityFilter::ALL
      .get(self.selected_severity)
      .copied()
      .unwrap_or(SeverityFilter::All)
  }

  pub fn select_filter(&mut self, filter: SeverityFilter) {
    self.selected_severity = filter.index();
  }

  pub fn move_severity_selection(&mut self, delta: isize) {
    move_index(
      &mut self.selected_severity,
      SeverityFilter::ALL.len(),
      delta,
    );
  }

  fn clamp(&mut self) {
    clamp_index(&mut self.selected_severity, SeverityFilter::ALL.len());
  }
}

#[derive(Debug, Clone)]
pub struct TuiModel {
  pub view: View,
  pub daemon: DaemonViewModel,
  pub search: String,
  pub search_mode: bool,
  pub entrypoint_selected: usize,
  pub domain_selected: usize,
  #[cfg(windows)]
  pub iis: IisViewModel,
  pub logs: LogViewModel,
  pub settings: SettingsViewModel,
  snapshot: GuiStateSnapshot,
}

#[cfg(windows)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IisViewModel {
  pub bindings: Vec<IisBinding>,
  pub selected: usize,
  pub route_host_input: String,
  pub route_host_input_binding_id: Option<String>,
  pub route_host_inputs: BTreeMap<String, String>,
  pub route_host_input_mode: bool,
  pub loading: bool,
  pub request_in_flight: bool,
  pub action_in_flight: bool,
  pub last_error: Option<String>,
}

#[cfg(windows)]
impl IisViewModel {
  pub fn mark_loading(&mut self) {
    self.loading = true;
    self.request_in_flight = true;
    self.last_error = None;
  }

  pub fn apply_response(&mut self, response: QueryIisBindingsResponse) {
    self.loading = false;
    self.request_in_flight = false;
    self.bindings = response.bindings;
    self.route_host_inputs.retain(|binding_id, _| {
      self
        .bindings
        .iter()
        .any(|binding| binding.identity.binding_id == *binding_id)
    });
    if self
      .route_host_input_binding_id
      .as_ref()
      .is_some_and(|binding_id| {
        !self
          .bindings
          .iter()
          .any(|binding| binding.identity.binding_id == *binding_id)
      })
    {
      self.route_host_input_mode = false;
      self.route_host_input.clear();
      self.route_host_input_binding_id = None;
    }
    self.last_error = response.issue.map(|issue| issue.message);
    self.clamp();
  }

  pub fn apply_error(&mut self, message: String) {
    self.loading = false;
    self.request_in_flight = false;
    self.last_error = Some(message);
  }

  pub fn selected_binding(&self) -> Option<IisBinding> {
    self.bindings.get(self.selected).cloned()
  }

  pub fn begin_route_host_input(&mut self, binding_id: String) {
    self.route_host_input = self
      .route_host_inputs
      .get(&binding_id)
      .cloned()
      .unwrap_or_default();
    self.route_host_input_binding_id = Some(binding_id);
    self.route_host_input_mode = true;
  }

  pub fn finish_route_host_input(&mut self) {
    if let Some(binding_id) = self.route_host_input_binding_id.as_ref() {
      let route_host = self.route_host_input.trim();
      if route_host.is_empty() {
        self.route_host_inputs.remove(binding_id);
      } else {
        self
          .route_host_inputs
          .insert(binding_id.clone(), route_host.to_string());
        self.route_host_input = route_host.to_string();
      }
    }
    self.route_host_input_mode = false;
  }

  pub fn route_host_for_binding(&self, binding_id: &str) -> Option<&str> {
    self
      .route_host_inputs
      .get(binding_id)
      .map(String::as_str)
      .filter(|host| !host.trim().is_empty())
  }

  pub fn push_route_host_char(&mut self, ch: char) {
    self.route_host_input.push(ch);
  }

  pub fn pop_route_host_char(&mut self) {
    self.route_host_input.pop();
  }

  fn move_selection(&mut self, delta: isize) {
    move_index(&mut self.selected, self.bindings.len(), delta);
  }

  fn clamp(&mut self) {
    clamp_index(&mut self.selected, self.bindings.len());
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogTarget {
  pub registration_id: String,
  pub domain_name: String,
  pub stream: LogStreamIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogViewModel {
  pub target: Option<LogTarget>,
  pub entries: Vec<LogEntry>,
  pub next_cursor: Option<String>,
  pub loading: bool,
  pub request_in_flight: bool,
  pub paused: bool,
  pub minimum_severity: Option<LogSeverity>,
  pub status: LogStreamStatus,
  pub has_gap: bool,
  pub has_more_before: bool,
  pub truncated_by_retention: bool,
  pub read_error: Option<String>,
}

impl Default for LogViewModel {
  fn default() -> Self {
    Self {
      target: None,
      entries: Vec::new(),
      next_cursor: None,
      loading: false,
      request_in_flight: false,
      paused: false,
      minimum_severity: None,
      status: LogStreamStatus::Empty,
      has_gap: false,
      has_more_before: false,
      truncated_by_retention: false,
      read_error: None,
    }
  }
}

impl LogViewModel {
  pub fn reset_for_target(&mut self, target: LogTarget) {
    let paused = self.paused;
    let minimum_severity = self.minimum_severity;
    *self = Self {
      target: Some(target),
      paused,
      minimum_severity,
      loading: true,
      request_in_flight: false,
      ..Self::default()
    };
  }

  pub fn reset_for_filter(&mut self, minimum_severity: Option<LogSeverity>) {
    self.entries.clear();
    self.next_cursor = None;
    self.loading = self.target.is_some();
    self.minimum_severity = minimum_severity;
    self.status = LogStreamStatus::Empty;
    self.has_gap = false;
    self.has_more_before = false;
    self.truncated_by_retention = false;
    self.read_error = None;
  }

  pub fn mark_loading(&mut self) {
    self.loading = true;
    self.request_in_flight = true;
    self.read_error = None;
  }

  pub fn apply_response(&mut self, response: cadder_protocol::QueryLogsResponse) {
    self.loading = false;
    self.request_in_flight = false;
    self.status = response.stream_status;
    self.has_gap = response.has_gap;
    self.has_more_before = response.has_more_before;
    self.truncated_by_retention = response.truncated_by_retention;
    self.next_cursor = response.next_cursor;
    self.read_error = None;
    self.entries.extend(response.entries);
  }

  pub fn apply_read_error(&mut self, message: String) {
    self.loading = false;
    self.request_in_flight = false;
    self.status = LogStreamStatus::ReadError;
    self.read_error = Some(message);
  }

  pub fn active_stream(&self) -> Option<LogStreamIdentity> {
    self.target.as_ref().map(|target| target.stream.clone())
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
  pub runtime: String,
  pub config: String,
  pub entrypoints: usize,
  pub domains: usize,
  pub active_domains: usize,
}

impl Default for TuiModel {
  fn default() -> Self {
    Self {
      view: View::Overview,
      daemon: DaemonViewModel::default(),
      search: String::new(),
      search_mode: false,
      entrypoint_selected: 0,
      domain_selected: 0,
      #[cfg(windows)]
      iis: IisViewModel::default(),
      logs: LogViewModel::default(),
      settings: SettingsViewModel::default(),
      snapshot: GuiStateSnapshot {
        captured_at_utc: chrono::Utc::now(),
        registrations: Vec::new(),
        runtime: cadder_protocol::RuntimeState::idle(),
        config: cadder_protocol::ConfigState::idle(),
      },
    }
  }
}

impl TuiModel {
  pub fn set_snapshot(&mut self, snapshot: GuiStateSnapshot) {
    self.snapshot = snapshot;
    self.clamp_selections();
  }

  pub fn mark_daemon_connected(&mut self) {
    self.daemon.mark_connected();
  }

  pub fn mark_daemon_unavailable(
    &mut self,
    kind: DaemonUnavailableKind,
    message: impl Into<String>,
  ) {
    self.daemon.mark_unavailable(kind, message);
  }

  pub fn mark_daemon_starting(&mut self, message: impl Into<String>) {
    self.daemon.mark_starting(message);
  }

  pub fn mark_daemon_start_failed(&mut self, message: impl Into<String>) {
    self.daemon.mark_start_failed(message);
  }

  pub fn can_use_daemon(&self) -> bool {
    self.daemon.state.is_connected()
  }

  pub fn can_start_daemon(&self) -> bool {
    self.daemon.state.can_start()
  }

  pub fn snapshot(&self) -> &GuiStateSnapshot {
    &self.snapshot
  }

  pub fn next_view(&mut self) {
    let next = (self.view.index() + 1) % View::ALL.len();
    self.view = View::ALL[next];
    self.clamp_current_selection();
  }

  pub fn previous_view(&mut self) {
    let index = self.view.index();
    self.view = View::ALL[(index + View::ALL.len() - 1) % View::ALL.len()];
    self.clamp_current_selection();
  }

  pub fn move_selection(&mut self, delta: isize) {
    match self.view {
      View::Entrypoints => {
        let len = self.filtered_registrations().len();
        move_index(&mut self.entrypoint_selected, len, delta);
      }
      View::Domains => {
        let len = self.filtered_domains().len();
        move_index(&mut self.domain_selected, len, delta);
      }
      #[cfg(windows)]
      View::IisHandoff => self.iis.move_selection(delta),
      View::Settings => self.settings.move_severity_selection(delta),
      _ => {}
    }
  }

  pub fn clamp_current_selection(&mut self) {
    match self.view {
      View::Entrypoints => {
        let len = self.filtered_registrations().len();
        clamp_index(&mut self.entrypoint_selected, len);
      }
      View::Domains => {
        let len = self.filtered_domains().len();
        clamp_index(&mut self.domain_selected, len);
      }
      #[cfg(windows)]
      View::IisHandoff => self.iis.clamp(),
      View::Settings => self.settings.clamp(),
      _ => {}
    }
  }

  pub fn clamp_selections(&mut self) {
    let entrypoint_len = self.filtered_registrations().len();
    let domain_len = self.filtered_domains().len();
    clamp_index(&mut self.entrypoint_selected, entrypoint_len);
    clamp_index(&mut self.domain_selected, domain_len);
    #[cfg(windows)]
    self.iis.clamp();
    self.settings.clamp();
  }

  pub fn sync_settings_severity_from_logs(&mut self) {
    self
      .settings
      .select_filter(SeverityFilter::from_minimum_severity(
        self.logs.minimum_severity,
      ));
  }

  pub fn summary(&self) -> Summary {
    let domains = self
      .snapshot
      .registrations
      .iter()
      .map(|registration| registration.registered_domains.len())
      .sum();
    let active_domains = self
      .snapshot
      .registrations
      .iter()
      .flat_map(|registration| &registration.registered_domains)
      .filter(|domain| domain.activation_state.is_enabled())
      .count();
    Summary {
      runtime: format!("{:?}", self.snapshot.runtime.status),
      config: format!("{:?}", self.snapshot.config.status),
      entrypoints: self.snapshot.registrations.len(),
      domains,
      active_domains,
    }
  }

  pub fn filtered_registrations(&self) -> Vec<&EntrypointRegistration> {
    self
      .snapshot
      .registrations
      .iter()
      .filter(|registration| {
        self.search.is_empty()
          || registration.registration_id.contains(&self.search)
          || registration
            .source_working_directory
            .raw
            .contains(&self.search)
          || registration.source_config_path.raw.contains(&self.search)
          || registration
            .registered_domains
            .iter()
            .any(|domain| domain.name.canonical.contains(&self.search))
      })
      .collect()
  }

  pub fn filtered_domains(&self) -> Vec<(&EntrypointRegistration, &RegisteredDomain)> {
    self
      .snapshot
      .registrations
      .iter()
      .flat_map(|registration| {
        registration
          .registered_domains
          .iter()
          .map(move |domain| (registration, domain))
      })
      .filter(|(registration, domain)| {
        self.search.is_empty()
          || registration.registration_id.contains(&self.search)
          || domain.name.canonical.contains(&self.search)
      })
      .collect()
  }

  pub fn selected_entrypoint(&self) -> Option<&EntrypointRegistration> {
    self
      .filtered_registrations()
      .get(self.entrypoint_selected)
      .copied()
  }

  pub fn selected_domain(&self) -> Option<(String, RegisteredDomain)> {
    self
      .filtered_domains()
      .get(self.domain_selected)
      .map(|(registration, domain)| (registration.registration_id.clone(), (*domain).clone()))
  }

  pub fn selected_log_target(&self) -> Option<LogTarget> {
    self
      .selected_domain()
      .map(|(registration_id, domain)| LogTarget {
        registration_id,
        domain_name: domain.name.canonical,
        stream: domain.log_stream,
      })
  }

  pub fn open_selected_domain_logs(&mut self) -> bool {
    let Some(target) = self.selected_log_target() else {
      return false;
    };
    self.logs.reset_for_target(target);
    self.view = View::Logs;
    true
  }
}

fn move_index(index: &mut usize, len: usize, delta: isize) {
  if len == 0 {
    *index = 0;
    return;
  }
  *index = (*index as isize + delta).clamp(0, len.saturating_sub(1) as isize) as usize;
}

fn clamp_index(index: &mut usize, len: usize) {
  if len == 0 {
    *index = 0;
    return;
  }
  *index = (*index).min(len.saturating_sub(1));
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_protocol::{
    ActivationState, ConfigApplyStatus, ConfigDiagnostic, ConfigState, EntrypointInstanceIdentity,
    LogAttributionKind, LogEntryKind, LogStreamIdentity, OwnerProcessIdentity, QueryLogsResponse,
    RegisteredDomain, RuntimeDiagnostic, RuntimeState, RuntimeStatus, SourcePath,
  };
  use chrono::Utc;

  fn snapshot_with_registrations(registrations: Vec<EntrypointRegistration>) -> GuiStateSnapshot {
    GuiStateSnapshot {
      captured_at_utc: Utc::now(),
      registrations,
      runtime: RuntimeState::idle(),
      config: ConfigState::idle(),
    }
  }

  fn snapshot() -> GuiStateSnapshot {
    snapshot_with_registrations(vec![registration(
      "shim-1",
      "/work/app",
      vec![
        RegisteredDomain::active("app.localhost"),
        RegisteredDomain {
          activation_state: ActivationState::Inactive,
          ..RegisteredDomain::active("api.localhost")
        },
      ],
    )])
  }

  fn registration(
    registration_id: &str,
    working_directory: &str,
    registered_domains: Vec<RegisteredDomain>,
  ) -> EntrypointRegistration {
    let now = Utc::now();
    let identity = EntrypointInstanceIdentity {
      instance_id: registration_id.to_string(),
      started_at_utc: now,
      shim_session_nonce: format!("{registration_id}-nonce"),
    };
    EntrypointRegistration {
      registration_id: registration_id.to_string(),
      entrypoint_instance: identity.clone(),
      source_working_directory: SourcePath::new(working_directory, None),
      source_config_path: SourcePath::new(format!("{working_directory}/Caddyfile"), None),
      registered_domains,
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 1,
        process_start_time_utc: now,
        shim_session_nonce: identity.shim_session_nonce,
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint(registration_id),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
    }
  }

  #[test]
  fn summary_counts_domains() {
    let mut model = TuiModel::default();
    model.set_snapshot(snapshot());

    let summary = model.summary();

    assert_eq!(summary.entrypoints, 1);
    assert_eq!(summary.domains, 2);
    assert_eq!(summary.active_domains, 1);
  }

  #[test]
  fn daemon_connectivity_state_transitions_are_explicit() {
    let mut model = TuiModel::default();

    assert!(matches!(
      model.daemon.state,
      DaemonConnectionState::NotRunning { .. }
    ));
    assert!(!model.can_use_daemon());
    assert!(model.can_start_daemon());

    model.mark_daemon_starting("starting cadderd");
    assert_eq!(model.daemon.state.label(), "Starting");
    assert!(!model.can_use_daemon());
    assert!(!model.can_start_daemon());

    model.mark_daemon_unavailable(DaemonUnavailableKind::ConnectionFailed, "permission denied");
    assert!(matches!(
      model.daemon.state,
      DaemonConnectionState::ConnectionFailed { .. }
    ));
    assert!(model.can_start_daemon());

    model.mark_daemon_start_failed("could not find cadderd");
    assert_eq!(model.daemon.state.label(), "Start failed");
    assert!(model.daemon.state.guidance().contains("retry"));

    model.mark_daemon_connected();
    assert!(model.can_use_daemon());
    assert_eq!(model.daemon.state.label(), "Connected");
  }

  #[test]
  fn view_titles_indices_and_wrapping_navigation_are_stable() {
    #[cfg(windows)]
    let expected = [
      ("Overview", 0),
      ("Entrypoints", 1),
      ("Domains", 2),
      ("IIS Handoff", 3),
      ("Logs", 4),
      ("Settings", 5),
      ("Diagnostics", 6),
    ];
    #[cfg(not(windows))]
    let expected = [
      ("Overview", 0),
      ("Entrypoints", 1),
      ("Domains", 2),
      ("Logs", 3),
      ("Settings", 4),
      ("Diagnostics", 5),
    ];

    assert_eq!(View::ALL.map(|view| (view.title(), view.index())), expected);

    let mut model = TuiModel::default();
    model.previous_view();
    assert_eq!(model.view, View::Diagnostics);
    model.next_view();
    assert_eq!(model.view, View::Overview);
  }

  #[test]
  fn move_selection_clamps_to_visible_rows() {
    let mut model = TuiModel::default();
    model.set_snapshot(snapshot());
    model.view = View::Domains;

    model.move_selection(10);
    assert_eq!(model.domain_selected, 1);
    model.move_selection(-10);
    assert_eq!(model.domain_selected, 0);
  }

  #[cfg(windows)]
  #[test]
  fn iis_selection_clamps_to_discovered_bindings() {
    let mut model = TuiModel {
      view: View::IisHandoff,
      ..TuiModel::default()
    };
    model.iis.bindings = vec![
      cadder_protocol::IisBinding {
        identity: cadder_protocol::IisBindingIdentity {
          binding_id: "one".to_string(),
          site_name: "Default Web Site".to_string(),
          protocol: "http".to_string(),
          binding_information: "*:80:app.localhost".to_string(),
        },
        ip_address: "*".to_string(),
        port: 80,
        host_header: "app.localhost".to_string(),
        domain_key: Some("app.localhost".to_string()),
        handoff_state: cadder_protocol::IisHandoffState::Available,
        issue: None,
        restore_metadata: None,
      },
      cadder_protocol::IisBinding {
        identity: cadder_protocol::IisBindingIdentity {
          binding_id: "two".to_string(),
          site_name: "Default Web Site".to_string(),
          protocol: "https".to_string(),
          binding_information: "*:443:secure.localhost".to_string(),
        },
        ip_address: "*".to_string(),
        port: 443,
        host_header: "secure.localhost".to_string(),
        domain_key: Some("secure.localhost".to_string()),
        handoff_state: cadder_protocol::IisHandoffState::Unsupported,
        issue: None,
        restore_metadata: None,
      },
    ];

    model.move_selection(10);
    assert_eq!(model.iis.selected, 1);
    model.iis.bindings.pop();
    model.clamp_current_selection();
    assert_eq!(model.iis.selected, 0);
  }

  #[cfg(not(windows))]
  #[test]
  fn iis_view_is_absent_on_non_windows() {
    assert_eq!(
      View::ALL.map(|view| view.title()),
      [
        "Overview",
        "Entrypoints",
        "Domains",
        "Logs",
        "Settings",
        "Diagnostics"
      ]
    );
  }

  #[test]
  fn top_level_navigation_preserves_per_view_selection() {
    let mut model = TuiModel::default();
    model.set_snapshot(snapshot_with_registrations(vec![
      registration(
        "shim-1",
        "/work/app",
        vec![RegisteredDomain::active("app.localhost")],
      ),
      registration(
        "shim-2",
        "/work/api",
        vec![
          RegisteredDomain::active("api.localhost"),
          RegisteredDomain::active("admin.localhost"),
        ],
      ),
    ]));

    model.view = View::Entrypoints;
    model.move_selection(1);
    assert_eq!(model.entrypoint_selected, 1);

    model.next_view();
    model.move_selection(2);
    assert_eq!(model.view, View::Domains);
    assert_eq!(model.domain_selected, 2);

    model.previous_view();
    assert_eq!(model.view, View::Entrypoints);
    assert_eq!(model.entrypoint_selected, 1);

    model.next_view();
    assert_eq!(model.view, View::Domains);
    assert_eq!(model.domain_selected, 2);
  }

  #[test]
  fn set_snapshot_clamps_all_row_selections() {
    let mut model = TuiModel::default();
    model.set_snapshot(snapshot_with_registrations(vec![
      registration(
        "shim-1",
        "/work/app",
        vec![RegisteredDomain::active("app.localhost")],
      ),
      registration(
        "shim-2",
        "/work/api",
        vec![RegisteredDomain::active("api.localhost")],
      ),
    ]));
    model.entrypoint_selected = 1;
    model.domain_selected = 1;

    model.set_snapshot(snapshot_with_registrations(vec![registration(
      "shim-1",
      "/work/app",
      vec![RegisteredDomain::active("app.localhost")],
    )]));

    assert_eq!(model.entrypoint_selected, 0);
    assert_eq!(model.domain_selected, 0);
  }

  #[test]
  fn settings_severity_selection_maps_to_log_filter() {
    let mut model = TuiModel {
      view: View::Settings,
      ..TuiModel::default()
    };

    assert_eq!(model.settings.selected_filter(), SeverityFilter::All);
    assert_eq!(model.settings.selected_filter().minimum_severity(), None);

    model.move_selection(2);
    assert_eq!(model.settings.selected_filter(), SeverityFilter::Warn);
    assert_eq!(
      model.settings.selected_filter().minimum_severity(),
      Some(LogSeverity::Warn)
    );

    model.logs.minimum_severity = Some(LogSeverity::Error);
    model.sync_settings_severity_from_logs();

    assert_eq!(model.settings.selected_filter(), SeverityFilter::Error);
  }

  #[test]
  fn filters_domains_by_search() {
    let mut model = TuiModel::default();
    model.set_snapshot(snapshot());
    model.search = "api".to_string();

    let domains = model.filtered_domains();

    assert_eq!(domains.len(), 1);
    assert_eq!(domains[0].1.name.canonical, "api.localhost");
  }

  #[test]
  fn domain_rows_keep_entrypoint_association() {
    let mut model = TuiModel::default();
    model.set_snapshot(snapshot_with_registrations(vec![
      registration(
        "shim-1",
        "/work/app",
        vec![RegisteredDomain::active("app.localhost")],
      ),
      registration(
        "shim-2",
        "/work/api",
        vec![RegisteredDomain::active("api.localhost")],
      ),
    ]));

    let domains = model.filtered_domains();

    assert_eq!(domains.len(), 2);
    assert_eq!(domains[0].0.registration_id, "shim-1");
    assert_eq!(domains[0].1.name.canonical, "app.localhost");
    assert_eq!(domains[1].0.registration_id, "shim-2");
    assert_eq!(domains[1].1.name.canonical, "api.localhost");
  }

  #[test]
  fn opens_selected_domain_log_target() {
    let mut model = TuiModel::default();
    model.set_snapshot(snapshot());
    model.view = View::Domains;

    assert!(model.open_selected_domain_logs());

    let target = model.logs.target.as_ref().unwrap();
    assert_eq!(model.view, View::Logs);
    assert_eq!(target.registration_id, "shim-1");
    assert_eq!(target.domain_name, "app.localhost");
    assert_eq!(target.stream, LogStreamIdentity::domain("app.localhost"));
    assert!(model.logs.loading);
  }

  #[test]
  fn severity_change_resets_log_cursor_and_entries() {
    let mut logs = LogViewModel {
      target: Some(LogTarget {
        registration_id: "shim-1".to_string(),
        domain_name: "app.localhost".to_string(),
        stream: LogStreamIdentity::domain("app.localhost"),
      }),
      next_cursor: Some("seq:10".to_string()),
      entries: vec![cadder_protocol::LogEntry {
        sequence_number: 10,
        timestamp_utc: Utc::now(),
        severity: LogSeverity::Info,
        stream: LogStreamIdentity::domain("app.localhost"),
        attribution_kind: cadder_protocol::LogAttributionKind::Domain,
        entry_kind: cadder_protocol::LogEntryKind::Normal,
        raw_message: "first".to_string(),
        domain_key: Some("app.localhost".to_string()),
        source_registration_id: None,
        source_instance_id: None,
        operation: None,
      }],
      ..LogViewModel::default()
    };

    logs.reset_for_filter(Some(LogSeverity::Error));

    assert!(logs.entries.is_empty());
    assert_eq!(logs.next_cursor, None);
    assert_eq!(logs.minimum_severity, Some(LogSeverity::Error));
    assert!(logs.loading);
  }

  #[test]
  fn severity_change_preserves_in_flight_log_request_state() {
    let mut logs = LogViewModel {
      target: Some(LogTarget {
        registration_id: "shim-1".to_string(),
        domain_name: "app.localhost".to_string(),
        stream: LogStreamIdentity::domain("app.localhost"),
      }),
      loading: true,
      request_in_flight: true,
      ..LogViewModel::default()
    };

    logs.reset_for_filter(Some(LogSeverity::Warn));

    assert!(logs.loading);
    assert!(logs.request_in_flight);
    assert_eq!(logs.minimum_severity, Some(LogSeverity::Warn));
  }

  #[test]
  fn log_response_applies_status_cursor_and_retention_metadata() {
    let mut logs = LogViewModel {
      target: Some(LogTarget {
        registration_id: "shim-1".to_string(),
        domain_name: "app.localhost".to_string(),
        stream: LogStreamIdentity::domain("app.localhost"),
      }),
      loading: true,
      request_in_flight: true,
      ..LogViewModel::default()
    };

    logs.apply_response(QueryLogsResponse {
      request_id: "logs".to_string(),
      accepted: true,
      message: "ok".to_string(),
      stream: LogStreamIdentity::domain("app.localhost"),
      stream_status: LogStreamStatus::Stale,
      entries: vec![cadder_protocol::LogEntry {
        sequence_number: 7,
        timestamp_utc: Utc::now(),
        severity: LogSeverity::Warn,
        stream: LogStreamIdentity::domain("app.localhost"),
        attribution_kind: LogAttributionKind::Domain,
        entry_kind: LogEntryKind::Normal,
        raw_message: "retained warning".to_string(),
        domain_key: Some("app.localhost".to_string()),
        source_registration_id: None,
        source_instance_id: None,
        operation: None,
      }],
      next_cursor: Some("seq:7".to_string()),
      has_gap: true,
      has_more_before: true,
      truncated_by_retention: true,
    });

    assert!(!logs.loading);
    assert!(!logs.request_in_flight);
    assert_eq!(logs.status, LogStreamStatus::Stale);
    assert_eq!(logs.next_cursor.as_deref(), Some("seq:7"));
    assert!(logs.has_gap);
    assert!(logs.has_more_before);
    assert!(logs.truncated_by_retention);
  }

  #[test]
  fn diagnostics_snapshot_is_available_to_diagnostics_view() {
    let mut model = TuiModel::default();
    let mut snapshot = snapshot();
    snapshot.config = ConfigState {
      status: ConfigApplyStatus::Failed,
      last_attempted_at_utc: Some(Utc::now()),
      last_successful_reload_at_utc: None,
      effective_config_hash: None,
      diagnostics: vec![ConfigDiagnostic {
        code: "runtime-apply-failed".to_string(),
        message: "reload failed".to_string(),
        domain_key: None,
        source_config_paths: vec!["/work/app/Caddyfile".to_string()],
      }],
    };
    snapshot.runtime = RuntimeState {
      status: RuntimeStatus::Unhealthy,
      binary_path: None,
      version: None,
      process_id: None,
      admin_endpoint: None,
      diagnostics: vec![RuntimeDiagnostic {
        code: "runtime-error".to_string(),
        message: "process exited".to_string(),
        operation: Some("run".to_string()),
      }],
    };

    model.set_snapshot(snapshot);

    assert_eq!(
      model.snapshot().config.diagnostics[0].code,
      "runtime-apply-failed"
    );
    assert_eq!(
      model.snapshot().runtime.diagnostics[0].code,
      "runtime-error"
    );
  }
}
