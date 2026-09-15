use super::*;

pub(super) async fn supervise_unary_request<W>(
  reader: &mut IpcFrameReader,
  writer: &mut W,
  authorized: &AuthorizedRequestEnvelope<'_>,
  supervision: UnarySupervisionContext<'_>,
) -> Result<ConnectionAction>
where
  W: AsyncWrite + Unpin,
{
  let state = supervision.state;
  let owned = supervision.owned;
  let limits = supervision.limits;
  let definition = authorized.definition();
  let deadline = limits.operation_deadline(definition.deadline());
  let request_id = Some(authorized.request_id().clone());
  let operation_fence = if operation_uses_fence(definition) {
    match state.issue_operation_fence() {
      Ok(fence) => Some(fence),
      Err(CommitRejection::Draining) => {
        send_shutting_down(writer, request_id.clone(), limits).await?;
        return Ok(ConnectionAction::Close);
      }
      Err(error) => return Err(error.into()),
    }
  } else {
    None
  };
  if owned_mutation_uses_worker(definition.name()) {
    return supervise_owned_mutation_request(
      reader,
      writer,
      authorized,
      operation_fence.expect("registration mutation has an operation fence"),
      deadline,
      supervision,
    )
    .await;
  }
  let mut handler = Box::pin(dispatch_authorized_request(
    writer,
    state,
    owned,
    authorized,
    RequestDispatchContext {
      operation_fence: operation_fence.as_ref(),
      deadline,
      limits,
    },
  ));
  let mut next_frame = Box::pin(reader.next());

  enum First<T> {
    Handler(T),
    Reader(ConcurrentRead),
    Cancelled,
    ShuttingDown,
    Timeout,
  }

  let first = tokio::select! {
    biased;
    _ = wait_for_request_drain(definition, supervision.request_drain) => First::ShuttingDown,
    _ = wait_for_operation_cancellation(operation_fence.as_ref()) => First::Cancelled,
    frame = &mut next_frame => First::Reader(classify_concurrent_read(frame)),
    _ = sleep_until(deadline) => First::Timeout,
    result = &mut handler => First::Handler(result),
  };

  match first {
    First::Handler(result) => {
      if let Some(fence) = &operation_fence {
        fence.complete();
      }
      let action = result?;
      drop(handler);
      drop(next_frame);
      if reader.read_buffer().is_empty() {
        Ok(action)
      } else {
        send_pipelined_error(writer, None, limits).await?;
        Ok(ConnectionAction::Close)
      }
    }
    First::Reader(concurrent) => {
      drop(next_frame);
      enum FollowUp<T> {
        Handler(T),
        ShuttingDown,
        Cancelled,
        Timeout,
      }
      let handler_result = tokio::select! {
        biased;
        _ = wait_for_request_drain(definition, supervision.request_drain) => FollowUp::ShuttingDown,
        _ = wait_for_operation_cancellation(operation_fence.as_ref()) => FollowUp::Cancelled,
        _ = sleep_until(deadline) => FollowUp::Timeout,
        result = &mut handler => FollowUp::Handler(result),
      };

      match handler_result {
        FollowUp::Handler(result) => {
          if let Some(fence) = &operation_fence {
            fence.complete();
          }
          drop(handler);
          result?;
        }
        FollowUp::ShuttingDown | FollowUp::Cancelled => {
          if let Some(fence) = &operation_fence {
            fence.revoke();
          }
          drop(handler);
          send_request_shutting_down(writer, request_id.clone(), limits).await?;
          return Ok(ConnectionAction::Close);
        }
        FollowUp::Timeout => {
          if let Some(fence) = &operation_fence {
            fence.revoke();
          }
          drop(handler);
          send_operation_timeout(writer, request_id.clone(), definition, limits).await?;
        }
      }
      if let ConcurrentRead::Pipelined(pipelined_request_id) = concurrent {
        send_pipelined_error(writer, pipelined_request_id, limits).await?;
      }
      Ok(ConnectionAction::Close)
    }
    First::Cancelled => {
      if let Some(fence) = &operation_fence {
        fence.revoke();
      }
      drop(handler);
      drop(next_frame);
      Ok(ConnectionAction::Close)
    }
    First::ShuttingDown => {
      if let Some(fence) = &operation_fence {
        fence.revoke();
      }
      drop(handler);
      drop(next_frame);
      send_request_shutting_down(writer, request_id.clone(), limits).await?;
      Ok(ConnectionAction::Close)
    }
    First::Timeout => {
      if let Some(fence) = &operation_fence {
        fence.revoke();
      }
      drop(handler);
      drop(next_frame);
      send_operation_timeout(writer, request_id, definition, limits).await?;
      Ok(ConnectionAction::Close)
    }
  }
}

