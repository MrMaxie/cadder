use super::*;

#[derive(Debug, Clone)]
pub struct ProcessRuntime {
  resolver: RealCaddyResolver,
  pub(super) paths: RuntimePaths,
  pub(super) child: Arc<Mutex<Option<OwnedRuntimeProcess>>>,
  snapshot: Arc<StdMutex<RuntimeState>>,
  pub(super) log_tasks: TaskTracker,
  pub(super) timeouts: RuntimeTimeouts,
  #[cfg(test)]
  force_inspect_failure: Arc<AtomicBool>,
}

#[derive(Debug)]
pub(super) struct OwnedRuntimeProcess {
  pub(super) child: ProcessTreeChild,
  binary: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeTimeouts {
  pub start_check: Duration,
  pub reload: Duration,
  pub graceful_stop: Duration,
  pub stop_wait: Duration,
  pub kill_wait: Duration,
}

impl Default for RuntimeTimeouts {
  fn default() -> Self {
    Self {
      start_check: Duration::from_millis(250),
      reload: Duration::from_secs(30),
      graceful_stop: Duration::from_secs(3),
      stop_wait: Duration::from_secs(3),
      kill_wait: Duration::from_secs(4),
    }
  }
}

impl RuntimeTimeouts {
  fn stop_budget(self) -> Duration {
    let defaults = Self::default();
    std::cmp::min(self.graceful_stop, defaults.graceful_stop)
      + std::cmp::min(self.stop_wait, defaults.stop_wait)
      + std::cmp::min(self.kill_wait, defaults.kill_wait)
  }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RuntimeStopDeadlines {
  pub(super) graceful: Instant,
  pub(super) stop: Instant,
  pub(super) kill: Instant,
}

impl RuntimeStopDeadlines {
  pub(super) fn new(overall: Instant, timeouts: RuntimeTimeouts) -> Self {
    let started = Instant::now();
    let available = overall.saturating_duration_since(started);
    let defaults = RuntimeTimeouts::default();
    let graceful = std::cmp::min(timeouts.graceful_stop, defaults.graceful_stop);
    let stop = std::cmp::min(timeouts.stop_wait, defaults.stop_wait);
    let kill = std::cmp::min(timeouts.kill_wait, defaults.kill_wait);
    let configured = graceful + stop + kill;
    let scale = if configured.is_zero() {
      0.0
    } else {
      (available.as_secs_f64() / configured.as_secs_f64()).min(1.0)
    };
    let graceful_deadline = std::cmp::min(overall, started + graceful.mul_f64(scale));
    let stop_deadline = std::cmp::min(overall, graceful_deadline + stop.mul_f64(scale));
    let kill_deadline = std::cmp::min(overall, stop_deadline + kill.mul_f64(scale));
    Self {
      graceful: graceful_deadline,
      stop: stop_deadline,
      kill: kill_deadline,
    }
  }
}

impl ProcessRuntime {
  pub fn new(resolver: RealCaddyResolver, paths: RuntimePaths) -> Self {
    Self::with_timeouts(resolver, paths, RuntimeTimeouts::default())
  }

  pub fn with_timeouts(
    resolver: RealCaddyResolver,
    paths: RuntimePaths,
    timeouts: RuntimeTimeouts,
  ) -> Self {
    Self {
      resolver,
      paths,
      child: Arc::new(Mutex::new(None)),
      snapshot: Arc::new(StdMutex::new(RuntimeState::idle())),
      log_tasks: TaskTracker::new(),
      timeouts,
      #[cfg(test)]
      force_inspect_failure: Arc::new(AtomicBool::new(false)),
    }
  }

  pub async fn inspect(&self) -> RuntimeState {
    let Ok(mut guard) = self.child.try_lock() else {
      return self.snapshot();
    };
    if let Some(owned) = guard.as_mut() {
      let binary_path = Some(owned.binary.display().to_string());
      let process_id = owned.child.id();
      #[cfg(test)]
      if self.force_inspect_failure.load(Ordering::SeqCst) {
        let snapshot = self.runtime_unhealthy_state(
          binary_path,
          process_id,
          Some(CADDY_ADMIN_ENDPOINT),
          "runtime-inspect-failed",
          "could not inspect real Caddy runtime: injected failure".to_string(),
          Some("inspect".to_string()),
        );
        self.set_snapshot(snapshot.clone());
        return snapshot;
      }
      let status = owned.child.try_wait();
      match status {
        Ok(None) => {
          let snapshot = self.runtime_running_state(binary_path, process_id, None);
          self.set_snapshot(snapshot.clone());
          return snapshot;
        }
        Ok(Some(status)) => {
          let snapshot = self.runtime_unhealthy_state(
            binary_path,
            process_id,
            None,
            "runtime-exited",
            format!("real Caddy runtime exited with status {status}"),
            Some("inspect".to_string()),
          );
          self.set_snapshot(snapshot.clone());
          return snapshot;
        }
        Err(error) => {
          let snapshot = self.runtime_unhealthy_state(
            binary_path,
            process_id,
            Some(CADDY_ADMIN_ENDPOINT),
            "runtime-inspect-failed",
            format!("could not inspect real Caddy runtime: {error}"),
            Some("inspect".to_string()),
          );
          self.set_snapshot(snapshot.clone());
          return snapshot;
        }
      }
    }

    let snapshot = RuntimeState::idle();
    self.set_snapshot(snapshot.clone());
    snapshot
  }

