use super::*;
use clap::CommandFactory;

#[test]
fn command_metadata_matches_release_identity() {
  let command = Args::command();

  assert_eq!(command.get_name(), "cadderd");
  assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
  assert_eq!(
    command.get_about().map(ToString::to_string),
    Some(env!("CARGO_PKG_DESCRIPTION").to_string())
  );
}

#[test]
fn short_help_uses_package_description() {
  let help = Args::command().render_help().to_string();

  assert!(
    help.contains(env!("CARGO_PKG_DESCRIPTION")),
    "short help output should include the package description: {help}"
  );
}

#[test]
fn long_help_describes_daemon_options() {
  let help = Args::command().render_long_help().to_string();

  assert!(
    help.contains("Absolute path used when Cadder starts the real Caddy executable"),
    "long help output should describe --real-caddy: {help}"
  );
  assert!(!help.contains("--runtime-dir"));
  assert!(!help.contains("--runtime-profile"));
  assert!(
    help.contains("Caddy backend mode used by the daemon"),
    "long help output should describe --caddy-backend: {help}"
  );
  assert!(
    help.contains("Start cadderd detached in the background"),
    "long help output should describe --background: {help}"
  );
}

#[test]
fn background_flag_is_explicit_detached_launcher() {
  let args = Args::parse_from(["cadderd", "--background", "--caddy-backend", "mock"]);

  assert!(args.background);
  assert!(!args.detach_ready);
  assert_eq!(args.caddy_backend, Some(CaddyBackendMode::Mock));
}

#[test]
fn runtime_selection_options_are_rejected() {
  let error = Args::try_parse_from(["cadderd", "--runtime-dir", "runtime-test"])
    .expect_err("removed runtime selection option should be rejected");

  assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
}

#[tokio::test]
async fn ctrl_c_registration_failure_keeps_detached_daemon_running() {
  let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
  let signal_shutdown_tx = shutdown_tx.clone();

  request_shutdown_after_ctrl_c(Err(io::Error::other("no console")), &signal_shutdown_tx);
  drop(signal_shutdown_tx);

  assert!(!*shutdown_rx.borrow());
  assert!(
    tokio::time::timeout(std::time::Duration::from_millis(10), shutdown_rx.changed())
      .await
      .is_err(),
    "a ctrl-c registration failure must not close the daemon shutdown channel"
  );
  drop(shutdown_tx);
}

#[test]
fn ctrl_c_signal_requests_daemon_shutdown() {
  let (shutdown_tx, shutdown_rx) = watch::channel(false);

  request_shutdown_after_ctrl_c(Ok(()), &shutdown_tx);

  assert!(*shutdown_rx.borrow());
}
