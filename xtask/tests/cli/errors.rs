#[test]
fn lightweight_command_parse_errors_do_not_run_heavy_build_steps() {
  let cases: &[&[&str]] = &[
    &["coverage", "--output"],
    &["definitely-not-a-heavy-command"],
    &["dist"],
    &["package", "--out", "target/package-without-platform"],
    &["runtime-installer", "--out"],
    &["verify-runtime-installer-dist", "--dir"],
  ];

  for args in cases {
    let output = Command::new(xtask_bin()).args(*args).output().unwrap();

    assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
      stderr.contains("a value is required")
        || stderr.contains("required arguments were not provided")
        || stderr.contains("unrecognized subcommand"),
      "{args:?} stderr did not describe the parse failure: {stderr}"
    );
  }
}

#[test]
fn unknown_command_reports_xtask_error() {
  let output = Command::new(xtask_bin())
    .arg("definitely-not-a-command")
    .output()
    .unwrap();

  assert!(!output.status.success());
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("unrecognized subcommand"));
}