  fn snapshot(&self) -> RuntimeState {
    self
      .snapshot
      .lock()
      .expect("runtime snapshot lock poisoned")
      .clone()
  }

  fn set_snapshot(&self, snapshot: RuntimeState) {
    *self
      .snapshot
      .lock()
      .expect("runtime snapshot lock poisoned") = snapshot;
  }

  fn runtime_running_state(
    &self,
    binary_path: Option<String>,
    process_id: Option<u32>,
    version: Option<String>,
  ) -> RuntimeState {
    RuntimeState {
      status: RuntimeStatus::Running,
      binary_path,
      version,
      process_id,
      admin_endpoint: Some(CADDY_ADMIN_ENDPOINT.to_string()),
      diagnostics: Vec::new(),
    }
  }

  fn runtime_unhealthy_state(
    &self,
    binary_path: Option<String>,
    process_id: Option<u32>,
    admin_endpoint: Option<&str>,
    code: &str,
    message: String,
    operation: Option<String>,
  ) -> RuntimeState {
    RuntimeState {
      status: RuntimeStatus::Unhealthy,
      binary_path,
      version: None,
      process_id,
      admin_endpoint: admin_endpoint.map(str::to_string),
      diagnostics: vec![cadder_ipc::RuntimeDiagnostic {
        code: code.to_string(),
        message,
        operation,
      }],
    }
  }

  pub async fn apply_config(&self, rendered: &[u8], logs: &CaddyLogStore) -> Result<()> {
    CaddyRuntime::from(self.clone())
      .apply_config(rendered, logs)
      .await
  }

  pub(super) async fn begin_apply_config(
    &self,
    rendered: &[u8],
    logs: &CaddyLogStore,
  ) -> Result<(ProcessRuntimeApplyReceipt, Result<()>)> {
    let previous_config = read_effective_config(&self.paths).await?;
    let staged = StagedRuntimeConfig::stage(&self.paths, rendered).await?;
    let was_running = self.runtime_is_running(logs).await?;

    let outcome = if was_running {
      self.reload(staged.path(), logs).await
    } else {
      self.start(staged.path(), logs).await
    };
    Ok((
      ProcessRuntimeApplyReceipt {
        runtime: self.clone(),
        staged,
        previous_config,
        was_running,
      },
      outcome,
    ))
  }

  pub(super) async fn begin_stop(
    &self,
    logs: &CaddyLogStore,
  ) -> Result<(ProcessRuntimeStopReceipt, Result<()>)> {
    let previous_config = read_effective_config(&self.paths).await?;
    let was_running = self.runtime_is_running(logs).await?;
    if was_running && previous_config.is_none() {
      anyhow::bail!("running Caddy runtime does not have an effective config for rollback");
    }
    let outcome = if was_running {
      self.stop().await
    } else {
      Ok(())
    };
    Ok((
      ProcessRuntimeStopReceipt {
        runtime: self.clone(),
        previous_config,
        was_running,
      },
      outcome,
    ))
  }