pub(super) fn owned_mutation_uses_worker(message_type: &str) -> bool {
  matches!(
    message_type,
    message_types::REGISTER_ENTRYPOINT_REQUEST
      | message_types::UNREGISTER_ENTRYPOINT_REQUEST
      | message_types::HEARTBEAT_ENTRYPOINT_REQUEST
      | message_types::SET_ENTRYPOINT_ENABLED_REQUEST
      | message_types::SET_DOMAIN_ENABLED_REQUEST
  )
}

pub(super) async fn supervise_owned_mutation_request<W>(
  reader: &mut IpcFrameReader,
  writer: &mut W,
  authorized: &AuthorizedRequestEnvelope<'_>,
  fence: OperationFence,
  deadline: Instant,
  supervision: UnarySupervisionContext<'_>,
) -> Result<ConnectionAction>
where
  W: AsyncWrite + Unpin,
{
  let UnarySupervisionContext {
    state,
    owned,
    mutation_tasks,
    mutation_cancellation,
    request_drain: _,
    limits,
  } = supervision;
  macro_rules! decode_owned_request {
    ($request:ty, $map:expr) => {
      match authorized.decode::<$request>() {
        Ok(request) => $map(request),
        Err(error) => {
          let response =
            ProtocolErrorResponse::rejected(Some(authorized.request_id().clone()), error);
          write_envelope_until(
            writer,
            message_types::PROTOCOL_ERROR_RESPONSE,
            &response,
            deadline,
            limits.write_no_progress,
          )
          .await?;
          return Ok(ConnectionAction::Continue);
        }
      }
    };
  }
  let request = match authorized.definition().name() {
    message_types::REGISTER_ENTRYPOINT_REQUEST => {
      decode_owned_request!(RegisterEntrypointPayload, |request| {
        OwnedMutationRequest::Register(Box::new(request))
      })
    }
    message_types::UNREGISTER_ENTRYPOINT_REQUEST => decode_owned_request!(
      UnregisterEntrypointPayload,
      OwnedMutationRequest::Unregister
    ),
    message_types::HEARTBEAT_ENTRYPOINT_REQUEST => {
      decode_owned_request!(HeartbeatEntrypointPayload, OwnedMutationRequest::Heartbeat)
    }
    message_types::SET_ENTRYPOINT_ENABLED_REQUEST => decode_owned_request!(
      SetEntrypointEnabledPayload,
      OwnedMutationRequest::SetEntrypointEnabled
    ),
    message_types::SET_DOMAIN_ENABLED_REQUEST => decode_owned_request!(
      SetDomainEnabledPayload,
      OwnedMutationRequest::SetDomainEnabled
    ),
    _ => unreachable!("owned mutation supervisor called for an untracked operation"),
  };
  let request_id = Some(authorized.request_id().clone());
  let response_type = request.response_type();
  let Some(mutation_ownership) = owned.begin_mutation() else {
    let _ = fence.try_revoke();
    send_shutting_down(writer, request_id.clone(), limits).await?;
    return Ok(ConnectionAction::Close);
  };
  let worker_state = state.clone();
  let response_request_id = authorized.request_id().to_string();
  let worker_fence = fence.clone();
  let completion_fence = fence.clone();
  let worker_ownership = owned.clone();
  let worker_cancellation = mutation_cancellation.clone();
  let worker = async move {
    let _mutation_ownership = mutation_ownership;
    #[cfg(test)]
    tokio::select! {
      biased;
      _ = worker_cancellation.cancelled() => return Err(CommitRejection::Revoked),
      _ = sleep(limits.dispatch_delay) => {}
    }
    let mut execution = Box::pin(async move {
      request
        .execute(
          worker_state,
          worker_ownership,
          worker_fence,
          response_request_id,
        )
        .await
    });
    tokio::select! {
      result = &mut execution => result,
      _ = worker_cancellation.cancelled() => {
        let _ = completion_fence.try_revoke();
        execution.await
      }
    }
  };
  let Some(mut worker) = mutation_tasks.spawn(worker) else {
    let _ = fence.try_revoke();
    send_shutting_down(writer, request_id.clone(), limits).await?;
    return Ok(ConnectionAction::Close);
  };
  let mut next_frame = Box::pin(reader.next());
  let cancellation = fence.cancellation();

  enum First<T> {
    Worker(T),
    Reader(ConcurrentRead),
    ShuttingDown,
    Cancelled,
    Timeout,
  }

  let first = tokio::select! {
    biased;
    _ = supervision.request_drain.cancelled() => First::ShuttingDown,
    _ = cancellation.cancelled() => First::Cancelled,
    frame = &mut next_frame => First::Reader(classify_concurrent_read(frame)),
    result = &mut worker => First::Worker(result),
    _ = sleep_until(deadline) => First::Timeout,
  };

  match first {
    First::Worker(result) => {
      let response = result.context("owned mutation worker failed")??;
      let wrote_response = write_owned_mutation_result(
        writer,
        &response,
        OwnedMutationWriteContext {
          fence: &fence,
          request_id: request_id.as_ref(),
          definition: authorized.definition(),
          response_type,
          deadline,
          limits,
        },
      )
      .await?;
      drop(next_frame);
      if !wrote_response {
        return Ok(ConnectionAction::Close);
      }
      if reader.read_buffer().is_empty() {
        Ok(ConnectionAction::Continue)
      } else {
        send_pipelined_error(writer, None, limits).await?;
        Ok(ConnectionAction::Close)
      }
    }
    First::Reader(concurrent) => {
      drop(next_frame);
      enum FollowUp<T> {
        Worker(T),
        ShuttingDown,
        Cancelled,
        Timeout,
      }
      let worker_result = tokio::select! {
        biased;
        _ = supervision.request_drain.cancelled() => FollowUp::ShuttingDown,
        _ = cancellation.cancelled() => FollowUp::Cancelled,
        result = &mut worker => FollowUp::Worker(result),
        _ = sleep_until(deadline) => FollowUp::Timeout,
      };

      match worker_result {
        FollowUp::Worker(result) => {
          let response = result.context("owned mutation worker failed")??;
          let _ = write_owned_mutation_result(
            writer,
            &response,
            OwnedMutationWriteContext {
              fence: &fence,
              request_id: request_id.as_ref(),
              definition: authorized.definition(),
              response_type,
              deadline,
              limits,
            },
          )
          .await?;
        }
        FollowUp::ShuttingDown => {
          match fence.try_revoke() {
            RevokeOutcome::Finalized => {
              let response = (&mut worker)
                .await
                .context("owned mutation worker failed")??;
              write_envelope_until(
                writer,
                response_type,
                &response,
                Instant::now() + limits.write_no_progress,
                limits.write_no_progress,
              )
              .await?;
            }
            RevokeOutcome::Revoked | RevokeOutcome::AlreadyRevoked => {
              drop(worker);
              send_request_shutting_down(writer, request_id.clone(), limits).await?;
            }
          }
          return Ok(ConnectionAction::Close);
        }
        FollowUp::Cancelled => {
          let response = (&mut worker)
            .await
            .context("owned mutation worker failed")??;
          write_envelope_until(
            writer,
            response_type,
            &response,
            Instant::now() + limits.write_no_progress,
            limits.write_no_progress,
          )
          .await?;
        }
        FollowUp::Timeout => match fence.try_revoke() {
          RevokeOutcome::Revoked => {
            drop(worker);
            send_operation_timeout(writer, request_id.clone(), authorized.definition(), limits)
              .await?;
          }
          RevokeOutcome::Finalized => {
            let response = (&mut worker)
              .await
              .context("owned mutation worker failed")??;
            write_envelope_until(
              writer,
              response_type,
              &response,
              Instant::now() + limits.write_no_progress,
              limits.write_no_progress,
            )
            .await?;
          }
          RevokeOutcome::AlreadyRevoked => drop(worker),
        },
      }
      if let ConcurrentRead::Pipelined(pipelined_request_id) = concurrent {
        send_pipelined_error(writer, pipelined_request_id, limits).await?;
      }
      Ok(ConnectionAction::Close)
    }
    First::ShuttingDown => {
      match fence.try_revoke() {
        RevokeOutcome::Finalized => {
          let response = worker.await.context("owned mutation worker failed")??;
          write_envelope_until(
            writer,
            response_type,
            &response,
            Instant::now() + limits.write_no_progress,
            limits.write_no_progress,
          )
          .await?;
        }
        RevokeOutcome::Revoked | RevokeOutcome::AlreadyRevoked => {
          drop(worker);
          send_request_shutting_down(writer, request_id.clone(), limits).await?;
        }
      }
      drop(next_frame);
      Ok(ConnectionAction::Close)
    }
    First::Cancelled => {
      let response = worker.await.context("owned mutation worker failed")??;
      write_envelope_until(
        writer,
        response_type,
        &response,
        Instant::now() + limits.write_no_progress,
        limits.write_no_progress,
      )
      .await?;
      drop(next_frame);
      Ok(ConnectionAction::Close)
    }
    First::Timeout => {
      drop(next_frame);
      match fence.try_revoke() {
        RevokeOutcome::Revoked => {
          drop(worker);
          send_operation_timeout(writer, request_id, authorized.definition(), limits).await?;
        }
        RevokeOutcome::Finalized => {
          let response = worker.await.context("owned mutation worker failed")??;
          write_envelope_until(
            writer,
            response_type,
            &response,
            Instant::now() + limits.write_no_progress,
            limits.write_no_progress,
          )
          .await?;
        }
        RevokeOutcome::AlreadyRevoked => drop(worker),
      }
      Ok(ConnectionAction::Close)
    }
  }
}

