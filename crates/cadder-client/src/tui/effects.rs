use cadder_api::{DomainSelector, OperatorContext, unavailable_status};

use crate::app::{ActionFailure, ConnectionStatus, RefreshOutcome};
use crate::data::{EntityId, MutationTarget};

use super::events::UiAction;

pub(super) enum Effect {
  Refresh(cadder_ipc::LogStreamIdentity),
  Start,
  Stop,
  Restart,
  Mutate(MutationTarget),
}

impl Effect {
  pub(super) fn from(action: UiAction, log_stream: cadder_ipc::LogStreamIdentity) -> Self {
    match action {
      UiAction::Refresh => Self::Refresh(log_stream),
      UiAction::StartDaemon => Self::Start,
      UiAction::StopDaemon => Self::Stop,
      UiAction::RestartDaemon => Self::Restart,
      UiAction::Mutate(target) => Self::Mutate(target),
    }
  }
}

pub(super) enum EffectResult {
  Refresh(RefreshOutcome),
  Start(Result<(), ActionFailure>),
  Lifecycle(Result<(), ActionFailure>),
  Mutation(Result<(), ActionFailure>),
}

pub(super) fn spawn_effect(
  effects: &mut tokio::task::JoinSet<EffectResult>,
  context: OperatorContext,
  effect: Effect,
) {
  effects.spawn(async move {
    match effect {
      Effect::Refresh(stream) => EffectResult::Refresh(load_refresh(&context, stream).await),
      Effect::Start => EffectResult::Start(
        context
          .ensure_daemon_running("tui")
          .await
          .map_err(Into::into),
      ),
      Effect::Stop => EffectResult::Lifecycle(context.stop_daemon("tui").await.map_err(Into::into)),
      Effect::Restart => {
        EffectResult::Lifecycle(context.restart_daemon("tui").await.map_err(Into::into))
      }
      Effect::Mutate(target) => EffectResult::Mutation(execute_mutation(&context, target).await),
    }
  });
}

async fn load_refresh(
  context: &OperatorContext,
  stream: cadder_ipc::LogStreamIdentity,
) -> RefreshOutcome {
  match context.query_state_response().await {
    Ok(response) => {
      let Some(snapshot) = response.snapshot else {
        return RefreshOutcome::Unavailable {
          connection: ConnectionStatus::Error,
          message: "Cadder daemon returned no state snapshot.".to_string(),
          guidance: None,
        };
      };
      let logs = context
        .query_logs("tui", "query logs", stream, 200)
        .await
        .map_err(Into::into);
      RefreshOutcome::Connected {
        snapshot: Box::new(snapshot),
        logs,
      }
    }
    Err(error) => {
      let status = unavailable_status(context, &error);
      let connection = match status.connection_state {
        cadder_api::ConnectionStateView::NotRunning => ConnectionStatus::Offline,
        cadder_api::ConnectionStateView::ConnectionFailed => ConnectionStatus::Error,
        cadder_api::ConnectionStateView::Connected => ConnectionStatus::Connected,
      };
      RefreshOutcome::Unavailable {
        connection,
        message: status.message,
        guidance: status.guidance,
      }
    }
  }
}

async fn execute_mutation(
  context: &OperatorContext,
  target: MutationTarget,
) -> Result<(), ActionFailure> {
  let result = match target.entity {
    EntityId::Entrypoint(registration_id) => context
      .set_entrypoint_enabled("tui", registration_id, target.enabled)
      .await
      .map(|_| ()),
    EntityId::Domain {
      registration_id,
      canonical_domain,
    } => context
      .set_domain_enabled(
        "tui",
        &DomainSelector {
          domain: canonical_domain,
          registration: Some(registration_id),
        },
        target.enabled,
      )
      .await
      .map(|_| ()),
    EntityId::Status(_) => return Ok(()),
  };
  result.map_err(Into::into)
}

#[cfg(test)]
mod tests {
  use super::*;
  use cadder_api::{
    CadderSession, CaddyConfigCoordinator, DaemonLaunchOptions, DaemonServer, DaemonState,
    RuntimePaths,
  };
  use cadder_ipc::{
    ActivationState, EntrypointInstanceIdentity, EntrypointRegistration, LogStreamIdentity,
    OwnerProcessIdentity, RegisterEntrypointPayload, RegisteredDomain, SourcePath, new_request_id,
  };
  use chrono::Utc;
  use tokio::sync::watch;
  use tokio::time::{Duration, sleep};