  pub(super) async fn start(&self, config_path: &Path, logs: &CaddyLogStore) -> Result<()> {
    if self.child.lock().await.is_some() {
      anyhow::bail!("real Caddy runtime already owns a child process");
    }
    let image = self.resolver.verify_for_spawn().await?;
    let binary = image.path().to_path_buf();
    let mut child = image
      .spawn("real Caddy runtime", |command| {
        command
          .arg("run")
          .arg("--config")
          .arg(config_path)
          .stdout(Stdio::piped())
          .stderr(Stdio::piped());
      })
      .await?;

    self.log_tasks.reopen();
    if let Some(stdout) = child.take_stdout() {
      spawn_log_reader(&self.log_tasks, stdout, logs.clone(), "stdout");
    }
    if let Some(stderr) = child.take_stderr() {
      spawn_log_reader(&self.log_tasks, stderr, logs.clone(), "stderr");
    }

    yield_now().await;
    match timeout(self.timeouts.start_check, child.wait()).await {
      Ok(Ok(status)) => {
        let exit = status
          .code()
          .map_or_else(|| status.to_string(), |code| format!("exit code {code}"));
        anyhow::bail!("real Caddy runtime exited immediately with {exit}");
      }
      Ok(Err(error)) => return Err(error).context("inspect started real Caddy runtime"),
      Err(_) => {}
    }

    if let Err(error) = self
      .wait_for_initial_config(&image, &mut child, config_path)
      .await
    {
      let cleanup_error = match child.try_wait() {
        Ok(Some(_)) => None,
        Ok(None) => child
          .terminate_and_join("real Caddy runtime after startup verification failed")
          .await
          .err(),
        Err(error) => Some(
          anyhow::Error::new(error).context("inspect real Caddy after startup verification failed"),
        ),
      };
      return Err(error).context(format!(
        "verify initial Caddy configuration; process cleanup error: {}",
        cleanup_error
          .as_ref()
          .map_or_else(|| "none".to_string(), |error| format!("{error:#}"))
      ));
    }

    let process_id = child.id();
    *self.child.lock().await = Some(OwnedRuntimeProcess {
      child,
      binary: binary.clone(),
    });
    self.set_snapshot(self.runtime_running_state(
      Some(binary.display().to_string()),
      process_id,
      Some(image.version().to_string()),
    ));
    logs
      .append(
        LogStreamIdentity::runtime_control(),
        LogSeverity::Info,
        "real Caddy runtime started",
        LogAttributionKind::RuntimeControl,
        Some("start".to_string()),
      )
      .await;
    Ok(())
  }

  async fn wait_for_initial_config(
    &self,
    image: &crate::caddy_image::VerifiedCaddyImage,
    child: &mut ProcessTreeChild,
    config_path: &Path,
  ) -> Result<()> {
    let deadline = Instant::now() + self.timeouts.reload;
    let retry = (|| async {
      let remaining = deadline.saturating_duration_since(Instant::now());
      submit_reload(image, config_path, remaining).await
    })
    .retry(
      ExponentialBuilder::default()
        .with_min_delay(Duration::from_millis(25))
        .with_max_delay(Duration::from_millis(250))
        .without_max_times(),
    );

    tokio::select! {
      status = child.wait() => {
        let status = status.context("wait for real Caddy during startup verification")?;
        anyhow::bail!("real Caddy runtime exited before accepting its initial configuration with status {status}");
      }
      result = timeout_at(deadline, retry) => {
        result
          .with_context(|| format!(
            "real Caddy did not accept its initial configuration within {} seconds",
            self.timeouts.reload.as_secs_f32()
          ))??;
      }
    }
    Ok(())
  }

  async fn reload(&self, config_path: &Path, logs: &CaddyLogStore) -> Result<()> {
    let image = self.resolver.verify_for_spawn().await?;
    match submit_reload(&image, config_path, self.timeouts.reload).await {
      Ok(()) => {
        logs
          .append(
            LogStreamIdentity::runtime_control(),
            LogSeverity::Info,
            "real Caddy runtime reloaded",
            LogAttributionKind::RuntimeControl,
            Some("reload".to_string()),
          )
          .await;
        Ok(())
      }
      Err(error) => {
        let message = format!("{error:#}");
        logs
          .append(
            LogStreamIdentity::runtime_control(),
            LogSeverity::Error,
            &message,
            LogAttributionKind::RuntimeControl,
            Some("reload".to_string()),
          )
          .await;
        Err(error)
      }
    }
  }

  pub async fn stop(&self) -> Result<()> {
    let deadline = Instant::now() + self.timeouts.stop_budget();
    let (result, quiescent) = self.stop_until(deadline).await.into_parts();
    if quiescent {
      return result;
    }
    self
      .force_stop()
      .await
      .context("force-stop real Caddy runtime after bounded stop failed")?;
    result
  }

