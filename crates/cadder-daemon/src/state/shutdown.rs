use super::*;
use tokio::time::{Duration, Instant, sleep, timeout_at};

pub(crate) struct ShutdownPreparation {
  pub(crate) response: BasicResponse,
  pub(crate) runtime_quiescent: bool,
}

impl DaemonState {
  pub async fn shutdown(&self) -> BasicResponse {
    let response = self.prepare_shutdown().await;
    if response.accepted {
      self.request_shutdown();
    }
    response
  }

  pub(crate) async fn prepare_shutdown(&self) -> BasicResponse {
    let _operation = self
      .config_operation
      .acquire()
      .await
      .expect("config operation semaphore closed");
    let runtime = {
      let coordinator = self.coordinator.lock().await;
      coordinator.runtime()
    };
    self.finish_shutdown(runtime.stop().await)
  }

  pub(crate) async fn prepare_shutdown_until(&self, deadline: Instant) -> ShutdownPreparation {
    let _operation = match timeout_at(deadline, self.config_operation.acquire()).await {
      Ok(Ok(operation)) => operation,
      Ok(Err(_)) => panic!("config operation semaphore closed"),
      Err(_) => {
        return ShutdownPreparation {
          response: shutdown_failure(
            "Daemon shutdown could not acquire the runtime operation before its deadline.",
          ),
          runtime_quiescent: false,
        };
      }
    };
    let runtime = match timeout_at(deadline, self.coordinator.lock()).await {
      Ok(coordinator) => coordinator.runtime(),
      Err(_) => {
        return ShutdownPreparation {
          response: shutdown_failure(
            "Daemon shutdown could not inspect runtime ownership before its deadline.",
          ),
          runtime_quiescent: false,
        };
      }
    };
    let (result, runtime_quiescent) = runtime.stop_until(deadline).await.into_parts();
    ShutdownPreparation {
      response: self.finish_shutdown(result),
      runtime_quiescent,
    }
  }

  pub(crate) async fn contain_runtime(&self) -> anyhow::Result<()> {
    let _operation = self
      .config_operation
      .acquire()
      .await
      .expect("config operation semaphore closed");
    let runtime = {
      let coordinator = self.coordinator.lock().await;
      coordinator.runtime()
    };
    runtime.contain().await
  }

  pub(crate) async fn contain_runtime_fail_stop(&self) {
    let mut failure_recorded = false;
    loop {
      match self.contain_runtime().await {
        Ok(()) => return,
        Err(error) => {
          if !failure_recorded {
            self.logs.append(
              LogStreamIdentity::runtime_control(),
              LogSeverity::Error,
              format!(
                "Owned Caddy containment did not complete; Cadder keeps runtime ownership and retries without releasing discovery or the daemon lock: {error:#}"
              ),
              LogAttributionKind::RuntimeControl,
              Some("shutdown-containment".to_string()),
            );
            failure_recorded = true;
          }
          sleep(Duration::from_millis(250)).await;
        }
      }
    }
  }

  pub(crate) async fn shutdown_storage_until(&self, deadline: Instant) -> anyhow::Result<bool> {
    self.store.shutdown_until(deadline).await
  }

  pub(crate) async fn contain_storage_shutdown(&self) -> anyhow::Result<()> {
    self.store.contain_shutdown().await
  }

  fn finish_shutdown(&self, result: anyhow::Result<()>) -> BasicResponse {
    if let Err(error) = result {
      return BasicResponse {
        request_id: "shutdown".to_string(),
        accepted: false,
        message: error.to_string(),
      };
    }
    if let Err(error) = self.store.enqueue_history_for_shutdown(
      HistoryKind::Runtime,
      "Daemon shutdown requested.",
      &serde_json::json!({ "accepted": true }),
    ) {
      return shutdown_failure(&format!(
        "Daemon shutdown could not queue its final history record: {error:#}"
      ));
    }
    self.begin_operation_drain();
    BasicResponse {
      request_id: "shutdown".to_string(),
      accepted: true,
      message: "Daemon shutdown requested.".to_string(),
    }
  }

  pub(crate) fn request_shutdown(&self) {
    self.shutdown_signal.request();
  }

  pub(crate) fn prepare_shutdown_at(&self, started_at: Instant) {
    self.shutdown_signal.prepare(started_at);
  }
}

fn shutdown_failure(message: &str) -> BasicResponse {
  BasicResponse {
    request_id: "shutdown".to_string(),
    accepted: false,
    message: message.to_string(),
  }
}
