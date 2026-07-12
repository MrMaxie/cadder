use crate::{
  BasicResponse, HeartbeatEntrypointRequest, IdentifierError, QueryAutostartRequest,
  QueryAutostartResponse, QueryHistoryRequest, QueryHistoryResponse, QueryIisBindingsRequest,
  QueryIisBindingsResponse, QueryLogsRequest, QueryLogsResponse, QueryStateRequest,
  QueryStateResponse, RegisterEntrypointRequest, RegisterEntrypointResponse, RequestId,
  SetAutostartRequest, SetAutostartResponse, SetDomainEnabledRequest, SetEntrypointEnabledRequest,
  SetIisHandoffRequest, SetIisHandoffResponse, ShutdownDaemonRequest, StateChangedEvent,
  SubscribeStateRequest, UnregisterEntrypointRequest,
};
use serde::{Serialize, de::DeserializeOwned};

mod sealed {
  pub trait Sealed {}
}

/// A transitional flat-envelope request that exposes validated correlation before transport work
/// starts.
///
/// The trait is sealed because the legacy adapter accepts only protocol-owned request DTOs. This
/// lets connection, framing, EOF, and timeout failures retain the original [`RequestId`] even when
/// no daemon response arrives. The versioned client consumes [`crate::RequestEnvelope`] directly
/// and does not implement this transitional trait.
pub trait LegacyCorrelatedRequest: sealed::Sealed + Serialize {
  type Response: DeserializeOwned;

  const OPERATION: &'static str;
  const RESPONSE: &'static str;

  fn correlation_id(&self) -> Result<RequestId, IdentifierError>;
}

macro_rules! correlated_legacy_requests {
  ($(($request:ty, $response_type:ty, $operation:path, $response:path)),+ $(,)?) => {
    $(
      impl sealed::Sealed for $request {}

      impl LegacyCorrelatedRequest for $request {
        type Response = $response_type;

        const OPERATION: &'static str = $operation;
        const RESPONSE: &'static str = $response;

        fn correlation_id(&self) -> Result<RequestId, IdentifierError> {
          RequestId::parse(self.request_id.clone())
        }
      }
    )+
  };
}