  pub(crate) async fn stop_until(&self, deadline: Instant) -> RuntimeStopOutcome {
    let deadlines = RuntimeStopDeadlines::new(deadline, self.timeouts);
    let mut child_guard = match timeout_at(deadline, self.child.lock()).await {
      Ok(guard) => guard,
      Err(_) => {
        return RuntimeStopOutcome::new(
          Err(anyhow::anyhow!(
            "real Caddy runtime stop could not acquire child ownership before its deadline"
          )),
          false,
        );
      }
    };
    let mut stop_error = None;
    if let Some(owned) = child_guard.as_mut() {
      match owned.child.try_wait() {
        Ok(Some(_)) => *child_guard = None,
        Ok(None) => {
          stop_error = match timeout_at(deadlines.graceful, self.resolver.verify_for_spawn()).await
          {
            Ok(Ok(image)) => request_graceful_stop_until(image, deadlines.graceful, deadlines.stop)
              .await
              .err(),
            Ok(Err(error)) => Some(error.context("verify pinned Caddy image for graceful stop")),
            Err(_) => Some(anyhow::anyhow!(
              "verify pinned Caddy image for graceful stop exceeded its deadline"
            )),
          };

          match timeout_at(deadlines.stop, owned.child.wait()).await {
            Ok(Ok(_)) => *child_guard = None,
            Ok(Err(error)) => {
              return RuntimeStopOutcome::new(
                Err(error).context("wait for real Caddy runtime during stop"),
                false,
              );
            }
            Err(_) => {
              if let Err(error) = owned.child.start_kill() {
                return RuntimeStopOutcome::new(
                  Err(error).context("start kill for timed-out real Caddy runtime"),
                  false,
                );
              }
              match timeout_at(deadlines.kill, owned.child.wait()).await {
                Ok(Ok(_)) => {
                  *child_guard = None;
                  if stop_error.is_none() {
                    stop_error = Some(anyhow::anyhow!(
                      "real Caddy runtime did not stop within {} seconds and was killed",
                      self.timeouts.stop_wait.as_secs_f32()
                    ));
                  }
                }
                Ok(Err(error)) => {
                  return RuntimeStopOutcome::new(
                    Err(error).context("join killed real Caddy runtime"),
                    false,
                  );
                }
                Err(_) => {
                  return RuntimeStopOutcome::new(
                    Err(anyhow::anyhow!(
                      "real Caddy runtime kill did not complete before the shutdown deadline"
                    )),
                    false,
                  );
                }
              }
            }
          }
        }
        Err(error) => {
          return RuntimeStopOutcome::new(
            Err(error).context("inspect real Caddy runtime before stop"),
            false,
          );
        }
      }
    }
    drop(child_guard);

    self.set_snapshot(RuntimeState::idle());

    self.log_tasks.close();
    if timeout_at(deadline, self.log_tasks.wait()).await.is_err() {
      return RuntimeStopOutcome::new(
        Err(anyhow::anyhow!(
          "real Caddy runtime log readers did not join before the shutdown deadline"
        )),
        false,
      );
    }
    RuntimeStopOutcome::new(stop_error.map_or(Ok(()), Err), true)
  }

  pub(super) async fn force_stop(&self) -> Result<()> {
    let mut child_guard = self.child.lock().await;
    if let Some(owned) = child_guard.as_mut() {
      match owned.child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) => {
          owned
            .child
            .terminate_and_join("real Caddy runtime")
            .await
            .context("force-stop real Caddy runtime")?;
        }
        Err(error) => {
          return Err(error).context("inspect real Caddy runtime during force-stop");
        }
      }
      *child_guard = None;
    }
    drop(child_guard);
    self.set_snapshot(RuntimeState::idle());
    self.log_tasks.close();
    timeout(self.timeouts.kill_wait, self.log_tasks.wait())
      .await
      .context("join real Caddy runtime log readers during force-stop")?;
    Ok(())
  }

  async fn runtime_is_running(&self, logs: &CaddyLogStore) -> Result<bool> {
    let mut guard = self.child.lock().await;
    #[cfg(test)]
    if self.force_inspect_failure.load(Ordering::SeqCst) && guard.is_some() {
      anyhow::bail!("injected real Caddy runtime inspection failure");
    }
    let Some(owned) = guard.as_mut() else {
      return Ok(false);
    };
    let status = owned.child.try_wait();
    match status {
      Ok(None) => Ok(true),
      Ok(Some(status)) => {
        logs
          .append(
            LogStreamIdentity::runtime_control(),
            LogSeverity::Warn,
            format!("real Caddy runtime exited with status {status}; restarting"),
            LogAttributionKind::RuntimeControl,
            Some("runtime-liveness".to_string()),
          )
          .await;
        *guard = None;
        Ok(false)
      }
      Err(error) => {
        logs
          .append(
            LogStreamIdentity::runtime_control(),
            LogSeverity::Error,
            format!("could not inspect real Caddy runtime: {error}; retaining owned child"),
            LogAttributionKind::RuntimeControl,
            Some("runtime-liveness".to_string()),
          )
          .await;
        Err(error).context("inspect real Caddy runtime liveness")
      }
    }
  }

  #[cfg(test)]
  pub(super) fn force_inspect_failure_for_test(&self, enabled: bool) {
    self.force_inspect_failure.store(enabled, Ordering::SeqCst);
  }
}

mod io;
mod receipts;

pub(super) use io::*;
pub(crate) use receipts::{ProcessRuntimeApplyReceipt, ProcessRuntimeStopReceipt};
