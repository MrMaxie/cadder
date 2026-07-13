pub const PROTOCOL_VERSION: u16 = 2;
pub const MIN_COMPATIBLE_PROTOCOL_VERSION: u16 = 1;

pub mod autostart;
pub mod client;
pub mod commands;
pub mod envelopes;
pub mod errors;
pub mod events;
pub mod handshake;
pub mod history;
pub mod identifiers;
pub mod iis;
pub mod logs;
pub mod message_types;
pub mod mutations;
pub mod operations;
pub mod responses;
pub mod state;
pub mod version;
pub mod wire;

pub use autostart::*;
pub use client::*;
pub use commands::*;
pub use envelopes::*;
pub use errors::*;
pub use events::*;
pub use handshake::*;
pub use history::*;
pub use identifiers::*;
pub use iis::*;
pub use logs::*;
pub use message_types::*;
pub use mutations::*;
pub use operations::*;
pub use responses::*;
pub use state::*;
pub use version::*;
pub use wire::*;
#[cfg(test)]
mod tests {
  use chrono::Utc;
  use serde::{Serialize, de::DeserializeOwned};

  use super::*;

  #[test]
  fn canonicalizes_domains() {
    assert_eq!(
      canonicalize_domain("  WWW.Example.Localhost. "),
      "www.example.localhost"
    );
  }

  #[test]
  fn serializes_envelope_with_camel_case_payload() {
    let request = QueryStateRequest {
      request_id: "request-1".to_string(),
    };
    let envelope = IpcEnvelope::new(message_types::QUERY_STATE_REQUEST, &request).unwrap();
    let json = serde_json::to_string(&envelope).unwrap();

    assert!(json.contains(&format!("\"protocolVersion\":{PROTOCOL_VERSION}")));
    assert!(json.contains("\"capabilities\""));
    assert!(json.contains("\"supportedCapabilityVersions\""));
    assert!(json.contains("\"type\":\"query-state-request\""));
    assert!(json.contains("\"requestId\":\"request-1\""));

    let decoded: QueryStateRequest = envelope.decode().unwrap();
    assert_eq!(decoded, request);
  }

  #[test]
  fn decode_rejects_protocol_version_below_compatibility_floor() {
    let request = QueryStateRequest {
      request_id: "request-1".to_string(),
    };
    let mut envelope = IpcEnvelope::new(message_types::QUERY_STATE_REQUEST, &request).unwrap();
    envelope.protocol_version = MIN_COMPATIBLE_PROTOCOL_VERSION.saturating_sub(1);

    let error = envelope.decode::<QueryStateRequest>().unwrap_err();

    assert!(
      error
        .to_string()
        .contains("unsupported Cadder IPC protocol version")
    );
  }

  #[test]
  fn decode_accepts_newer_protocol_version_with_supported_payload() {
    let request = QueryStateRequest {
      request_id: "request-1".to_string(),
    };
    let mut envelope = IpcEnvelope::new(message_types::QUERY_STATE_REQUEST, &request).unwrap();
    envelope.protocol_version = PROTOCOL_VERSION.saturating_add(10);

    let decoded: QueryStateRequest = envelope.decode().unwrap();

    assert_eq!(decoded, request);
  }

  #[test]
  fn decode_accepts_older_compatible_protocol_version_with_known_fields() {
    let request = QueryStateRequest {
      request_id: "request-1".to_string(),
    };
    let mut envelope = IpcEnvelope::new(message_types::QUERY_STATE_REQUEST, &request).unwrap();
    envelope.protocol_version = MIN_COMPATIBLE_PROTOCOL_VERSION;
    envelope.payload["futureOptionalField"] = serde_json::json!("ignored");

    let decoded: QueryStateRequest = envelope.decode().unwrap();

    assert_eq!(decoded, request);
  }

  #[test]
  fn envelope_decode_reports_protocol_floor_compatibility_error() {
    let request = QueryStateRequest {
      request_id: "request-1".to_string(),
    };
    let mut envelope = IpcEnvelope::new(message_types::QUERY_STATE_REQUEST, &request).unwrap();
    envelope.protocol_version = MIN_COMPATIBLE_PROTOCOL_VERSION.saturating_sub(1);

    let error = envelope.decode::<QueryStateRequest>().unwrap_err();

    assert_eq!(error.kind, ProtocolErrorKind::IncompatibleProtocolVersion);
    assert_eq!(
      error.minimum_compatible_protocol_version,
      Some(MIN_COMPATIBLE_PROTOCOL_VERSION)
    );
    assert_eq!(error.current_protocol_version, Some(PROTOCOL_VERSION));
    assert!(
      error
        .guidance
        .as_deref()
        .is_some_and(|guidance| guidance.contains("Upgrade the Cadder client"))
    );
  }

