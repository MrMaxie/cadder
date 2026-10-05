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
  SET_ENTRYPOINT_ENABLED_REQUEST = "set-entrypoint-enabled-request";
  SET_DOMAIN_ENABLED_REQUEST = "set-domain-enabled-request";
  QUERY_LOGS_REQUEST = "query-logs-request";
  SHUTDOWN_DAEMON_REQUEST = "shutdown-daemon-request";
}

pub const REGISTER_ENTRYPOINT_RESPONSE: &str = "register-entrypoint-response";
pub const UNREGISTER_ENTRYPOINT_RESPONSE: &str = "unregister-entrypoint-response";
pub const HEARTBEAT_ENTRYPOINT_RESPONSE: &str = "heartbeat-entrypoint-response";
pub const QUERY_STATE_RESPONSE: &str = "query-state-response";
pub const SET_ENTRYPOINT_ENABLED_RESPONSE: &str = "set-entrypoint-enabled-response";
pub const SET_DOMAIN_ENABLED_RESPONSE: &str = "set-domain-enabled-response";
pub const QUERY_LOGS_RESPONSE: &str = "query-logs-response";
pub const SHUTDOWN_DAEMON_RESPONSE: &str = "shutdown-daemon-response";
pub const PROTOCOL_ERROR_RESPONSE: &str = "protocol-error-response";