correlated_legacy_requests!(
  (
    RegisterEntrypointRequest,
    RegisterEntrypointResponse,
    crate::message_types::REGISTER_ENTRYPOINT_REQUEST,
    crate::message_types::REGISTER_ENTRYPOINT_RESPONSE
  ),
  (
    UnregisterEntrypointRequest,
    BasicResponse,
    crate::message_types::UNREGISTER_ENTRYPOINT_REQUEST,
    crate::message_types::UNREGISTER_ENTRYPOINT_RESPONSE
  ),
  (
    HeartbeatEntrypointRequest,
    BasicResponse,
    crate::message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
    crate::message_types::HEARTBEAT_ENTRYPOINT_RESPONSE
  ),
  (
    QueryStateRequest,
    QueryStateResponse,
    crate::message_types::QUERY_STATE_REQUEST,
    crate::message_types::QUERY_STATE_RESPONSE
  ),
  (
    SubscribeStateRequest,
    StateChangedEvent,
    crate::message_types::SUBSCRIBE_STATE_REQUEST,
    crate::message_types::STATE_CHANGED_EVENT
  ),
  (
    SetEntrypointEnabledRequest,
    BasicResponse,
    crate::message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
    crate::message_types::SET_ENTRYPOINT_ENABLED_RESPONSE
  ),
  (
    SetDomainEnabledRequest,
    BasicResponse,
    crate::message_types::SET_DOMAIN_ENABLED_REQUEST,
    crate::message_types::SET_DOMAIN_ENABLED_RESPONSE
  ),
  (
    QueryHistoryRequest,
    QueryHistoryResponse,
    crate::message_types::QUERY_HISTORY_REQUEST,
    crate::message_types::QUERY_HISTORY_RESPONSE
  ),
  (
    QueryAutostartRequest,
    QueryAutostartResponse,
    crate::message_types::QUERY_AUTOSTART_REQUEST,
    crate::message_types::QUERY_AUTOSTART_RESPONSE
  ),
  (
    SetAutostartRequest,
    SetAutostartResponse,
    crate::message_types::SET_AUTOSTART_REQUEST,
    crate::message_types::SET_AUTOSTART_RESPONSE
  ),
  (
    ShutdownDaemonRequest,
    BasicResponse,
    crate::message_types::SHUTDOWN_DAEMON_REQUEST,
    crate::message_types::SHUTDOWN_DAEMON_RESPONSE
  ),
  (
    QueryLogsRequest,
    QueryLogsResponse,
    crate::message_types::QUERY_LOGS_REQUEST,
    crate::message_types::QUERY_LOGS_RESPONSE
  ),
  (
    QueryIisBindingsRequest,
    QueryIisBindingsResponse,
    crate::message_types::QUERY_IIS_BINDINGS_REQUEST,
    crate::message_types::QUERY_IIS_BINDINGS_RESPONSE
  ),
  (
    SetIisHandoffRequest,
    SetIisHandoffResponse,
    crate::message_types::SET_IIS_HANDOFF_REQUEST,
    crate::message_types::SET_IIS_HANDOFF_RESPONSE
  ),
);

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn legacy_correlated_requests_validate_request_ids() {
    let legacy = QueryStateRequest {
      request_id: "legacy-correlation-1".to_string(),
    };
    assert_eq!(
      legacy.correlation_id().unwrap().as_str(),
      "legacy-correlation-1"
    );
    assert_eq!(
      QueryStateRequest::OPERATION,
      crate::message_types::QUERY_STATE_REQUEST
    );
    assert_eq!(
      QueryStateRequest::RESPONSE,
      crate::message_types::QUERY_STATE_RESPONSE
    );

    let invalid = QueryStateRequest {
      request_id: String::new(),
    };
    assert!(invalid.correlation_id().is_err());
  }

  #[test]
  fn legacy_request_operation_and_response_bindings_are_complete() {
    fn assert_response_type<TRequest, TResponse>()
    where
      TRequest: LegacyCorrelatedRequest<Response = TResponse>,
      TResponse: DeserializeOwned,
    {
    }

    macro_rules! assert_contract {
      ($request:ty, $response_type:ty, $operation:path, $response:path) => {
        assert_response_type::<$request, $response_type>();
        assert_eq!(<$request>::OPERATION, $operation);
        assert_eq!(<$request>::RESPONSE, $response);
      };
    }

    assert_contract!(
      RegisterEntrypointRequest,
      RegisterEntrypointResponse,
      crate::message_types::REGISTER_ENTRYPOINT_REQUEST,
      crate::message_types::REGISTER_ENTRYPOINT_RESPONSE
    );
    assert_contract!(
      UnregisterEntrypointRequest,
      BasicResponse,
      crate::message_types::UNREGISTER_ENTRYPOINT_REQUEST,
      crate::message_types::UNREGISTER_ENTRYPOINT_RESPONSE
    );
    assert_contract!(
      HeartbeatEntrypointRequest,
      BasicResponse,
      crate::message_types::HEARTBEAT_ENTRYPOINT_REQUEST,
      crate::message_types::HEARTBEAT_ENTRYPOINT_RESPONSE
    );
    assert_contract!(
      QueryStateRequest,
      QueryStateResponse,
      crate::message_types::QUERY_STATE_REQUEST,
      crate::message_types::QUERY_STATE_RESPONSE
    );
    assert_contract!(
      SubscribeStateRequest,
      StateChangedEvent,
      crate::message_types::SUBSCRIBE_STATE_REQUEST,
      crate::message_types::STATE_CHANGED_EVENT
    );
    assert_contract!(
      SetEntrypointEnabledRequest,
      BasicResponse,
      crate::message_types::SET_ENTRYPOINT_ENABLED_REQUEST,
      crate::message_types::SET_ENTRYPOINT_ENABLED_RESPONSE
    );
    assert_contract!(
      SetDomainEnabledRequest,
      BasicResponse,
      crate::message_types::SET_DOMAIN_ENABLED_REQUEST,
      crate::message_types::SET_DOMAIN_ENABLED_RESPONSE
    );
    assert_contract!(
      QueryHistoryRequest,
      QueryHistoryResponse,
      crate::message_types::QUERY_HISTORY_REQUEST,
      crate::message_types::QUERY_HISTORY_RESPONSE
    );
    assert_contract!(
      QueryAutostartRequest,
      QueryAutostartResponse,
      crate::message_types::QUERY_AUTOSTART_REQUEST,
      crate::message_types::QUERY_AUTOSTART_RESPONSE
    );
    assert_contract!(
      SetAutostartRequest,
      SetAutostartResponse,
      crate::message_types::SET_AUTOSTART_REQUEST,
      crate::message_types::SET_AUTOSTART_RESPONSE
    );
    assert_contract!(
      ShutdownDaemonRequest,
      BasicResponse,
      crate::message_types::SHUTDOWN_DAEMON_REQUEST,
      crate::message_types::SHUTDOWN_DAEMON_RESPONSE
    );
    assert_contract!(
      QueryLogsRequest,
      QueryLogsResponse,
      crate::message_types::QUERY_LOGS_REQUEST,
      crate::message_types::QUERY_LOGS_RESPONSE
    );
    assert_contract!(
      QueryIisBindingsRequest,
      QueryIisBindingsResponse,
      crate::message_types::QUERY_IIS_BINDINGS_REQUEST,
      crate::message_types::QUERY_IIS_BINDINGS_RESPONSE
    );
    assert_contract!(
      SetIisHandoffRequest,
      SetIisHandoffResponse,
      crate::message_types::SET_IIS_HANDOFF_REQUEST,
      crate::message_types::SET_IIS_HANDOFF_RESPONSE
    );
  }
}