  #[test]
  fn protocol_capabilities_report_unsupported_capability_as_typed_error() {
    let capabilities = ProtocolCapabilities::current();

    let error = capabilities
      .require("future-dashboard")
      .expect_err("future dashboard must not be supported by the reset contract");

    assert_eq!(error.kind, ProtocolErrorKind::UnsupportedCapability);
    assert_eq!(
      error.required_capability.as_deref(),
      Some("future-dashboard")
    );
    assert_eq!(error.required_capability_version, Some(1));
    assert!(error.supported_capabilities.contains(&"logs".to_string()));
    assert!(
      error
        .supported_capability_versions
        .iter()
        .any(|capability| capability.name == "logs" && capability.version == 1)
    );
    assert!(
      error
        .guidance
        .as_deref()
        .is_some_and(|guidance| guidance.contains("future-dashboard"))
    );
  }

  #[test]
  fn protocol_capabilities_check_versioned_capability_ranges() {
    let capabilities = ProtocolCapabilities {
      protocol_version: PROTOCOL_VERSION,
      minimum_compatible_protocol_version: MIN_COMPATIBLE_PROTOCOL_VERSION,
      supported_capabilities: vec!["logs".to_string()].into_boxed_slice(),
      supported_capability_versions: vec![ProtocolCapability::new("logs", 3, 2)].into_boxed_slice(),
    };

    assert!(capabilities.supports_version("logs", 2));
    assert!(capabilities.supports_version("logs", 3));
    assert!(!capabilities.supports_version("logs", 1));

    let error = capabilities
      .require_version("logs", 4)
      .expect_err("future logs capability version should require a newer node");

    assert_eq!(error.kind, ProtocolErrorKind::UnsupportedCapability);
    assert_eq!(error.required_capability.as_deref(), Some("logs"));
    assert_eq!(error.required_capability_version, Some(4));
    assert_eq!(error.supported_capability_versions[0].version, 3);
  }

  #[test]
  fn access_denied_error_reports_denied_operation() {
    let error = ProtocolError::access_denied(
      "set-autostart-request",
      "IPC policy rejected the request.",
      Some("Use the owning local account.".to_string()),
    );

    assert_eq!(error.kind, ProtocolErrorKind::AccessDenied);
    assert_eq!(
      error.denied_operation.as_deref(),
      Some("set-autostart-request")
    );
    assert!(
      error
        .guidance
        .as_deref()
        .is_some_and(|guidance| guidance.contains("owning local account"))
    );
  }

  #[test]
  fn envelope_new_returns_payload_serialization_errors() {
    struct FailingPayload;

    impl Serialize for FailingPayload {
      fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
      where
        S: serde::Serializer,
      {
        Err(serde::ser::Error::custom("payload serialization failed"))
      }
    }

    let error = IpcEnvelope::new("failing-payload", &FailingPayload).unwrap_err();

    assert!(error.to_string().contains("payload serialization failed"));
  }

  #[test]
  fn activation_state_enabled_variants_are_stable() {
    let cases = [
      (ActivationState::Unknown, false),
      (ActivationState::Registered, true),
      (ActivationState::Activating, true),
      (ActivationState::Active, true),
      (ActivationState::Inactive, false),
      (ActivationState::Faulted, false),
    ];

    for (state, expected) in cases {
      assert_eq!(state.is_enabled(), expected, "{state:?}");
    }
  }

  #[test]
  fn enum_contract_variants_roundtrip_with_camel_case_names() {
    fn roundtrip<T>(values: &[T], expected: &[&str])
    where
      T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
      let json = serde_json::to_string(values).unwrap();
      for variant in expected {
        assert!(json.contains(variant), "{json} should contain {variant}");
      }
      let decoded: Vec<T> = serde_json::from_str(&json).unwrap();
      assert_eq!(decoded, values);
    }

