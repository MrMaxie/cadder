macro_rules! request_message_types {
  ($($name:ident = $value:literal;)+) => {
    $(pub const $name: &str = $value;)+

    /// Every request operation that must have one registry definition and one dispatcher path.
    pub const REQUEST_MESSAGE_TYPES: &[&str] = &[$($name),+];
  };
}

request_message_types! {
  REGISTER_ENTRYPOINT_REQUEST = "register-entrypoint-request";
  UNREGISTER_ENTRYPOINT_REQUEST = "unregister-entrypoint-request";
  HEARTBEAT_ENTRYPOINT_REQUEST = "heartbeat-entrypoint-request";
  QUERY_STATE_REQUEST = "query-state-request";
  SUBSCRIBE_STATE_REQUEST = "subscribe-state-request";
  SET_ENTRYPOINT_ENABLED_REQUEST = "set-entrypoint-enabled-request";
  SET_DOMAIN_ENABLED_REQUEST = "set-domain-enabled-request";
  QUERY_IIS_BINDINGS_REQUEST = "query-iis-bindings-request";
  SET_IIS_HANDOFF_REQUEST = "set-iis-handoff-request";
  QUERY_LOGS_REQUEST = "query-logs-request";
  QUERY_HISTORY_REQUEST = "query-history-request";
  QUERY_AUTOSTART_REQUEST = "query-autostart-request";
  SET_AUTOSTART_REQUEST = "set-autostart-request";
  SHUTDOWN_DAEMON_REQUEST = "shutdown-daemon-request";
}

pub const REGISTER_ENTRYPOINT_RESPONSE: &str = "register-entrypoint-response";
pub const UNREGISTER_ENTRYPOINT_RESPONSE: &str = "unregister-entrypoint-response";
pub const HEARTBEAT_ENTRYPOINT_RESPONSE: &str = "heartbeat-entrypoint-response";
pub const QUERY_STATE_RESPONSE: &str = "query-state-response";
pub const STATE_CHANGED_EVENT: &str = "state-changed-event";
pub const SET_ENTRYPOINT_ENABLED_RESPONSE: &str = "set-entrypoint-enabled-response";
pub const SET_DOMAIN_ENABLED_RESPONSE: &str = "set-domain-enabled-response";
pub const QUERY_IIS_BINDINGS_RESPONSE: &str = "query-iis-bindings-response";
pub const SET_IIS_HANDOFF_RESPONSE: &str = "set-iis-handoff-response";
pub const QUERY_LOGS_RESPONSE: &str = "query-logs-response";
pub const QUERY_HISTORY_RESPONSE: &str = "query-history-response";
pub const QUERY_AUTOSTART_RESPONSE: &str = "query-autostart-response";
pub const SET_AUTOSTART_RESPONSE: &str = "set-autostart-response";
pub const SHUTDOWN_DAEMON_RESPONSE: &str = "shutdown-daemon-response";
pub const PROTOCOL_ERROR_RESPONSE: &str = "protocol-error-response";