pub(super) async fn write_owned_mutation_result<W>(
  writer: &mut W,
  response: &OwnedMutationResponse,
  context: OwnedMutationWriteContext<'_>,
) -> Result<bool>
where
  W: AsyncWrite + Unpin,
{
  let write_deadline = if Instant::now() < context.deadline {
    context.fence.complete();
    context.deadline
  } else {
    match context.fence.try_revoke() {
      RevokeOutcome::Finalized => Instant::now() + context.limits.write_no_progress,
      RevokeOutcome::Revoked => {
        send_operation_timeout(
          writer,
          context.request_id.cloned(),
          context.definition,
          context.limits,
        )
        .await?;
        return Ok(false);
      }
      RevokeOutcome::AlreadyRevoked => return Ok(false),
    }
  };
  write_envelope_until(
    writer,
    context.response_type,
    response,
    write_deadline,
    context.limits.write_no_progress,
  )
  .await?;
  Ok(true)
}

pub(super) enum OwnedMutationRequest {
  Register(Box<RegisterEntrypointPayload>),
  Unregister(UnregisterEntrypointPayload),
  Heartbeat(HeartbeatEntrypointPayload),
  SetEntrypointEnabled(SetEntrypointEnabledPayload),
  SetDomainEnabled(SetDomainEnabledPayload),
}

