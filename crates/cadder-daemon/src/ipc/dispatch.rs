use super::*;

pub(super) async fn dispatch_authorized_request<W>(
  writer: &mut W,
  state: &DaemonState,
  owned: &ConnectionOwnership,
  authorized: &AuthorizedRequestEnvelope<'_>,
  dispatch: RequestDispatchContext<'_>,
) -> Result<ConnectionAction>
where
  W: AsyncWrite + Unpin,
{
  let RequestDispatchContext {
    operation_fence,
    deadline,
    limits,
  } = dispatch;
  macro_rules! send_response {
    ($message_type:expr, $response:expr) => {
      let mut response = $response;
      response.request_id = authorized.request_id().to_string();
      write_envelope_until(
        writer,
        $message_type,
        &response,
        deadline,
        limits.write_no_progress,
      )
      .await?;
    };
  }
  macro_rules! decode_request {
    ($request:ty) => {
      match decode_or_reject_until::<$request, _>(writer, authorized, deadline, limits).await? {
        Some(request) => request,
        None => return Ok(ConnectionAction::Continue),
      }
    };
  }

  #[cfg(test)]
  sleep(limits.dispatch_delay).await;

  match authorized.definition().name() {
    message_types::REGISTER_ENTRYPOINT_REQUEST => {
      let request = decode_request!(RegisterEntrypointPayload);
      let fence = mutation_fence(operation_fence)?;
      let nonce = request
        .registration
        .entrypoint_instance
        .shim_session_nonce
        .clone();
      let response = state.register_fenced(request.registration, fence).await?;
      if let Some(id) = response
        .registration_id
        .as_ref()
        .filter(|_| response.accepted)
      {
        owned.insert(id.clone(), nonce);
      }
      send_response!(message_types::REGISTER_ENTRYPOINT_RESPONSE, response);
    }
    message_types::UNREGISTER_ENTRYPOINT_REQUEST => {
      let request = decode_request!(UnregisterEntrypointPayload);
      let fence = mutation_fence(operation_fence)?;
      let response = state
        .unregister_fenced(&request.registration_id, &request.shim_session_nonce, fence)
        .await?;
      if response.accepted {
        owned.remove(&request.registration_id);
      }
      send_response!(message_types::UNREGISTER_ENTRYPOINT_RESPONSE, response);
    }
    message_types::HEARTBEAT_ENTRYPOINT_REQUEST => {
      let request = decode_request!(HeartbeatEntrypointPayload);
      let response = state
        .heartbeat_fenced(request, mutation_fence(operation_fence)?)
        .await?;
      send_response!(message_types::HEARTBEAT_ENTRYPOINT_RESPONSE, response);
    }
    message_types::QUERY_STATE_REQUEST => {
      let _request = decode_request!(QueryStatePayload);
      let response = state.query_state().await;
      send_response!(message_types::QUERY_STATE_RESPONSE, response);
    }
    message_types::SET_ENTRYPOINT_ENABLED_REQUEST => {
      let request = decode_request!(SetEntrypointEnabledPayload);
      let response = state
        .set_entrypoint_enabled_fenced(request, mutation_fence(operation_fence)?)
        .await?;
      send_response!(message_types::SET_ENTRYPOINT_ENABLED_RESPONSE, response);
    }
    message_types::SET_DOMAIN_ENABLED_REQUEST => {
      let request = decode_request!(SetDomainEnabledPayload);
      let response = state
        .set_domain_enabled_fenced(request, mutation_fence(operation_fence)?)
        .await?;
      send_response!(message_types::SET_DOMAIN_ENABLED_RESPONSE, response);
    }
    message_types::QUERY_LOGS_REQUEST => {
      let request = decode_request!(QueryLogsPayload);
      let response = state.query_logs(request).await;
      send_response!(message_types::QUERY_LOGS_RESPONSE, response);
    }
    message_types::SHUTDOWN_DAEMON_REQUEST => {
      let _request = decode_request!(ShutdownDaemonPayload);
      let response = BasicResponse {
        request_id: authorized.request_id().to_string(),
        accepted: true,
        message: "Daemon shutdown started.".to_string(),
      };
      let shutdown_started_at = Instant::now();
      state.prepare_shutdown_at(shutdown_started_at);
      state.request_shutdown();
      #[cfg(test)]
      sleep(limits.shutdown_response_delay).await;
      let response_deadline = std::cmp::min(deadline, shutdown_started_at + limits.shutdown_accept);
      let write_result = write_envelope_until(
        writer,
        message_types::SHUTDOWN_DAEMON_RESPONSE,
        &response,
        response_deadline,
        limits.write_no_progress,
      )
      .await;
      write_result?;
      return Ok(ConnectionAction::Close);
    }
    other => {
      unreachable!("authorized operation `{other}` has no dispatcher")
    }
  }

  Ok(ConnectionAction::Continue)
}
