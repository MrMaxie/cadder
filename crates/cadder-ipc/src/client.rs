use crate::{
  BasicResponse, HeartbeatEntrypointPayload, QueryLogsPayload, QueryLogsResponse,
  QueryStatePayload, QueryStateResponse, RegisterEntrypointPayload, RegisterEntrypointResponse,
  SetDomainEnabledPayload, SetEntrypointEnabledPayload, ShutdownDaemonPayload,
  UnregisterEntrypointPayload, message_types,
};
use serde::{Serialize, de::DeserializeOwned};

mod sealed {
  pub trait Sealed {}
}

/// A protocol-owned operation payload with one statically known response type.
pub trait CorrelatedRequest: sealed::Sealed + Serialize + Clone + crate::OperationPayload {
  type Response: DeserializeOwned;

  const RESPONSE: &'static str;
}

macro_rules! correlated_requests {
  ($(($request:ty, $response_type:ty, $response:path)),+ $(,)?) => {
    $(
      impl sealed::Sealed for $request {}

      impl CorrelatedRequest for $request {
        type Response = $response_type;
        const RESPONSE: &'static str = $response;
      }
    )+
  };
}

correlated_requests!(
  (
    RegisterEntrypointPayload,
    RegisterEntrypointResponse,
    message_types::REGISTER_ENTRYPOINT_RESPONSE
  ),
  (
    UnregisterEntrypointPayload,
    BasicResponse,
    message_types::UNREGISTER_ENTRYPOINT_RESPONSE
  ),
  (
    HeartbeatEntrypointPayload,
    BasicResponse,
    message_types::HEARTBEAT_ENTRYPOINT_RESPONSE
  ),
  (
    QueryStatePayload,
    QueryStateResponse,
    message_types::QUERY_STATE_RESPONSE
  ),
  (
    SetEntrypointEnabledPayload,
    BasicResponse,
    message_types::SET_ENTRYPOINT_ENABLED_RESPONSE
  ),
  (
    SetDomainEnabledPayload,
    BasicResponse,
    message_types::SET_DOMAIN_ENABLED_RESPONSE
  ),
  (
    ShutdownDaemonPayload,
    BasicResponse,
    message_types::SHUTDOWN_DAEMON_RESPONSE
  ),
  (
    QueryLogsPayload,
    QueryLogsResponse,
    message_types::QUERY_LOGS_RESPONSE
  ),
);