  fn registration() -> EntrypointRegistration {
    let now = Utc::now();
    EntrypointRegistration {
      registration_id: "entry-1".to_string(),
      entrypoint_instance: EntrypointInstanceIdentity {
        instance_id: "entry-1".to_string(),
        started_at_utc: now,
        shim_session_nonce: "nonce-1".to_string(),
      },
      source_working_directory: SourcePath::new("D:/Projects/App", None),
      source_config_path: SourcePath::new("D:/Projects/App/Caddyfile", None),
      registered_domains: vec![RegisteredDomain::active("app.localhost")],
      activation_state: ActivationState::Active,
      owner_process: OwnerProcessIdentity {
        process_id: std::process::id(),
        process_start_time_utc: now,
        shim_session_nonce: "nonce-1".to_string(),
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint("entry-1"),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
    }
  }

  async fn running_context() -> (
    tempfile::TempDir,
    OperatorContext,
    watch::Sender<bool>,
    CadderSession,
    tokio::task::JoinHandle<()>,
  ) {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    paths.ensure_dirs().unwrap();
    let state = DaemonState::new(CaddyConfigCoordinator::new_mock(paths.clone()));
    let server = DaemonServer::new(paths.clone(), state);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let server_task = tokio::spawn(async move { server.run_until(shutdown_rx).await.unwrap() });
    let mut session = loop {
      match CadderSession::connect(&paths).await {
        Ok(session) => break session,
        Err(_) => sleep(Duration::from_millis(10)).await,
      }
    };
    let response = session
      .request(
        new_request_id("register"),
        &RegisterEntrypointPayload {
          registration: registration(),
        },
      )
      .await
      .unwrap();
    assert!(response.accepted, "{}", response.message);
    (
      temp,
      OperatorContext::from_paths(paths, DaemonLaunchOptions::default()),
      shutdown_tx,
      session,
      server_task,
    )
  }

  #[test]
  fn effects_map_every_ui_action_without_erasing_payloads() {
    let stream = LogStreamIdentity::domain("app.localhost");
    assert!(matches!(
      Effect::from(UiAction::Refresh, stream.clone()),
      Effect::Refresh(value) if value == stream
    ));
    assert!(matches!(
      Effect::from(UiAction::StartDaemon, stream.clone()),
      Effect::Start
    ));
    assert!(matches!(
      Effect::from(UiAction::StopDaemon, stream.clone()),
      Effect::Stop
    ));
    assert!(matches!(
      Effect::from(UiAction::RestartDaemon, stream.clone()),
      Effect::Restart
    ));
    let target = MutationTarget {
      entity: EntityId::Status(crate::data::StatusId::Runtime),
      enabled: true,
    };
    assert!(matches!(
      Effect::from(UiAction::Mutate(target), stream),
      Effect::Mutate(MutationTarget {
        entity: EntityId::Status(_),
        enabled: true
      })
    ));
  }

  #[tokio::test]
  async fn refresh_mutations_and_lifecycle_effects_run_through_the_operator_api() {
    let (_temp, context, _shutdown_tx, _session, server_task) = running_context().await;
    let refresh = load_refresh(&context, LogStreamIdentity::runtime_control()).await;
    assert!(matches!(refresh, RefreshOutcome::Connected { .. }));

    for target in [
      MutationTarget {
        entity: EntityId::Entrypoint("entry-1".to_string()),
        enabled: false,
      },
      MutationTarget {
        entity: EntityId::Entrypoint("entry-1".to_string()),
        enabled: true,
      },
      MutationTarget {
        entity: EntityId::Domain {
          registration_id: "entry-1".to_string(),
          canonical_domain: "app.localhost".to_string(),
        },
        enabled: false,
      },
      MutationTarget {
        entity: EntityId::Status(crate::data::StatusId::Runtime),
        enabled: false,
      },
    ] {
      execute_mutation(&context, target).await.unwrap();
    }

    let mut effects = tokio::task::JoinSet::new();
    spawn_effect(
      &mut effects,
      context.clone(),
      Effect::Refresh(LogStreamIdentity::runtime_control()),
    );
    assert!(matches!(
      effects.join_next().await.unwrap().unwrap(),
      EffectResult::Refresh(_)
    ));
    spawn_effect(&mut effects, context, Effect::Stop);
    assert!(matches!(
      effects.join_next().await.unwrap().unwrap(),
      EffectResult::Lifecycle(Ok(()))
    ));
    server_task.await.unwrap();
  }

  #[tokio::test]
  async fn unavailable_effects_preserve_failures() {
    let temp = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::resolve(Some(temp.path().join("runtime"))).unwrap();
    let context = OperatorContext::from_paths(
      paths,
      DaemonLaunchOptions {
        explicit_daemon: Some(temp.path().join("missing-cadderd")),
        ..DaemonLaunchOptions::default()
      },
    );

    assert!(matches!(
      load_refresh(&context, LogStreamIdentity::runtime_control()).await,
      RefreshOutcome::Unavailable {
        connection: ConnectionStatus::Offline,
        ..
      }
    ));

    let mut effects = tokio::task::JoinSet::new();
    for effect in [Effect::Start, Effect::Restart] {
      spawn_effect(&mut effects, context.clone(), effect);
      match effects.join_next().await.unwrap().unwrap() {
        EffectResult::Start(result) | EffectResult::Lifecycle(result) => assert!(result.is_err()),
        _ => panic!("unexpected effect result"),
      }
    }
  }
}