impl OwnedMutationRequest {
  fn response_type(&self) -> &'static str {
    match self {
      Self::Register(_) => message_types::REGISTER_ENTRYPOINT_RESPONSE,
      Self::Unregister(_) => message_types::UNREGISTER_ENTRYPOINT_RESPONSE,
      Self::Heartbeat(_) => message_types::HEARTBEAT_ENTRYPOINT_RESPONSE,
      Self::SetEntrypointEnabled(_) => message_types::SET_ENTRYPOINT_ENABLED_RESPONSE,
      Self::SetDomainEnabled(_) => message_types::SET_DOMAIN_ENABLED_RESPONSE,
    }
  }

  async fn execute(
    self,
    state: DaemonState,
    ownership: ConnectionOwnership,
    fence: OperationFence,
    request_id: String,
  ) -> Result<OwnedMutationResponse, CommitRejection> {
    match self {
      Self::Register(request) => {
        let nonce = request
          .registration
          .entrypoint_instance
          .shim_session_nonce
          .clone();
        let mut response = state.register_fenced(request.registration, &fence).await?;
        response.request_id = request_id;
        if let Some(registration_id) = response
          .registration_id
          .as_ref()
          .filter(|_| response.accepted)
        {
          ownership.insert(registration_id.clone(), nonce);
        }
        Ok(OwnedMutationResponse::Register(response))
      }
      Self::Unregister(request) => {
        let registration_id = request.registration_id.clone();
        let mut response = state
          .unregister_fenced(&registration_id, &request.shim_session_nonce, &fence)
          .await?;
        response.request_id = request_id;
        if response.accepted {
          ownership.remove(&registration_id);
        }
        Ok(OwnedMutationResponse::Basic(response))
      }
      Self::Heartbeat(request) => {
        let mut response = state.heartbeat_fenced(request, &fence).await?;
        response.request_id = request_id;
        Ok(OwnedMutationResponse::Basic(response))
      }
      Self::SetEntrypointEnabled(request) => {
        let mut response = state.set_entrypoint_enabled_fenced(request, &fence).await?;
        response.request_id = request_id;
        Ok(OwnedMutationResponse::Basic(response))
      }
      Self::SetDomainEnabled(request) => {
        let mut response = state.set_domain_enabled_fenced(request, &fence).await?;
        response.request_id = request_id;
        Ok(OwnedMutationResponse::Basic(response))
      }
    }
  }
}

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum OwnedMutationResponse {
  Register(cadder_ipc::RegisterEntrypointResponse),
  Basic(cadder_ipc::BasicResponse),
}

pub(super) fn operation_uses_fence(definition: &cadder_ipc::OperationDefinition) -> bool {
  definition.access() == OperationAccess::Mutation
    && definition.name() != message_types::SHUTDOWN_DAEMON_REQUEST
}

pub(super) async fn wait_for_operation_cancellation(fence: Option<&OperationFence>) {
  match fence {
    Some(fence) => {
      let cancellation = fence.cancellation();
      cancellation.cancelled().await;
    }
    None => std::future::pending().await,
  }
}

pub(super) async fn wait_for_request_drain(
  definition: &cadder_ipc::OperationDefinition,
  request_drain: &CancellationToken,
) {
  if definition.name() == message_types::SHUTDOWN_DAEMON_REQUEST {
    std::future::pending().await
  } else {
    request_drain.cancelled().await
  }
}
