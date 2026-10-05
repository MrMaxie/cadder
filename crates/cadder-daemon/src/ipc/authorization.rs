use super::*;

pub(super) async fn authorize_or_reject<W>(
  writer: &mut W,
  state: &DaemonState,
  envelope: &RawRequestEnvelope,
  security: &ConnectionSecurityContext,
) -> Result<bool>
where
  W: AsyncWrite + Unpin,
{
  let operation = operation_for_message_type(envelope.operation());
  let decision = security.policy.evaluate(
    &security.owner_principal,
    &security.peer_principal,
    &operation,
  );
  if decision.is_allowed() {
    return Ok(true);
  }

  state
    .logs()
    .append(
      LogStreamIdentity::runtime_control(),
      LogSeverity::Warn,
      format!(
        "Denied Cadder IPC {:?} operation `{}` by local security policy: {}",
        operation.kind(),
        operation.name(),
        decision.reason_code()
      ),
      LogAttributionKind::RuntimeControl,
      Some("ipc-access-denied".to_string()),
    )
    .await;
  let error = ProtocolError::access_denied(
    operation.name(),
    decision.message(),
    decision.guidance().map(ToOwned::to_owned),
  );
  let response = ProtocolErrorResponse::rejected(Some(envelope.request_id().clone()), error);
  write_envelope(writer, message_types::PROTOCOL_ERROR_RESPONSE, &response).await?;
  Ok(false)
}

pub(super) fn operation_for_message_type(message_type: &str) -> IpcOperation {
  match OPERATION_REGISTRY.lookup(message_type) {
    Some(operation) if operation.access() == OperationAccess::Mutation => {
      IpcOperation::state_changing(message_type)
    }
    _ => IpcOperation::read_only(message_type),
  }
}

#[derive(Debug, Default)]
struct ConnectionOwnershipState {
  by_registration_id: BTreeMap<String, String>,
  active_mutations: usize,
  disconnected: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ConnectionOwnership {
  state: Arc<std::sync::Mutex<ConnectionOwnershipState>>,
  mutations_finished: Arc<tokio::sync::Notify>,
}

pub(super) struct ConnectionMutationGuard(ConnectionOwnership);

impl Drop for ConnectionMutationGuard {
  fn drop(&mut self) {
    let should_notify = {
      let mut state = self
        .0
        .state
        .lock()
        .expect("connection ownership lock poisoned");
      state.active_mutations -= 1;
      state.disconnected && state.active_mutations == 0
    };
    if should_notify {
      self.0.mutations_finished.notify_one();
    }
  }
}

impl ConnectionOwnership {
  pub(super) fn begin_mutation(&self) -> Option<ConnectionMutationGuard> {
    let mut state = self
      .state
      .lock()
      .expect("connection ownership lock poisoned");
    if state.disconnected {
      return None;
    }
    state.active_mutations += 1;
    Some(ConnectionMutationGuard(self.clone()))
  }

  pub(super) fn insert(&self, registration_id: String, shim_session_nonce: String) {
    self
      .state
      .lock()
      .expect("connection ownership lock poisoned")
      .by_registration_id
      .insert(registration_id, shim_session_nonce);
  }

  pub(super) fn remove(&self, registration_id: &str) {
    self
      .state
      .lock()
      .expect("connection ownership lock poisoned")
      .by_registration_id
      .remove(registration_id);
  }

  pub(super) async fn disconnect_and_take_entries(&self) -> BTreeMap<String, String> {
    {
      let mut state = self
        .state
        .lock()
        .expect("connection ownership lock poisoned");
      state.disconnected = true;
    }
    loop {
      let mutations_finished = self.mutations_finished.notified();
      {
        let mut state = self
          .state
          .lock()
          .expect("connection ownership lock poisoned");
        if state.active_mutations == 0 {
          return std::mem::take(&mut state.by_registration_id);
        }
      }
      mutations_finished.await;
    }
  }
}
