use super::*;

pub(in crate::runtime) async fn submit_reload(
  image: &crate::caddy_image::VerifiedCaddyImage,
  config_path: &Path,
  wait: Duration,
) -> Result<()> {
  let child = image
    .spawn("real Caddy reload", |command| {
      command
        .arg("reload")
        .arg("--config")
        .arg(config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    })
    .await?;
  let output = child
    .wait_for_bounded_output(wait, "real Caddy reload", MAX_RUNTIME_COMMAND_OUTPUT_BYTES)
    .await?;
  if output.status.success() {
    return Ok(());
  }
  anyhow::bail!(
    "caddy reload failed: {}",
    String::from_utf8_lossy(&output.stderr).trim()
  );
}

pub(in crate::runtime) async fn request_graceful_stop_until(
  image: crate::caddy_image::VerifiedCaddyImage,
  wait_deadline: Instant,
  cleanup_deadline: Instant,
) -> Result<()> {
  let mut child = image
    .spawn("caddy stop", |command| {
      command
        .arg("stop")
        .arg("--address")
        .arg("localhost:2019")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    })
    .await?;
  let graceful_stop = wait_deadline.saturating_duration_since(Instant::now());
  let status = match timeout_at(wait_deadline, child.wait()).await {
    Ok(result) => result.context("wait for caddy stop")?,
    Err(_) => {
      let timeout_message = format!(
        "caddy stop timed out after {} seconds",
        graceful_stop.as_secs_f32()
      );
      child
        .start_kill()
        .with_context(|| format!("{timeout_message}; start kill for the stop helper"))?;
      timeout_at(cleanup_deadline, child.wait())
        .await
        .with_context(|| format!("{timeout_message}; helper cleanup exceeded its deadline"))?
        .with_context(|| format!("{timeout_message}; join the terminated stop helper"))?;
      anyhow::bail!(timeout_message);
    }
  };
  if !status.success() {
    anyhow::bail!("caddy stop failed with status {status}");
  }
  Ok(())
}

pub(in crate::runtime) fn spawn_log_reader<R>(
  tasks: &TaskTracker,
  reader: R,
  logs: CaddyLogStore,
  channel: &'static str,
) where
  R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
  tasks.spawn(async move {
    let mut reader = BufReader::new(reader).lines();
    while let Ok(Some(line)) = reader.next_line().await {
      let severity = if channel == "stderr" {
        LogSeverity::Error
      } else {
        LogSeverity::Info
      };
      logs
        .append(
          LogStreamIdentity {
            stream_id: "runtime".to_string(),
            domain_key: None,
            channel: channel.to_string(),
          },
          severity,
          line,
          LogAttributionKind::Runtime,
          None,
        )
        .await;
    }
  });
}