    roundtrip(
      &[
        ActivationState::Unknown,
        ActivationState::Registered,
        ActivationState::Activating,
        ActivationState::Active,
        ActivationState::Inactive,
        ActivationState::Faulted,
      ],
      &[
        "unknown",
        "registered",
        "activating",
        "active",
        "inactive",
        "faulted",
      ],
    );
    roundtrip(
      &[
        RuntimeStatus::Unknown,
        RuntimeStatus::NotResolved,
        RuntimeStatus::Resolved,
        RuntimeStatus::Running,
        RuntimeStatus::Unhealthy,
        RuntimeStatus::Idle,
      ],
      &[
        "unknown",
        "notResolved",
        "resolved",
        "running",
        "unhealthy",
        "idle",
      ],
    );
    roundtrip(
      &[
        ConfigApplyStatus::Unknown,
        ConfigApplyStatus::NotApplied,
        ConfigApplyStatus::Applied,
        ConfigApplyStatus::Failed,
        ConfigApplyStatus::Idle,
      ],
      &["unknown", "notApplied", "applied", "failed", "idle"],
    );
    roundtrip(
      &[
        LogSeverity::Unknown,
        LogSeverity::Trace,
        LogSeverity::Debug,
        LogSeverity::Info,
        LogSeverity::Warn,
        LogSeverity::Error,
        LogSeverity::Fatal,
      ],
      &[
        "unknown", "trace", "debug", "info", "warn", "error", "fatal",
      ],
    );
    roundtrip(
      &[
        LogAttributionKind::Unknown,
        LogAttributionKind::Runtime,
        LogAttributionKind::RuntimeControl,
        LogAttributionKind::Entrypoint,
        LogAttributionKind::Domain,
      ],
      &[
        "unknown",
        "runtime",
        "runtimeControl",
        "entrypoint",
        "domain",
      ],
    );
    roundtrip(
      &[
        LogEntryKind::Normal,
        LogEntryKind::Lifecycle,
        LogEntryKind::IngestionOverflow,
        LogEntryKind::RetentionGap,
      ],
      &["normal", "lifecycle", "ingestionOverflow", "retentionGap"],
    );
    roundtrip(
      &[
        LogStreamStatus::Unknown,
        LogStreamStatus::Empty,
        LogStreamStatus::Active,
        LogStreamStatus::Stale,
        LogStreamStatus::Removed,
        LogStreamStatus::ReadError,
      ],
      &[
        "unknown",
        "empty",
        "active",
        "stale",
        "removed",
        "readError",
      ],
    );
    roundtrip(
      &[
        HistoryKind::Registration,
        HistoryKind::Runtime,
        HistoryKind::Config,
        HistoryKind::Autostart,
        HistoryKind::Iis,
        HistoryKind::Log,
      ],
      &[
        "registration",
        "runtime",
        "config",
        "autostart",
        "iis",
        "log",
      ],
    );
    roundtrip(
      &[AutostartMode::Disabled, AutostartMode::Daemon],
      &["disabled", "daemon"],
    );
    roundtrip(
      &[
        AutostartStatus::Unknown,
        AutostartStatus::Enabled,
        AutostartStatus::Disabled,
        AutostartStatus::Unsupported,
        AutostartStatus::Misconfigured,
      ],
      &[
        "unknown",
        "enabled",
        "disabled",
        "unsupported",
        "misconfigured",
      ],
    );
    roundtrip(
      &[
        StateChangeKind::Snapshot,
        StateChangeKind::RegistrationsChanged,
        StateChangeKind::RuntimeChanged,
      ],
      &["snapshot", "registrationsChanged", "runtimeChanged"],
    );
    roundtrip(
      &[
        IisHandoffState::Available,
        IisHandoffState::HandedOff,
        IisHandoffState::Unsupported,
        IisHandoffState::Conflict,
        IisHandoffState::MissingRoute,
        IisHandoffState::Unavailable,
        IisHandoffState::Busy,
      ],
      &[
        "available",
        "handedOff",
        "unsupported",
        "conflict",
        "missingRoute",
        "unavailable",
        "busy",
      ],
    );
    roundtrip(
      &[
        IisIssueKind::IisUnavailable,
        IisIssueKind::HandoffUnavailable,
        IisIssueKind::InsufficientPrivileges,
        IisIssueKind::ElevationRequired,
        IisIssueKind::ElevationDenied,
        IisIssueKind::ElevationUnsupported,
        IisIssueKind::UnsupportedBindingShape,
        IisIssueKind::MissingTlsCertificate,
        IisIssueKind::Conflict,
        IisIssueKind::MissingBinding,
        IisIssueKind::MissingRoute,
        IisIssueKind::RollbackSucceeded,
        IisIssueKind::RollbackFailed,
        IisIssueKind::RestoreFailed,
        IisIssueKind::Busy,
        IisIssueKind::ProviderError,
      ],
      &[
        "iisUnavailable",
        "insufficientPrivileges",
        "elevationRequired",
        "elevationDenied",
        "elevationUnsupported",
        "unsupportedBindingShape",
        "missingTlsCertificate",
        "conflict",
        "missingBinding",
        "missingRoute",
        "rollbackSucceeded",
        "rollbackFailed",
        "restoreFailed",
        "busy",
        "providerError",
      ],
    );
    roundtrip(
      &[
        IisPrivilegeLevel::User,
        IisPrivilegeLevel::Administrator,
        IisPrivilegeLevel::Unsupported,
      ],
      &["user", "administrator", "unsupported"],
    );
    roundtrip(
      &[
        IisOperationStepStatus::Pending,
        IisOperationStepStatus::Succeeded,
        IisOperationStepStatus::RequiresElevation,
        IisOperationStepStatus::Approved,
        IisOperationStepStatus::Denied,
        IisOperationStepStatus::Failed,
        IisOperationStepStatus::Skipped,
        IisOperationStepStatus::Unsupported,
      ],
      &[
        "pending",
        "succeeded",
        "requiresElevation",
        "approved",
        "denied",
        "failed",
        "skipped",
        "unsupported",
      ],
    );
    roundtrip(
      &[
        IisElevationApproval::NotRequired,
        IisElevationApproval::Required,
        IisElevationApproval::Approved,
        IisElevationApproval::Denied,
        IisElevationApproval::Unsupported,
      ],
      &[
        "notRequired",
        "required",
        "approved",
        "denied",
        "unsupported",
      ],
    );
    roundtrip(
      &[
        IisFollowUpAction::RetryElevation,
        IisFollowUpAction::RollbackHandoff,
        IisFollowUpAction::RemoveLoopbackBinding,
        IisFollowUpAction::RetryRestore,
        IisFollowUpAction::ClearRestoreMetadata,
      ],
      &[
        "retryElevation",
        "rollbackHandoff",
        "removeLoopbackBinding",
        "retryRestore",
        "clearRestoreMetadata",
      ],
    );
  }

  #[test]
  fn message_type_constants_and_identity_constructors_are_stable() {
    let message_types = [
      message_types::REGISTER_ENTRYPOINT_REQUEST,
      message_types::REGISTER_ENTRYPOINT_RESPONSE,
      message_types::UNREGISTER_ENTRYPOINT_REQUEST,
      message_types::UNREGISTER_ENTRYPOINT_RESPONSE,
      message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
      message_types::HEARTBEAT_ENTRYPOINT_RESPONSE,
      message_types::QUERY_STATE_REQUEST,
      message_types::QUERY_STATE_RESPONSE,
      message_types::SUBSCRIBE_STATE_REQUEST,
      message_types::STATE_CHANGED_EVENT,
      message_types::STATE_STREAM_HEARTBEAT,
      message_types::STATE_STREAM_GAP,
      message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
      message_types::SET_ENTRYPOINT_ENABLED_RESPONSE,
      message_types::SET_DOMAIN_ENABLED_REQUEST,
      message_types::SET_DOMAIN_ENABLED_RESPONSE,
      message_types::QUERY_IIS_BINDINGS_REQUEST,
      message_types::QUERY_IIS_BINDINGS_RESPONSE,
      message_types::SET_IIS_HANDOFF_REQUEST,
      message_types::SET_IIS_HANDOFF_RESPONSE,
      message_types::QUERY_LOGS_REQUEST,
      message_types::QUERY_LOGS_RESPONSE,
      message_types::QUERY_HISTORY_REQUEST,
      message_types::QUERY_HISTORY_RESPONSE,
      message_types::QUERY_AUTOSTART_REQUEST,
      message_types::QUERY_AUTOSTART_RESPONSE,
      message_types::SET_AUTOSTART_REQUEST,
      message_types::SET_AUTOSTART_RESPONSE,
      message_types::SHUTDOWN_DAEMON_REQUEST,
      message_types::SHUTDOWN_DAEMON_RESPONSE,
      message_types::PROTOCOL_ERROR_RESPONSE,
    ];
    let unique = message_types
      .iter()
      .copied()
      .collect::<std::collections::BTreeSet<_>>();
    let source = SourcePath::new("Caddyfile", Some("/workspace/Caddyfile".to_string()));
    let domain = DomainName::parse(" App.Localhost. ");
    let runtime_stream = LogStreamIdentity::runtime_control();
    let entrypoint_stream = LogStreamIdentity::entrypoint("shim-1");
    let domain_stream = LogStreamIdentity::domain(&domain.canonical);
    let registered = RegisteredDomain::active("API.Localhost.");

    assert_eq!(message_types.len(), unique.len());
    assert_eq!(
      message_types::SHUTDOWN_DAEMON_RESPONSE,
      "shutdown-daemon-response"
    );
    assert_eq!(source.raw, "Caddyfile");
    assert_eq!(source.canonical.as_deref(), Some("/workspace/Caddyfile"));
    assert_eq!(domain.raw, " App.Localhost. ");
    assert_eq!(domain.canonical, "app.localhost");
    assert_eq!(runtime_stream.stream_id, "runtime-control");
    assert_eq!(entrypoint_stream.stream_id, "entrypoint-shim-1");
    assert_eq!(domain_stream.domain_key.as_deref(), Some("app.localhost"));
    assert_eq!(registered.name.canonical, "api.localhost");
    assert_eq!(
      registered.log_stream.domain_key.as_deref(),
      Some("api.localhost")
    );
  }

  #[test]
  fn public_dtos_keep_debug_clone_and_json_contracts() {
    let now = Utc::now();
    let identity = EntrypointInstanceIdentity::new(now);
    let registration = EntrypointRegistration {
      registration_id: identity.instance_id.clone(),
      entrypoint_instance: identity.clone(),
      source_working_directory: SourcePath::new(".", Some("/workspace".to_string())),
      source_config_path: SourcePath::new("Caddyfile", Some("/workspace/Caddyfile".to_string())),
      registered_domains: vec![RegisteredDomain::active("app.localhost")],
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 42,
        process_start_time_utc: now,
        shim_session_nonce: identity.shim_session_nonce.clone(),
        executable_path: Some("caddy".to_string()),
      },
      log_stream: LogStreamIdentity::entrypoint(&identity.instance_id),
      shim_run: Some(ShimRunMetadata {
        adapter: Some("caddyfile".to_string()),
        raw_arguments: vec![
          "run".to_string(),
          "--config".to_string(),
          "Caddyfile".to_string(),
        ],
        command_line: "caddy run --config Caddyfile".to_string(),
      }),
      created_at_utc: now,
      last_heartbeat_utc: now,
    };
    let runtime_state = RuntimeState {
      status: RuntimeStatus::Running,
      binary_path: Some("caddy".to_string()),
      version: Some("2.10.0".to_string()),
      process_id: Some(1234),
      admin_endpoint: Some("localhost:2019".to_string()),
      diagnostics: vec![RuntimeDiagnostic {
        code: "runtime-ok".to_string(),
        message: "Runtime is running.".to_string(),
        operation: Some("inspect".to_string()),
      }],
    };
    let config_state = ConfigState {
      status: ConfigApplyStatus::Applied,
      last_attempted_at_utc: Some(now),
      last_successful_reload_at_utc: Some(now),
      effective_config_hash: Some("hash".to_string()),
      diagnostics: vec![ConfigDiagnostic {
        code: "config-ok".to_string(),
        message: "Config applied.".to_string(),
        domain_key: Some("app.localhost".to_string()),
        source_config_paths: vec!["Caddyfile".to_string()],
      }],
    };
    let logs = QueryLogsResponse {
      request_id: "logs-1".to_string(),
      accepted: true,
      message: "Logs returned.".to_string(),
      stream: LogStreamIdentity::runtime_control(),
      stream_status: LogStreamStatus::Active,
      entries: vec![LogEntry {
        sequence_number: 1,
        timestamp_utc: now,
        severity: LogSeverity::Info,
        domain_key: None,
        stream: LogStreamIdentity::runtime_control(),
        attribution_kind: LogAttributionKind::RuntimeControl,
        entry_kind: LogEntryKind::Lifecycle,
        raw_message: "started".to_string(),
        source_registration_id: Some(registration.registration_id.clone()),
        source_instance_id: Some(identity.instance_id.clone()),
        operation: Some("start".to_string()),
      }],
      next_cursor: Some("1".to_string()),
      has_gap: false,
      has_more_before: false,
      truncated_by_retention: false,
    };
    let step = IisOperationStep::administrator("iis-restore", "Restore IIS binding");
    let response = RegisterEntrypointResponse {
      request_id: "register-1".to_string(),
      accepted: true,
      message: "Registered.".to_string(),
      registration_id: Some(registration.registration_id.clone()),
    };
    let snapshot = GuiStateSnapshot {
      captured_at_utc: now,
      registrations: vec![registration.clone()],
      runtime: runtime_state.clone(),
      config: config_state.clone(),
      storage: Some(StorageState {
        backend: "files".to_string(),
        path: None,
        schema_version: 1,
        diagnostics: Vec::new(),
      }),
    };
    let values = serde_json::json!({
      "registerRequest": RegisterEntrypointRequest { request_id: "register-1".to_string(), registration: registration.clone() },
      "registerResponse": response.clone(),
      "unregister": UnregisterEntrypointRequest { request_id: "unregister-1".to_string(), registration_id: registration.registration_id.clone(), shim_session_nonce: identity.shim_session_nonce.clone() },
      "logs": logs.clone(),
      "snapshot": snapshot.clone(),
      "step": step.clone(),
      "basic": BasicResponse { request_id: "basic-1".to_string(), accepted: true, message: "ok".to_string() },
    });
    let rendered = serde_json::to_string(&values).unwrap();

    assert!(format!("{:?}", registration.clone()).contains("EntrypointRegistration"));
    assert!(format!("{:?}", runtime_state.clone()).contains("RuntimeState"));
    assert!(format!("{:?}", config_state.clone()).contains("ConfigState"));
    assert!(format!("{:?}", logs.clone()).contains("QueryLogsResponse"));
    assert!(format!("{:?}", step.clone()).contains("IisOperationStep"));
    assert!(rendered.contains("\"requiresElevation\""));
    assert_eq!(snapshot.registrations[0], registration);
    assert_eq!(
      response.registration_id.as_deref(),
      Some(identity.instance_id.as_str())
    );
  }

  #[test]
  fn serializes_iis_handoff_contracts() {
    let response = QueryIisBindingsResponse {
      request_id: "iis-1".to_string(),
      accepted: true,
      message: "ok".to_string(),
      bindings: vec![IisBinding {
        identity: IisBindingIdentity {
          binding_id: "Default Web Site|http|*:80:app.localhost".to_string(),
          site_name: "Default Web Site".to_string(),
          protocol: "http".to_string(),
          binding_information: "*:80:app.localhost".to_string(),
        },
        ip_address: "*".to_string(),
        port: 80,
        host_header: "app.localhost".to_string(),
        domain_key: Some("app.localhost".to_string()),
        handoff_state: IisHandoffState::Available,
        issue: Some(IisIssue::new(
          IisIssueKind::MissingRoute,
          "Cadder has no route.",
        )),
        restore_metadata: None,
      }],
      issue: None,
    };

    let envelope = IpcEnvelope::new(message_types::QUERY_IIS_BINDINGS_RESPONSE, &response).unwrap();
    let json = serde_json::to_string(&envelope).unwrap();
    let decoded: QueryIisBindingsResponse = envelope.decode().unwrap();

    assert!(json.contains("\"query-iis-bindings-response\""));
    assert!(json.contains("\"handoffState\":\"available\""));
    assert!(json.contains("\"missingRoute\""));
    assert_eq!(decoded, response);

    let handoff_response = SetIisHandoffResponse {
      request_id: "iis-on".to_string(),
      accepted: false,
      message: "Administrator approval was denied.".to_string(),
      binding: None,
      issue: Some(IisIssue::new(
        IisIssueKind::ElevationDenied,
        "Administrator approval was denied.",
      )),
      steps: vec![IisOperationStep {
        step_id: "iis-remove-public-binding".to_string(),
        label: "Remove IIS public binding.".to_string(),
        privilege_level: IisPrivilegeLevel::Administrator,
        status: IisOperationStepStatus::Denied,
        approval: IisElevationApproval::Denied,
        issue: None,
      }],
      follow_up_actions: vec![IisFollowUpAction::RetryElevation],
    };
    let envelope =
      IpcEnvelope::new(message_types::SET_IIS_HANDOFF_RESPONSE, &handoff_response).unwrap();
    let json = serde_json::to_string(&envelope).unwrap();
    let decoded: SetIisHandoffResponse = envelope.decode().unwrap();

    assert!(json.contains("\"privilegeLevel\":\"administrator\""));
    assert!(json.contains("\"approval\":\"denied\""));
    assert!(json.contains("\"retryElevation\""));
    assert_eq!(decoded, handoff_response);

    let missing_tls = IisIssue::new(
      IisIssueKind::MissingTlsCertificate,
      "Missing certificate metadata.",
    );
    let json = serde_json::to_string(&missing_tls).unwrap();
    assert!(json.contains("\"kind\":\"missingTlsCertificate\""));

    let request = SetIisHandoffRequest {
      request_id: "iis-on".to_string(),
      binding_id: "Default Web Site|https|*:443:".to_string(),
      enabled: true,
      route_host: Some("iis-app.localhost".to_string()),
    };
    let envelope = IpcEnvelope::new(message_types::SET_IIS_HANDOFF_REQUEST, &request).unwrap();
    let json = serde_json::to_string(&envelope).unwrap();
    let decoded: SetIisHandoffRequest = envelope.decode().unwrap();

    assert!(json.contains("\"set-iis-handoff-request\""));
    assert!(json.contains("\"routeHost\":\"iis-app.localhost\""));
    assert_eq!(decoded, request);
  }

  #[test]
  fn validates_owner_session_nonce() {
    let now = Utc::now();
    let identity = EntrypointInstanceIdentity::new(now);
    let registration = EntrypointRegistration {
      registration_id: identity.instance_id.clone(),
      source_working_directory: SourcePath::new(".", Some("/tmp/project".to_string())),
      source_config_path: SourcePath::new("Caddyfile", Some("/tmp/project/Caddyfile".to_string())),
      registered_domains: vec![RegisteredDomain::active("app.localhost")],
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 42,
        process_start_time_utc: now,
        shim_session_nonce: identity.shim_session_nonce.clone(),
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint(&identity.instance_id),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
      entrypoint_instance: identity,
    };

    assert_eq!(registration.validate_owner(), Ok(()));
  }

  #[test]
  fn helper_constructors_and_validation_errors_stay_stable() {
    assert!(ActivationState::from_enabled(true).is_enabled());
    assert_eq!(
      ActivationState::from_enabled(false),
      ActivationState::Inactive
    );
    assert_eq!(RuntimeState::idle().status, RuntimeStatus::Idle);
    assert_eq!(ConfigState::idle().status, ConfigApplyStatus::Idle);

    let user_step = IisOperationStep::user("discover", "Discover IIS bindings");
    assert_eq!(user_step.privilege_level, IisPrivilegeLevel::User);
    assert_eq!(user_step.approval, IisElevationApproval::NotRequired);
    let admin_step = IisOperationStep::administrator("restore", "Restore IIS binding");
    assert_eq!(admin_step.privilege_level, IisPrivilegeLevel::Administrator);
    assert_eq!(admin_step.status, IisOperationStepStatus::RequiresElevation);

    let now = Utc::now();
    let identity = EntrypointInstanceIdentity::new(now);
    let mut registration = EntrypointRegistration {
      registration_id: String::new(),
      entrypoint_instance: identity.clone(),
      source_working_directory: SourcePath::new(".", Some("/tmp/project".to_string())),
      source_config_path: SourcePath::new("Caddyfile", Some("/tmp/project/Caddyfile".to_string())),
      registered_domains: Vec::new(),
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: 42,
        process_start_time_utc: now,
        shim_session_nonce: identity.shim_session_nonce.clone(),
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint(&identity.instance_id),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
    };
    assert_eq!(
      registration.validate_owner().unwrap_err(),
      "registration_id is required"
    );

    registration.registration_id = "other".to_string();
    assert_eq!(
      registration.validate_owner().unwrap_err(),
      "registration_id must match instance_id"
    );

    registration.registration_id = registration.entrypoint_instance.instance_id.clone();
    registration.owner_process.shim_session_nonce = "wrong".to_string();
    assert_eq!(
      registration.validate_owner().unwrap_err(),
      "entrypoint and owner shim session nonce values must match"
    );

    let request_id = new_request_id("unit");
    assert!(request_id.starts_with("unit-"));
  }

  #[test]
  fn serializes_state_and_control_contracts() {
    let now = Utc::now();
    let identity = EntrypointInstanceIdentity::new(now);
    let registration = EntrypointRegistration {
      registration_id: identity.instance_id.clone(),
      entrypoint_instance: identity.clone(),
      source_working_directory: SourcePath::new(".", Some("/workspace".to_string())),
      source_config_path: SourcePath::new("Caddyfile", Some("/workspace/Caddyfile".to_string())),
      registered_domains: vec![RegisteredDomain::active("App.Localhost.")],
      activation_state: ActivationState::Registered,
      owner_process: OwnerProcessIdentity {
        process_id: 42,
        process_start_time_utc: now,
        shim_session_nonce: identity.shim_session_nonce.clone(),
        executable_path: Some("caddy".to_string()),
      },
      log_stream: LogStreamIdentity::entrypoint(&identity.instance_id),
      shim_run: Some(ShimRunMetadata {
        adapter: Some("caddyfile".to_string()),
        raw_arguments: vec!["run".to_string()],
        command_line: "caddy run".to_string(),
      }),
      created_at_utc: now,
      last_heartbeat_utc: now,
    };
    let snapshot = GuiStateSnapshot {
      captured_at_utc: now,
      registrations: vec![registration.clone()],
      runtime: RuntimeState {
        status: RuntimeStatus::Running,
        binary_path: Some("caddy".to_string()),
        version: Some("2.10.0".to_string()),
        process_id: Some(1234),
        admin_endpoint: Some("localhost:2019".to_string()),
        diagnostics: vec![RuntimeDiagnostic {
          code: "runtime-ok".to_string(),
          message: "Runtime is running.".to_string(),
          operation: Some("inspect".to_string()),
        }],
      },
      config: ConfigState {
        status: ConfigApplyStatus::Applied,
        last_attempted_at_utc: Some(now),
        last_successful_reload_at_utc: Some(now),
        effective_config_hash: Some("hash".to_string()),
        diagnostics: vec![ConfigDiagnostic {
          code: "config-ok".to_string(),
          message: "Config applied.".to_string(),
          domain_key: Some("app.localhost".to_string()),
          source_config_paths: vec!["Caddyfile".to_string()],
        }],
      },
      storage: Some(StorageState {
        backend: "files".to_string(),
        path: None,
        schema_version: 1,
        diagnostics: vec![RuntimeDiagnostic {
          code: "storage-ok".to_string(),
          message: "Storage is ready.".to_string(),
          operation: Some("open".to_string()),
        }],
      }),
    };
    let state_response = QueryStateResponse {
      request_id: "state-1".to_string(),
      accepted: true,
      message: "State returned.".to_string(),
      snapshot: Some(snapshot.clone()),
    };
    let state_event = StateChangedEvent {
      request_id: "watch-1".to_string(),
      sequence_number: 7,
      change_kind: StateChangeKind::RuntimeChanged,
      snapshot,
      registration_id: Some(identity.instance_id.clone()),
    };
    let stream_heartbeat = StateStreamHeartbeat {
      request_id: "watch-1".to_string(),
      last_sequence_number: 7,
    };
    let stream_gap = StateStreamGap {
      request_id: "watch-1".to_string(),
      first_missing_sequence_number: 8,
      last_missing_sequence_number: 11,
    };
    let requests = serde_json::json!([
      QueryStateRequest {
        request_id: "state".to_string()
      },
      SubscribeStateRequest {
        request_id: "sub".to_string()
      },
      HeartbeatEntrypointRequest {
        request_id: "heartbeat".to_string(),
        registration_id: identity.instance_id.clone(),
        shim_session_nonce: identity.shim_session_nonce.clone(),
      },
      SetEntrypointEnabledRequest {
        request_id: "entrypoint-enable".to_string(),
        registration_id: identity.instance_id.clone(),
        shim_session_nonce: Some(identity.shim_session_nonce.clone()),
        enabled: false,
      },
      SetDomainEnabledRequest {
        request_id: "domain-enable".to_string(),
        registration_id: identity.instance_id.clone(),
        domain_key: "app.localhost".to_string(),
        enabled: true,
      },
      QueryIisBindingsRequest {
        request_id: "iis-bindings".to_string()
      },
      QueryLogsRequest {
        request_id: "logs".to_string(),
        stream: LogStreamIdentity::domain("app.localhost"),
        limit: Some(50),
        cursor: Some("10".to_string()),
        minimum_severity: Some(LogSeverity::Warn),
      },
      QueryAutostartRequest {
        request_id: "autostart".to_string()
      },
      SetAutostartRequest {
        request_id: "set-autostart".to_string(),
        mode: AutostartMode::Daemon,
      },
      ShutdownDaemonRequest {
        request_id: "shutdown".to_string()
      },
    ]);

    let envelope = IpcEnvelope::new(message_types::QUERY_STATE_RESPONSE, &state_response).unwrap();
    let decoded: QueryStateResponse = envelope.decode().unwrap();
    let autostart_response = QueryAutostartResponse {
      request_id: "autostart".to_string(),
      accepted: false,
      message: "Autostart is unavailable.".to_string(),
      mode: AutostartMode::Daemon,
      status: AutostartStatus::Unsupported,
      target: Some("cadderd --runtime-dir runtime".to_string()),
      diagnostics: vec![AutostartDiagnostic {
        code: "unsupported".to_string(),
        message: "OS autostart is unsupported.".to_string(),
      }],
    };
    let autostart_envelope =
      IpcEnvelope::new(message_types::QUERY_AUTOSTART_RESPONSE, &autostart_response).unwrap();
    let decoded_autostart: QueryAutostartResponse = autostart_envelope.decode().unwrap();
    let event_json = serde_json::to_string(&state_event).unwrap();
    let heartbeat_json = serde_json::to_string(&stream_heartbeat).unwrap();
    let gap_json = serde_json::to_string(&stream_gap).unwrap();
    let request_json = serde_json::to_string(&requests).unwrap();
    let variants = serde_json::json!({
      "runtime": [RuntimeStatus::Unknown, RuntimeStatus::NotResolved, RuntimeStatus::Resolved, RuntimeStatus::Running, RuntimeStatus::Unhealthy, RuntimeStatus::Idle],
      "config": [ConfigApplyStatus::Unknown, ConfigApplyStatus::NotApplied, ConfigApplyStatus::Applied, ConfigApplyStatus::Failed, ConfigApplyStatus::Idle],
      "history": [HistoryKind::Registration, HistoryKind::Runtime, HistoryKind::Config, HistoryKind::Iis, HistoryKind::Autostart, HistoryKind::Log],
      "autostart": [AutostartStatus::Unknown, AutostartStatus::Disabled, AutostartStatus::Enabled, AutostartStatus::Unsupported, AutostartStatus::Misconfigured],
      "iis": [IisHandoffState::Available, IisHandoffState::HandedOff, IisHandoffState::Unsupported, IisHandoffState::Conflict, IisHandoffState::MissingRoute, IisHandoffState::Unavailable, IisHandoffState::Busy],
      "steps": [IisOperationStepStatus::Pending, IisOperationStepStatus::Succeeded, IisOperationStepStatus::RequiresElevation, IisOperationStepStatus::Approved, IisOperationStepStatus::Denied, IisOperationStepStatus::Failed, IisOperationStepStatus::Skipped, IisOperationStepStatus::Unsupported],
      "approval": [IisElevationApproval::NotRequired, IisElevationApproval::Required, IisElevationApproval::Approved, IisElevationApproval::Denied, IisElevationApproval::Unsupported],
      "followUp": [IisFollowUpAction::RetryElevation, IisFollowUpAction::RollbackHandoff, IisFollowUpAction::RetryRestore, IisFollowUpAction::RemoveLoopbackBinding, IisFollowUpAction::ClearRestoreMetadata],
    });

    assert_eq!(decoded, state_response);
    assert_eq!(decoded_autostart, autostart_response);
    assert!(event_json.contains("\"runtimeChanged\""));
    assert!(heartbeat_json.contains("\"lastSequenceNumber\":7"));
    assert!(gap_json.contains("\"firstMissingSequenceNumber\":8"));
    assert!(gap_json.contains("\"lastMissingSequenceNumber\":11"));
    assert!(request_json.contains("\"minimumSeverity\":\"warn\""));
    assert!(request_json.contains("\"set-autostart\""));
    assert!(variants.to_string().contains("misconfigured"));
  }

  #[test]
  fn serializes_history_and_autostart_contracts() {
    let now = Utc::now();
    let history = QueryHistoryResponse {
      request_id: "history-1".to_string(),
      accepted: true,
      message: "History returned.".to_string(),
      records: vec![HistoryRecord {
        sequence_number: 1,
        timestamp_utc: now,
        kind: HistoryKind::Runtime,
        summary: "Runtime started.".to_string(),
        registration_id: Some("shim-1".to_string()),
        domain_key: Some("app.localhost".to_string()),
        payload: serde_json::json!({ "status": "running" }),
      }],
      storage: Some(StorageState {
        backend: "files".to_string(),
        path: None,
        schema_version: 1,
        diagnostics: Vec::new(),
      }),
    };
    let envelope = IpcEnvelope::new(message_types::QUERY_HISTORY_RESPONSE, &history).unwrap();
    let json = serde_json::to_string(&envelope).unwrap();
    let decoded: QueryHistoryResponse = envelope.decode().unwrap();

    assert!(json.contains("\"runtime\""));
    assert_eq!(decoded, history);

    let autostart = SetAutostartResponse {
      request_id: "autostart-1".to_string(),
      accepted: true,
      message: "Autostart mode updated.".to_string(),
      mode: AutostartMode::Daemon,
      status: AutostartStatus::Enabled,
      target: Some("cadderd".to_string()),
      diagnostics: Vec::new(),
    };
    let envelope = IpcEnvelope::new(message_types::SET_AUTOSTART_RESPONSE, &autostart).unwrap();
    let json = serde_json::to_string(&envelope).unwrap();
    let decoded: SetAutostartResponse = envelope.decode().unwrap();

    assert!(json.contains("\"daemon\""));
    assert_eq!(decoded, autostart);
  }
}
